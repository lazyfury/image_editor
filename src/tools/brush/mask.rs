//! 一笔的覆盖率蒙版 + 按蒙版合成。
//!
//! 笔迹上的 stamp 是密集重叠的（间距 = 直径/4），如果每压一次就往像素上混
//! 一次颜色，标着 50% 不透明度的笔会画出接近全黑（同一像素被压 ~4 次）。
//!
//! 所以每一笔记一个**覆盖率蒙版**：重复压到的像素只记最大覆盖率，合成时从
//! *落笔前的快照* 重算 —— 一个像素在这一笔里只会被"最终覆盖率 × 不透明度"
//! 影响一次。抬笔后蒙版随笔触一起丢掉，所以两笔相交仍然是标准的 `1-(1-a)²`。

use std::rc::Rc;

use crate::canvas::Selection;
use crate::document::{Color, PixelBuffer, PixelRegion, Point};

use super::tip::Symmetry;
use super::BrushMode;

/// 覆盖率蒙版：每个像素在本笔里的最大覆盖率（`0..=255`）。
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeMask {
    width: u32,
    height: u32,
    coverage: Vec<u8>,
}

impl StrokeMask {
    /// 和图层缓冲区同尺寸的空蒙版（全 0）。
    pub fn new(width: u32, height: u32) -> Self {
        let len = (width as usize).saturating_mul(height as usize);
        Self {
            width,
            height,
            coverage: vec![0; len],
        }
    }

    /// 记下 `(x, y)` 本笔的最大覆盖率。
    ///
    /// 返回 `Some(peak)` 表示需要按 `peak` 重算这个像素；`None` 表示这次不比
    /// 之前更覆盖它（像素不用动）。越界也是 `None`。
    pub fn raise(&mut self, x: u32, y: u32, coverage: f32) -> Option<f32> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let value = (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
        let index = y as usize * self.width as usize + x as usize;
        if value == 0 || value <= self.coverage[index] {
            return None;
        }
        self.coverage[index] = value;
        Some(value as f32 / 255.0)
    }
}

/// 一次 stamp 的写入上下文：目标缓冲区 + 落笔前快照 + 本笔蒙版 + 笔参数。
///
/// 只做「文档像素 -> 缓冲区索引 -> 按蒙版合成」这一件事；几何在
/// [`super::tip`]，笔触状态在 [`super`]。
pub struct Writer<'a> {
    pub mode: BrushMode,
    pub color: Color,
    /// 每一笔的不透明度（乘在蒙版覆盖率上）。
    pub opacity: f32,
    /// 选区裁剪（`None` = 不裁）。
    pub clip: Option<Rc<Selection>>,
    /// 图层缓冲区原点（文档坐标）。
    pub offset: Point,
    /// 对称绘画：每个落笔像素再镜像到其它通道（轴 = 文档中线）。
    pub symmetry: Symmetry,
    /// 文档尺寸（镜像轴用）。
    pub document: (u32, u32),
    pub(super) before: &'a PixelBuffer,
    pub(super) mask: &'a mut StrokeMask,
    pub(super) target: &'a mut PixelBuffer,
    /// 写的是蒙版：颜色按亮度转成覆盖率（画黑 = 隐藏、画白 = 显示），
    /// 橡皮写黑。
    pub mask_mode: bool,
    /// 本笔**实际写过**的像素包围盒（文档坐标）：局部合成与撤销区域都用它。
    pub(super) dirty: Option<PixelRegion>,
}

impl Writer<'_> {
    /// 往文档坐标 `(doc_x, doc_y)` 合成 `coverage`，再按对称模式镜像。
    ///
    /// 镜像在**像素级**做，所以像素画也严格对称；重复压到同一像素由蒙版去重。
    pub fn put(&mut self, doc_x: i64, doc_y: i64, coverage: f32) {
        // 羽化过的选区让落笔按覆盖率衰减（硬选区里就是 0 / 1）。
        let coverage = match self.clip.as_ref() {
            Some(clip) => coverage * clip.coverage(doc_x.max(0) as u32, doc_y.max(0) as u32),
            None => coverage,
        };
        if coverage <= 0.0 {
            return;
        }
        self.put_one(doc_x, doc_y, coverage);
        for lane in 1..self.symmetry.lanes() {
            let Some((mx, my)) = self
                .symmetry
                .mirror_pixel(doc_x, doc_y, self.document, lane)
            else {
                continue;
            };
            self.put_one(mx, my, coverage);
        }
    }

    /// 本笔到目前为止的脏矩形（文档坐标）；一个像素都没写过就是 `None`。
    pub fn dirty(&self) -> Option<PixelRegion> {
        self.dirty
    }

    /// 只压 `(doc_x, doc_y)` 这一个像素；选区与图层边界在这里裁。
    fn put_one(&mut self, doc_x: i64, doc_y: i64, coverage: f32) {
        if doc_x < 0 || doc_y < 0 {
            return;
        }
        let bx = doc_x - self.offset.x as i64;
        let by = doc_y - self.offset.y as i64;
        if bx < 0 || by < 0 || bx >= self.target.width as i64 || by >= self.target.height as i64 {
            return;
        }
        let (bx, by) = (bx as u32, by as u32);
        let Some(peak) = self.mask.raise(bx, by, coverage) else {
            return;
        };
        // 只有真正合成过的像素才算脏（蒙版去重后的那些）。
        let touched = PixelRegion::new(doc_x as u32, doc_y as u32, 1, 1);
        self.dirty = Some(self.dirty.map_or(touched, |dirty| dirty.union(touched)));
        let alpha = peak * self.opacity;
        let base = self.before.get_pixel(bx, by);
        if self.mask_mode {
            // 蒙版只存灰度：画笔颜色取亮度，橡皮直接写黑（盖住）。
            let color = match self.mode {
                BrushMode::Paint => {
                    let Color { r, g, b, .. } = self.color;
                    let value = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32)
                        .round()
                        .clamp(0.0, 255.0) as u8;
                    Color::new(value, value, value, 255)
                }
                BrushMode::Erase => Color::BLACK,
            };
            self.target.blend_from(bx, by, base, color, alpha);
            return;
        }
        match self.mode {
            BrushMode::Paint => self.target.blend_from(bx, by, base, self.color, alpha),
            BrushMode::Erase => self.target.erase_from(bx, by, base, alpha),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个 8×8 的白色图层 + 它的快照 / 蒙版 / 写入上下文。
    struct Fixture {
        before: PixelBuffer,
        mask: StrokeMask,
        target: PixelBuffer,
    }

    impl Fixture {
        fn new() -> Self {
            let before = PixelBuffer::filled(8, 8, Color::WHITE);
            Self {
                mask: StrokeMask::new(8, 8),
                target: before.clone(),
                before,
            }
        }

        fn writer(&mut self) -> Writer<'_> {
            Writer {
                mode: BrushMode::Paint,
                color: Color::BLACK,
                opacity: 0.5,
                clip: None,
                offset: Point::ZERO,
                dirty: None,
                symmetry: Symmetry::Off,
                document: (8, 8),
                before: &self.before,
                mask: &mut self.mask,
                target: &mut self.target,
                mask_mode: false,
            }
        }

        fn pixel(&self, x: u32, y: u32) -> Color {
            self.target.get_pixel(x, y)
        }
    }

    #[test]
    fn a_pixel_is_composited_once_per_stroke_however_often_it_is_stamped() {
        // 同一位置压 4 次 50% 的 stamp：只能变一次（r ≈ 128），不能叠成黑。
        let mut fixture = Fixture::new();
        for _ in 0..4 {
            fixture.writer().put(2, 2, 1.0);
        }
        let color = fixture.pixel(2, 2);
        assert!((color.r as i32 - 128).abs() <= 1, "r = {}", color.r);
    }

    #[test]
    fn a_larger_coverage_still_updates_the_pixel() {
        // 先压 20% 再压 100%：结果是 100% 那一档（从快照重算，不是叠加）。
        let mut fixture = Fixture::new();
        fixture.writer().put(2, 2, 0.2);
        fixture.writer().put(2, 2, 1.0);
        let color = fixture.pixel(2, 2);
        assert!((color.r as i32 - 128).abs() <= 1, "r = {}", color.r);
    }

    #[test]
    fn an_erase_stroke_respects_the_mask_too() {
        let mut fixture = Fixture::new();
        for _ in 0..3 {
            let mut writer = fixture.writer();
            writer.mode = BrushMode::Erase;
            writer.put(3, 3, 1.0);
        }
        assert!((fixture.pixel(3, 3).a as i32 - 128).abs() <= 1);
    }

    #[test]
    fn symmetry_mirrors_the_composited_pixels() {
        let mut fixture = Fixture::new();
        let mut writer = fixture.writer();
        writer.symmetry = Symmetry::Both;
        writer.put(1, 1, 1.0);
        drop(writer);
        // 8×8 文档：四向镜像落在 (1,1) (6,1) (1,6) (6,6)。
        for (x, y) in [(1, 1), (6, 1), (1, 6), (6, 6)] {
            assert_ne!(fixture.pixel(x, y), Color::WHITE, "({x}, {y}) 应被压到");
        }
        assert_eq!(fixture.pixel(3, 3), Color::WHITE, "中间不该有东西");
    }

    #[test]
    fn the_writer_reports_the_region_it_actually_touched() {
        let mut fixture = Fixture::new();
        let mut writer = fixture.writer();
        writer.symmetry = Symmetry::Both;
        writer.put(2, 2, 1.0);
        writer.put(4, 5, 1.0);
        // 四向镜像 + 两次落笔 -> 一个覆盖全部的包围盒。
        assert_eq!(writer.dirty(), Some(PixelRegion::new(2, 2, 4, 4)));

        // 被选区挡掉的像素不算脏。
        let mut fixture = Fixture::new();
        let mut writer = fixture.writer();
        writer.clip = Some(Rc::new(Selection::rect(
            PixelRegion::new(0, 0, 1, 1),
            (8, 8),
        )));
        writer.put(6, 6, 1.0);
        assert_eq!(writer.dirty(), None);
    }

    #[test]
    fn the_selection_clips_the_stamp() {
        use crate::document::PixelRegion;

        let mut fixture = Fixture::new();
        let mut writer = fixture.writer();
        writer.clip = Some(Rc::new(Selection::rect(
            PixelRegion::new(0, 0, 4, 4),
            (8, 8),
        )));
        writer.put(2, 2, 1.0);
        writer.put(6, 6, 1.0);
        drop(writer);
        assert_ne!(fixture.pixel(2, 2), Color::WHITE, "选区内落笔");
        assert_eq!(fixture.pixel(6, 6), Color::WHITE, "选区外不落笔");
    }
}
