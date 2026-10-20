//! 任意形状的选区：与文档同尺寸的**覆盖率**掩码 + 描边轮廓。
//!
//! 框选是矩形、套索是自由折线、魔棒是容差连通区域，统一成一张 `Vec<u8>`
//! 掩码（0 = 没选中、255 = 完全选中）：`contains` 是 O(1)，画笔 / 填充按
//! [`Selection::coverage`] 衰减，所以羽化过的选区也能用（边缘渐变）。
//! 轮廓是一条条**线段**（文档坐标）—— 矩形 4 段、套索 N 段、魔棒是掩码的
//! 边界边（按段合并），给「马赛克线段」描边用，所以边框始终贴着真实选区。

use igui_core::Vec2;

use crate::document::PixelRegion;

/// 一条轮廓线段（文档坐标）。
pub type Edge = (Vec2, Vec2);

/// 与文档同尺寸的像素掩码 + 描边轮廓。
///
/// `area` / `bounds` 在构造时算一次（4K 掩码扫一遍要 40 ms，`is_empty()` 之类的
/// 谓词不能每次都扫），`outline` 是**合并过**的边界线段（整块矩形只有 4 条）。
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    width: u32,
    height: u32,
    /// 覆盖率：0 = 未选中、255 = 完全选中（羽化后中间值就是渐变边缘）。
    mask: Vec<u8>,
    outline: Vec<Edge>,
    /// 选中的像素个数。
    area: usize,
    /// 选中像素的包围盒；一个都没有时是 `None`。
    bounds: Option<PixelRegion>,
}

impl Selection {
    /// 矩形选区（框选）。区域会裁剪到文档内。
    pub fn rect(region: PixelRegion, document: (u32, u32)) -> Self {
        let (width, height) = document;
        let mut mask = vec![0u8; width as usize * height as usize];
        let x0 = region.x.min(width);
        let y0 = region.y.min(height);
        let x1 = region.right().min(width);
        let y1 = region.bottom().min(height);
        for y in y0..y1 {
            let row = y as usize * width as usize;
            mask[row + x0 as usize..row + x1 as usize].fill(255);
        }
        let corners = [
            Vec2::new(x0 as f32, y0 as f32),
            Vec2::new(x1 as f32, y0 as f32),
            Vec2::new(x1 as f32, y1 as f32),
            Vec2::new(x0 as f32, y1 as f32),
        ];
        let area = (x1.saturating_sub(x0) as usize) * (y1.saturating_sub(y0) as usize);
        Self {
            width,
            height,
            mask,
            outline: closed_edges(&corners),
            area,
            bounds: (area > 0).then(|| PixelRegion::new(x0, y0, x1 - x0, y1 - y0)),
        }
    }

    /// 整个文档（「全选」）。
    ///
    /// 不扫掩码：面积 / 包围盒是解析值，轮廓就是文档外框（4 条边）。
    pub fn all(document: (u32, u32)) -> Self {
        let (width, height) = document;
        let corners = [
            Vec2::new(0.0, 0.0),
            Vec2::new(width as f32, 0.0),
            Vec2::new(width as f32, height as f32),
            Vec2::new(0.0, height as f32),
        ];
        let area = width as usize * height as usize;
        Self {
            width,
            height,
            mask: vec![255u8; area],
            outline: closed_edges(&corners),
            area,
            bounds: (area > 0).then(|| PixelRegion::new(0, 0, width, height)),
        }
    }

    /// 反选：掩码取反，轮廓按新掩码的边界重算（没有选区时就是全选）。
    pub fn invert(&self) -> Self {
        let mask: Vec<u8> = self.mask.iter().map(|coverage| 255 - coverage).collect();
        Self::from_mask((self.width, self.height), mask)
    }

    /// 羽化：把掩码做两遍盒式模糊（近似高斯），边缘从 255 平滑淡到 0。
    ///
    /// `radius` 是文档像素；`0` 或负值返回原样。两遍盒式模糊足够平滑，
    /// 而且代价只有两趟 O(宽 × 高)，4K 上也就几十毫秒（一次点击的事）。
    pub fn feathered(&self, radius: f32) -> Self {
        let radius = radius.round() as i64;
        if radius < 1 {
            return self.clone();
        }
        let (width, height) = (self.width as i64, self.height as i64);
        if width == 0 || height == 0 {
            return self.clone();
        }
        let mut mask: Vec<u8> = self.mask.clone();
        for _ in 0..2 {
            mask = box_blur(&mask, width, height, radius, false);
            mask = box_blur(&mask, width, height, radius, true);
        }
        Self::from_mask((self.width, self.height), mask)
    }

    /// 自由折线围成的选区（套索）。偶奇规则扫描线光栅化，取像素中心。
    pub fn polygon(points: &[Vec2], document: (u32, u32)) -> Self {
        let (width, height) = document;
        let mut mask = vec![0u8; width as usize * height as usize];
        if points.len() >= 3 {
            for y in 0..height {
                let scan = y as f32 + 0.5;
                let mut crossings: Vec<f32> = Vec::new();
                for index in 0..points.len() {
                    let a = points[index];
                    let b = points[(index + 1) % points.len()];
                    if (a.y <= scan && b.y > scan) || (b.y <= scan && a.y > scan) {
                        let t = (scan - a.y) / (b.y - a.y);
                        crossings.push(a.x + t * (b.x - a.x));
                    }
                }
                crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                for pair in crossings.chunks(2) {
                    if pair.len() < 2 {
                        break;
                    }
                    let start = ((pair[0] - 0.5).ceil() as i64).max(0);
                    let end = ((pair[1] - 0.5).ceil() as i64).min(width as i64);
                    for x in start..end {
                        mask[y as usize * width as usize + x as usize] = 255;
                    }
                }
            }
        }
        finish(width, height, mask, closed_edges(points))
    }

    /// 从一个与文档同尺寸的覆盖率掩码构造（魔棒的容差区域 / 羽化结果）。
    ///
    /// 轮廓取「覆盖率 ≥ 50%」那圈边界（羽化后描边落在视觉边缘上）。
    pub fn from_mask(document: (u32, u32), mask: Vec<u8>) -> Self {
        let (width, height) = document;
        debug_assert_eq!(mask.len(), width as usize * height as usize);
        let outline = mask_edges(width, height, &mask);
        finish(width, height, mask, outline)
    }

    /// 像素的选中程度（0..=1）：羽化过的选区边缘是渐变的。
    pub fn coverage(&self, x: u32, y: u32) -> f32 {
        if x >= self.width || y >= self.height {
            return 0.0;
        }
        self.mask[y as usize * self.width as usize + x as usize] as f32 / 255.0
    }

    /// 像素是否（至少部分）被选中（O(1)）。
    pub fn contains(&self, x: u32, y: u32) -> bool {
        x < self.width
            && y < self.height
            && self.mask[y as usize * self.width as usize + x as usize] > 0
    }

    /// 选中的像素个数（构造时算好的，O(1)）。
    pub fn area(&self) -> usize {
        self.area
    }

    /// 有没有选中任何像素（套索画出退化的形状时用它判断）。
    pub fn is_empty(&self) -> bool {
        self.area() == 0
    }

    /// 选中像素的包围盒；没有选中任何像素返回 `None`（构造时算好的，O(1)）。
    pub fn bounds(&self) -> Option<PixelRegion> {
        self.bounds
    }

    /// 并集（加选）：两块掩码取或，面积 / 包围盒 / 轮廓跟着重算。
    ///
    /// 尺寸不同（文档换了）时退回 `self`，不 panic。
    pub fn union(&self, other: &Self) -> Self {
        if (self.width, self.height) != (other.width, other.height) {
            return self.clone();
        }
        // 并集取较大覆盖率（硬掩码下等价于逻辑或）。
        let mask = self
            .mask
            .iter()
            .zip(&other.mask)
            .map(|(a, b)| (*a).max(*b))
            .collect();
        Self::from_mask((self.width, self.height), mask)
    }

    /// 差集（减选）：从自己身上挖掉 `other`。
    pub fn subtract(&self, other: &Self) -> Self {
        if (self.width, self.height) != (other.width, other.height) {
            return self.clone();
        }
        // 差集按覆盖率相减（硬掩码下等价于 a && !b）。
        let mask = self
            .mask
            .iter()
            .zip(&other.mask)
            .map(|(a, b)| a.saturating_sub(*b))
            .collect();
        Self::from_mask((self.width, self.height), mask)
    }

    /// 描边用的轮廓线段（文档坐标）。
    pub fn outline(&self) -> &[Edge] {
        &self.outline
    }
}

/// 新选区怎么合到旧选区上：框选 / 套索 / 魔棒按住 Shift 加、Alt 减。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MergeMode {
    /// 直接替换旧选区。
    #[default]
    Replace,
    /// 并集（Shift）。
    Add,
    /// 差集（Alt）。
    Subtract,
}

impl MergeMode {
    /// 从修饰键推：`Alt` 优先于 `Shift`（和 PS 一致）。
    pub const fn from_modifiers(add: bool, subtract: bool) -> Self {
        if subtract {
            Self::Subtract
        } else if add {
            Self::Add
        } else {
            Self::Replace
        }
    }

    /// 把 `next` 合到 `base` 上；结果为空就返回 `None`（等于没有选区）。
    pub fn apply(self, base: Option<&Selection>, next: Selection) -> Option<Selection> {
        let merged = match (self, base) {
            (Self::Add, Some(base)) => base.union(&next),
            (Self::Subtract, Some(base)) => base.subtract(&next),
            _ => next,
        };
        (!merged.is_empty()).then_some(merged)
    }
}

/// 把一串点收成闭合的线段（最后一段回到起点）。
fn closed_edges(points: &[Vec2]) -> Vec<Edge> {
    if points.len() < 2 {
        return Vec::new();
    }
    (0..points.len())
        .map(|index| (points[index], points[(index + 1) % points.len()]))
        .collect()
}

/// 掩码 -> 选区：面积与包围盒扫一遍掩码，轮廓用合并过的边界线段。
fn finish(width: u32, height: u32, mask: Vec<u8>, outline: Vec<Edge>) -> Selection {
    let mut sum = 0usize;
    let (mut min_x, mut min_y) = (u32::MAX, u32::MAX);
    let (mut max_x, mut max_y) = (0u32, 0u32);
    for (index, coverage) in mask.iter().enumerate() {
        if *coverage == 0 {
            continue;
        }
        sum += *coverage as usize;
        let (x, y) = (
            (index % width as usize) as u32,
            (index / width as usize) as u32,
        );
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    // 「一个满覆盖像素」= 255，所以羽化后的面积按覆盖率折算。
    let area = sum / 255;
    let bounds =
        (sum > 0).then(|| PixelRegion::new(min_x, min_y, max_x - min_x + 1, max_y - min_y + 1));
    Selection {
        width,
        height,
        mask,
        outline,
        area,
        bounds,
    }
}

/// 一维盒式模糊（`vertical = false` 沿行、`true` 沿列）。
///
/// 越界按 **0**（未选中）算 —— 和 PS 一致：选区贴着画布边缘时，羽化会把那一边
/// 向内淡出，而不是当作「外面也选中」。
fn box_blur(mask: &[u8], width: i64, height: i64, radius: i64, vertical: bool) -> Vec<u8> {
    let mut out = vec![0u8; mask.len()];
    let (outer, inner) = if vertical {
        (width, height)
    } else {
        (height, width)
    };
    let index = |o: i64, i: i64| -> usize {
        if vertical {
            (i * width + o) as usize
        } else {
            (o * width + i) as usize
        }
    };
    let sample = |o: i64, i: i64| -> u32 {
        if i < 0 || i >= inner {
            0
        } else {
            mask[index(o, i)] as u32
        }
    };
    let window = (2 * radius + 1) as u32;
    for o in 0..outer {
        let mut sum: u32 = (-radius..=radius).map(|i| sample(o, i)).sum();
        for i in 0..inner {
            out[index(o, i)] = (sum / window) as u8;
            sum = sum + sample(o, i + radius + 1) - sample(o, i - radius);
        }
    }
    out
}

/// 掩码的边界边，**按段合并**：整块矩形只有 4 条边，4K 全选不再产出上万条线段。
///
/// 横边逐条行边界比较「上一行 vs 这一行」，竖边逐条列边界比较左右两侧；相邻的
/// 差异段合成一条线段，所以描边（蚂蚁线）命令数从「每像素一条」降到「每段一条」。
fn mask_edges(width: u32, height: u32, mask: &[u8]) -> Vec<Edge> {
    // 描边画在「覆盖率 ≥ 50%」那圈上：羽化后的边界看起来才在选区边缘。
    let selected = |x: i64, y: i64| -> bool {
        x >= 0
            && y >= 0
            && x < width as i64
            && y < height as i64
            && mask[y as usize * width as usize + x as usize] >= 128
    };
    let mut edges = Vec::new();

    // 横边：行边界 y 两侧选中状态不同的区间。
    for y in 0..=height as i64 {
        let mut x = 0i64;
        while x < width as i64 {
            if selected(x, y - 1) == selected(x, y) {
                x += 1;
                continue;
            }
            let start = x;
            while x < width as i64 && selected(x, y - 1) != selected(x, y) {
                x += 1;
            }
            edges.push((
                Vec2::new(start as f32, y as f32),
                Vec2::new(x as f32, y as f32),
            ));
        }
    }

    // 竖边：列边界 x 两侧选中状态不同的区间。
    for x in 0..=width as i64 {
        let mut y = 0i64;
        while y < height as i64 {
            if selected(x - 1, y) == selected(x, y) {
                y += 1;
                continue;
            }
            let start = y;
            while y < height as i64 && selected(x - 1, y) != selected(x, y) {
                y += 1;
            }
            edges.push((
                Vec2::new(x as f32, start as f32),
                Vec2::new(x as f32, y as f32),
            ));
        }
    }

    edges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_all_covers_every_pixel_and_its_outline_is_the_document_border() {
        let selection = Selection::all((16, 12));
        assert_eq!(selection.area(), 16 * 12);
        assert!(selection.contains(0, 0) && selection.contains(15, 11));
        // 边界线按段合并：整块矩形只剩外框的 4 条边（以前是每像素一条，共 56 条）。
        assert_eq!(selection.outline().len(), 4, "{:?}", selection.outline());
    }

    #[test]
    fn invert_swaps_inside_and_outside() {
        let selection = Selection::rect(PixelRegion::new(2, 2, 4, 4), (10, 10));
        let inverted = selection.invert();
        assert_eq!(inverted.area(), 10 * 10 - selection.area());
        assert!(!inverted.contains(3, 3), "原来选中的现在不选中");
        assert!(inverted.contains(0, 0) && inverted.contains(9, 9));
        // 轮廓仍然是闭合的一圈（能画出来），不是空的。
        assert!(!inverted.outline().is_empty());
    }

    #[test]
    fn inverting_twice_returns_the_original_mask() {
        // 掩码来回一趟不变；轮廓会按掩码边界重算（套索的折线轮廓换成像素边），
        // 所以只比掩码。
        let selection = Selection::polygon(
            &[
                Vec2::new(1.0, 1.0),
                Vec2::new(8.0, 2.0),
                Vec2::new(5.0, 9.0),
            ],
            (10, 10),
        );
        let twice = selection.invert().invert();
        assert_eq!(twice.area(), selection.area());
        for y in 0..10 {
            for x in 0..10 {
                assert_eq!(twice.contains(x, y), selection.contains(x, y), "({x}, {y})");
            }
        }
    }

    #[test]
    fn a_wand_style_mask_gets_merged_edges_and_cached_area() {
        // 4×4 文档，选中一竖条的中间两像素：包围盒、面积都对，边按段合并。
        let mut mask = vec![0u8; 16];
        for y in 1..3 {
            mask[y * 4 + 2] = 255;
        }
        let selection = Selection::from_mask((4, 4), mask);
        assert_eq!(selection.area(), 2);
        assert_eq!(selection.bounds(), Some(PixelRegion::new(2, 1, 1, 2)));
        // 横边 2 条（上下各一）+ 竖边 2 条（左右各一），都是合并后的段。
        assert_eq!(selection.outline().len(), 4);
        assert!(!selection.is_empty());
        assert_eq!(selection.area(), 2, "area 是缓存的，不重复扫掩码");
    }

    #[test]
    fn an_empty_mask_has_no_bounds() {
        let selection = Selection::from_mask((4, 4), vec![0u8; 16]);
        assert_eq!(selection.area(), 0);
        assert!(selection.is_empty());
        assert_eq!(selection.bounds(), None);
        assert!(selection.outline().is_empty());
    }

    #[test]
    fn union_and_subtract_combine_the_masks() {
        let left = Selection::rect(PixelRegion::new(0, 0, 4, 4), (10, 10));
        let right = Selection::rect(PixelRegion::new(2, 0, 4, 4), (10, 10));
        // 并集：0..6 一共 6×4 个像素（中间重叠 2 列）。
        assert_eq!(left.union(&right).area(), 6 * 4);
        // 差集：左边去掉重叠的 2 列。
        assert_eq!(left.subtract(&right).area(), 2 * 4);
        // 完全挖空 -> 空选区。
        assert_eq!(left.subtract(&left).area(), 0);
        // 尺寸不同不 panic，退回原样。
        let other_doc = Selection::rect(PixelRegion::new(0, 0, 2, 2), (20, 20));
        assert_eq!(left.union(&other_doc), left);
    }

    #[test]
    fn merge_mode_follows_the_modifier_keys() {
        assert_eq!(MergeMode::from_modifiers(false, false), MergeMode::Replace);
        assert_eq!(MergeMode::from_modifiers(true, false), MergeMode::Add);
        assert_eq!(MergeMode::from_modifiers(false, true), MergeMode::Subtract);
        assert_eq!(
            MergeMode::from_modifiers(true, true),
            MergeMode::Subtract,
            "Alt 优先"
        );
    }

    #[test]
    fn merging_without_a_base_selection_is_just_the_new_one() {
        let next = Selection::rect(PixelRegion::new(1, 1, 2, 2), (8, 8));
        assert_eq!(
            MergeMode::Add.apply(None, next.clone()).map(|s| s.area()),
            Some(4)
        );
        assert_eq!(
            MergeMode::Subtract
                .apply(None, next.clone())
                .map(|s| s.area()),
            Some(4),
            "没有旧选区时减选 = 新选区"
        );
        // 减空了就没有选区。
        let base = Selection::rect(PixelRegion::new(0, 0, 8, 8), (8, 8));
        assert_eq!(MergeMode::Subtract.apply(Some(&base), base.clone()), None);
    }

    #[test]
    fn feathering_fades_the_edges_without_moving_them() {
        // 16×16 全选羽化 2px：外圈变淡、中心仍然是满覆盖，描边仍在外框上。
        let selection = Selection::all((16, 16)).feathered(2.0);
        assert!(
            selection.coverage(0, 8) < 0.5,
            "边缘应变淡：{}",
            selection.coverage(0, 8)
        );
        assert_eq!(selection.coverage(8, 8), 1.0, "中心仍满覆盖");
        // 面积按覆盖率折算，所以羽化后比原来小。
        assert!(selection.area() < 16 * 16);
        assert!(selection.area() > 16 * 16 / 2, "不该掉太多");
        // 选区还在（boudns/轮廓没被抹掉）。
        assert_eq!(selection.bounds(), Some(PixelRegion::new(0, 0, 16, 16)));
        assert!(!selection.outline().is_empty());
    }

    #[test]
    fn feathering_with_a_zero_radius_is_a_no_op() {
        let selection = Selection::rect(PixelRegion::new(2, 2, 4, 4), (10, 10));
        assert_eq!(selection.feathered(0.0), selection);
        assert_eq!(selection.feathered(-3.0), selection);
    }

    #[test]
    fn a_rect_selection_matches_the_pixel_region() {
        let region = PixelRegion::new(2, 3, 4, 5);
        let selection = Selection::rect(region, (10, 10));
        for y in 0..10 {
            for x in 0..10 {
                assert_eq!(
                    selection.contains(x, y),
                    region.contains(x, y),
                    "({x}, {y})"
                );
            }
        }
        assert_eq!(selection.area(), region.area());
        assert_eq!(selection.bounds(), Some(region));
    }

    #[test]
    fn a_rect_outline_is_the_four_sides() {
        let selection = Selection::rect(PixelRegion::new(2, 3, 4, 5), (10, 10));
        assert_eq!(
            selection.outline(),
            &[
                (Vec2::new(2.0, 3.0), Vec2::new(6.0, 3.0)),
                (Vec2::new(6.0, 3.0), Vec2::new(6.0, 8.0)),
                (Vec2::new(6.0, 8.0), Vec2::new(2.0, 8.0)),
                (Vec2::new(2.0, 8.0), Vec2::new(2.0, 3.0)),
            ]
        );
    }

    /// 三角形套索：斜边以内选中，以外不选。
    #[test]
    fn a_triangle_polygon_fills_only_its_interior() {
        let points = [
            Vec2::new(2.0, 2.0),
            Vec2::new(8.0, 2.0),
            Vec2::new(2.0, 8.0),
        ];
        let selection = Selection::polygon(&points, (10, 10));
        assert!(selection.contains(3, 3), "靠近直角的内部");
        assert!(!selection.contains(7, 7), "斜边以外");
        assert!(!selection.contains(0, 0), "多边形以外");
        assert_eq!(selection.bounds(), Some(PixelRegion::new(2, 2, 5, 5)));
    }

    /// 凹多边形（C 形）：凹口里的像素不该被选中。
    #[test]
    fn a_concave_polygon_excludes_its_notch() {
        let points = [
            Vec2::new(2.0, 2.0),
            Vec2::new(8.0, 2.0),
            Vec2::new(8.0, 4.0),
            Vec2::new(4.0, 4.0),
            Vec2::new(4.0, 6.0),
            Vec2::new(8.0, 6.0),
            Vec2::new(8.0, 8.0),
            Vec2::new(2.0, 8.0),
        ];
        let selection = Selection::polygon(&points, (10, 10));
        assert!(selection.contains(3, 3), "C 的上臂");
        assert!(selection.contains(3, 7), "C 的下臂");
        assert!(selection.contains(6, 3), "C 的右侧");
        assert!(!selection.contains(6, 5), "凹口里不该选中");
    }

    #[test]
    fn a_degenerate_polygon_selects_nothing() {
        let selection = Selection::polygon(&[Vec2::new(1.0, 1.0), Vec2::new(2.0, 2.0)], (8, 8));
        assert!(selection.is_empty());
        assert_eq!(selection.bounds(), None);
    }

    /// 掩码构造：包含关系照搬掩码，轮廓是「选中像素朝向未选中邻居」的边。
    #[test]
    fn a_mask_selection_traces_its_boundary() {
        // 3×3 里只有中心一个像素被选中。
        let mut mask = vec![0u8; 9];
        mask[4] = 255;
        let selection = Selection::from_mask((3, 3), mask);
        assert!(selection.contains(1, 1));
        assert!(!selection.contains(0, 0));
        assert_eq!(selection.area(), 1);
        assert_eq!(selection.bounds(), Some(PixelRegion::new(1, 1, 1, 1)));
        assert_eq!(
            selection.outline(),
            &[
                (Vec2::new(1.0, 1.0), Vec2::new(2.0, 1.0)),
                (Vec2::new(1.0, 2.0), Vec2::new(2.0, 2.0)),
                (Vec2::new(1.0, 1.0), Vec2::new(1.0, 2.0)),
                (Vec2::new(2.0, 1.0), Vec2::new(2.0, 2.0)),
            ]
        );
    }
}
