//! 笔尖几何：形状 / 直径 / 硬度，以及一个 stamp 覆盖到哪些像素、覆盖率多少。
//!
//! 这里只算几何，不碰像素缓冲区、颜色、选区（那些在 [`super::mask`] 与
//! [`super`] 里）。像素模式（`hard`）覆盖率二值、圆心吸附像素网格；否则
//! 按 [`Tip::hardness`] 在 `半径 × hardness` 以内铺满、往外线性淡到半径外
//! 半像素 —— 硬度 1 就只剩这一圈半透明边（等价于旧版「软笔」）。

use igui_core::Vec2;

/// 笔刷形状。`Square` 更适合像素画（偶数尺寸是整数边长，不会出十字 / 多一格）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrushShape {
    Round,
    #[default]
    Square,
}

/// 对称绘画：以文档中线为轴，把每个 dab 额外压到镜像位置。
///
/// 轴固定在文档中心（`x' = width - 1 - x`、`y' = height - 1 - y`），所以镜像点永远
/// 落在文档里；落在文档外的原点（用户在画布外落笔）不镜像。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Symmetry {
    #[default]
    Off,
    Vertical,
    Horizontal,
    Both,
}

impl Symmetry {
    /// 循环切换（选项栏按钮用）。
    pub const fn next(self) -> Self {
        match self {
            Self::Off => Self::Vertical,
            Self::Vertical => Self::Horizontal,
            Self::Horizontal => Self::Both,
            Self::Both => Self::Off,
        }
    }

    /// 选项栏上的短名字。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "关",
            Self::Vertical => "垂直",
            Self::Horizontal => "水平",
            Self::Both => "四向",
        }
    }

    /// 这次落笔要走几个通道。
    pub const fn lanes(self) -> usize {
        match self {
            Self::Off => 1,
            Self::Vertical | Self::Horizontal => 2,
            Self::Both => 4,
        }
    }

    /// 第 `lane` 个通道上的**像素**坐标（`lane = 0` 是原位置）。
    ///
    /// 镜像在像素级做（`x' = width - 1 - x`），所以像素画也是严格对称的；
    /// 镜像点落在文档外时返回 `None`。
    pub fn mirror_pixel(
        self,
        x: i64,
        y: i64,
        document: (u32, u32),
        lane: usize,
    ) -> Option<(i64, i64)> {
        if lane >= self.lanes() {
            return None;
        }
        // 通道号的含义按模式定：四向用两位（低位 x、高位 y），单向只有一个镜像。
        let (flip_x, flip_y) = match self {
            Self::Off => (false, false),
            Self::Vertical => (lane == 1, false),
            Self::Horizontal => (false, lane == 1),
            Self::Both => (lane % 2 == 1, lane / 2 == 1),
        };
        let mirrored = (
            if flip_x { document.0 as i64 - 1 - x } else { x },
            if flip_y { document.1 as i64 - 1 - y } else { y },
        );
        let inside = mirrored.0 >= 0
            && mirrored.1 >= 0
            && mirrored.0 < document.0 as i64
            && mirrored.1 < document.1 as i64;
        inside.then_some(mirrored)
    }
}

/// 抗锯齿过渡带宽度（像素）：硬度 1 时只剩这一圈半透明边。
const EDGE_AA: f32 = 0.5;

/// 像素索引的浮点容差（像素单位，远小于 1 像素）。
///
/// 平滑曲线上的点由浮点运算得出，正好落在整数上时可能有 `1e-7` 的误差；
/// 加一个容差，避免 `1.9999999` 被算到上一格。
const PIXEL_EPSILON: f32 = 1e-3;

/// 一支笔尖：几何（直径 / 硬度 / 形状 / 像素模式）+ 落笔间距。
#[derive(Debug, Clone, PartialEq)]
pub struct Tip {
    /// 直径，文档像素。
    pub size: f32,
    /// 0..=1。1 = 只有 1px 抗锯齿边（硬边），0 = 从中心一路渐变到边缘。
    pub hardness: f32,
    pub shape: BrushShape,
    /// 像素模式：覆盖率二值、圆心吸附到像素网格。
    pub hard: bool,
    /// 落笔间距相对直径的比例。
    pub spacing_ratio: f32,
}

impl Default for Tip {
    fn default() -> Self {
        Self {
            size: 1.0,
            hardness: 1.0,
            shape: BrushShape::Square,
            hard: true,
            spacing_ratio: 0.25,
        }
    }
}

impl Tip {
    pub const MIN_SIZE: f32 = 1.0;
    pub const MAX_SIZE: f32 = 200.0;

    /// 落笔点之间的固定间距（文档像素）。
    ///
    /// 取直径的 1/4：相邻 stamp 有重叠、线段不会断开也不会出串珠边。像素笔
    /// 额外夹到至少 1px —— 小于一个像素的步长只会把同一个像素反复压。
    pub fn spacing(&self) -> f32 {
        let step = (self.size * self.spacing_ratio).max(super::stroke::MIN_SPACING);
        if self.hard {
            step.max(1.0)
        } else {
            step
        }
    }

    /// 像素模式把圆心吸附到光标下的像素中心；其余模式保持原位置。
    pub fn snap(&self, center: Vec2) -> Vec2 {
        if self.hard {
            Vec2::new(
                Self::pixel_index(center.x) as f32 + 0.5,
                Self::pixel_index(center.y) as f32 + 0.5,
            )
        } else {
            center
        }
    }

    /// 文档坐标 -> 像素索引：像素覆盖 `[i, i + 1)`，所以取整就是 `floor`。
    pub fn pixel_index(value: f32) -> i64 {
        (value + PIXEL_EPSILON).floor() as i64
    }

    /// 遍历这个 stamp 覆盖到的整数像素（文档坐标）：`visit(x, y, coverage)`。
    ///
    /// 覆盖率落在 `(0, 1]`；0 的像素不会回调（大笔刷边缘上省一半循环）。
    pub fn cover(&self, center: Vec2, mut visit: impl FnMut(i64, i64, f32)) {
        let center = self.snap(center);
        let radius = (self.size * 0.5).max(0.5);
        // 方形硬边的整数边长：奇数 n×n 居中，偶数把光标像素放左上（避免多一格）。
        let side = self.size.round().max(1.0) as i64;
        let half = side / 2;
        let square_min = -half + i64::from(side % 2 == 0);
        let square_max = half;
        let center_px = (Self::pixel_index(center.x), Self::pixel_index(center.y));

        let min_x = (center.x - radius - 1.0).floor() as i64;
        let max_x = (center.x + radius + 1.0).ceil() as i64;
        let min_y = (center.y - radius - 1.0).floor() as i64;
        let max_y = (center.y + radius + 1.0).ceil() as i64;

        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let dx = x as f32 + 0.5 - center.x;
                let dy = y as f32 + 0.5 - center.y;
                let coverage = if self.hard {
                    match self.shape {
                        BrushShape::Round => {
                            if (dx * dx + dy * dy).sqrt() <= radius {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BrushShape::Square => {
                            let ix = x - center_px.0;
                            let iy = y - center_px.1;
                            if ix >= square_min
                                && ix <= square_max
                                && iy >= square_min
                                && iy <= square_max
                            {
                                1.0
                            } else {
                                0.0
                            }
                        }
                    }
                } else {
                    self.soft_coverage(radius, dx, dy)
                };
                if coverage > 0.0 {
                    visit(x, y, coverage);
                }
            }
        }
    }

    /// 软边覆盖率：实心核心到 `半径 × 硬度`，往外线性淡出到 `半径 + 0.5`
    /// （圆用欧氏距离，方用切比雪夫距离 = 软方形）。
    ///
    /// 硬度 1 时 `span` 正好是 1px，退化成旧版 `(radius + 0.5 - 距离).clamp(0, 1)`。
    fn soft_coverage(&self, radius: f32, dx: f32, dy: f32) -> f32 {
        let metric = match self.shape {
            BrushShape::Round => (dx * dx + dy * dy).sqrt(),
            BrushShape::Square => dx.abs().max(dy.abs()),
        };
        let hardness = self.hardness.clamp(0.0, 1.0);
        let inner = (radius * hardness - EDGE_AA).max(0.0);
        let span = (radius + EDGE_AA - inner).max(f32::EPSILON);
        ((radius + EDGE_AA - metric) / span).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 不旋转的图章：圆心 (16, 16)、直径 8。
    fn stamp8() -> Vec2 {
        Vec2::new(16.0, 16.0)
    }

    fn tip(size: f32, hardness: f32) -> Tip {
        Tip {
            size,
            hardness,
            shape: BrushShape::Round,
            hard: false,
            ..Tip::default()
        }
    }

    /// 圆笔某一行的覆盖率（文档坐标，圆心在 `(16, 16)`）。
    fn coverage_at(tip: &Tip, x: i64, y: i64) -> f32 {
        let mut found = 0.0;
        tip.cover(stamp8(), |px, py, coverage| {
            if px == x && py == y {
                found = coverage;
            }
        });
        found
    }

    /// 整个 stamp 里「既不是 0 也不是 1」的像素数（过渡带有多宽）。
    fn partial_pixels(tip: &Tip) -> usize {
        let mut partial = 0;
        tip.cover(stamp8(), |_x, _y, coverage| {
            if coverage > 0.0 && coverage < 1.0 {
                partial += 1;
            }
        });
        partial
    }

    #[test]
    fn hardness_one_keeps_the_old_one_pixel_anti_aliased_edge() {
        // 旧版软笔就是硬度 1：edge = (radius + 0.5 - 距离).clamp(0, 1)。
        let tip = tip(10.0, 1.0);
        let radius = 5.0;
        for y in 12..=20 {
            for x in 12..=20 {
                let dx = x as f32 + 0.5 - 16.0;
                let dy = y as f32 + 0.5 - 16.0;
                let metric = (dx * dx + dy * dy).sqrt();
                let expected = (radius + 0.5 - metric).clamp(0.0, 1.0);
                assert!(
                    (coverage_at(&tip, x, y) - expected).abs() < 1e-6,
                    "({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn a_lower_hardness_widens_the_gradient_band() {
        let hard = tip(20.0, 1.0);
        let soft = tip(20.0, 0.0);
        // 落笔点所在像素（像素中心离圆心 0.7px）两个都是接近实心。
        assert_eq!(coverage_at(&hard, 16, 16), 1.0);
        assert!(
            coverage_at(&soft, 16, 16) > 0.9,
            "{}",
            coverage_at(&soft, 16, 16)
        );
        // 硬度 1 只有一圈抗锯齿边；硬度 0 半个盘子都在渐变。
        assert!(
            partial_pixels(&soft) > partial_pixels(&hard) * 4,
            "soft {} vs hard {}",
            partial_pixels(&soft),
            partial_pixels(&hard)
        );
    }

    #[test]
    fn coverage_falls_off_monotonically_from_the_centre() {
        for hardness in [0.0, 0.25, 0.5, 1.0] {
            let tip = tip(20.0, hardness);
            let mut previous = 1.0;
            for offset in 0..=12 {
                let coverage = coverage_at(&tip, 16 + offset, 16);
                assert!(
                    coverage <= previous + 1e-6,
                    "硬度 {hardness}：offset {offset} 回升了"
                );
                previous = coverage;
            }
            assert_eq!(coverage_at(&tip, 40, 16), 0.0);
        }
    }

    #[test]
    fn pixel_mode_ignores_hardness_and_keeps_binary_coverage() {
        let mut tip = tip(10.0, 0.0);
        tip.hard = true;
        tip.cover(stamp8(), |_x, _y, coverage| {
            assert_eq!(coverage, 1.0, "像素模式只有 0 / 1");
        });
    }

    #[test]
    fn symmetry_mirrors_at_the_pixel_level() {
        let document = (128, 64);
        // 关掉只有一个通道。
        assert_eq!(Symmetry::Off.mirror_pixel(10, 20, document, 1), None);
        // 垂直：x' = width - 1 - x（像素级，像素画也严格对称）。
        assert_eq!(
            Symmetry::Vertical.mirror_pixel(10, 20, document, 1),
            Some((117, 20))
        );
        // 水平：y' = height - 1 - y。
        assert_eq!(
            Symmetry::Horizontal.mirror_pixel(10, 20, document, 1),
            Some((10, 43))
        );
        // 四向：四个通道都不同。
        let both: Vec<(i64, i64)> = (0..4)
            .filter_map(|lane| Symmetry::Both.mirror_pixel(10, 20, document, lane))
            .collect();
        assert_eq!(both, vec![(10, 20), (117, 20), (10, 43), (117, 43)]);
    }

    #[test]
    fn a_mirror_outside_the_document_is_dropped() {
        // 原位置在文档外（画布外落笔）时镜像会落到文档外：不画。
        assert_eq!(Symmetry::Vertical.mirror_pixel(-5, 20, (128, 128), 1), None);
        // 文档中心附近的镜像仍然在文档内。
        assert_eq!(
            Symmetry::Vertical.mirror_pixel(0, 0, (128, 128), 1),
            Some((127, 0))
        );
    }

    #[test]
    fn symmetry_cycles_off_vertical_horizontal_both() {
        let mut mode = Symmetry::Off;
        let mut seen = Vec::new();
        for _ in 0..4 {
            mode = mode.next();
            seen.push(mode.label());
        }
        assert_eq!(seen, vec!["垂直", "水平", "四向", "关"]);
    }

    #[test]
    fn a_hard_square_tip_covers_an_integer_block() {
        // 直径 6 的硬边方形笔：正好 6×6 个像素，每个都是实心。
        let tip = Tip {
            size: 6.0,
            hardness: 0.0,
            shape: BrushShape::Square,
            hard: true,
            ..Tip::default()
        };
        let mut pixels = Vec::new();
        tip.cover(Vec2::new(16.5, 16.5), |x, y, coverage| {
            assert_eq!(coverage, 1.0);
            pixels.push((x, y));
        });
        assert_eq!(pixels.len(), 36);
        let min_x = pixels.iter().map(|(x, _)| *x).min().unwrap();
        let max_x = pixels.iter().map(|(x, _)| *x).max().unwrap();
        assert_eq!((min_x, max_x), (14, 19), "偶数边长不偏格");
    }
}
