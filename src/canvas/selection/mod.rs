//! 任意形状的选区：与文档同尺寸的**覆盖率**掩码 + 描边轮廓。
//!
//! 框选是矩形、套索是自由折线、魔棒是容差连通区域，统一成一张 `Vec<u8>`
//! 掩码（0 = 没选中、255 = 完全选中）：`contains` 是 O(1)，画笔 / 填充按
//! [`Selection::coverage`] 衰减，所以羽化过的选区也能用（边缘渐变）。
//! 轮廓是一条条**线段**（文档坐标）—— 矩形 4 段、套索 N 段、魔棒是掩码的
//! 边界边（按段合并），给「马赛克线段」描边用，所以边框始终贴着真实选区。

use igui_core::Vec2;

use crate::document::PixelRegion;

mod feather;
mod merge;
mod outline;

#[cfg(test)]
mod tests;

pub use merge::MergeMode;

use outline::{closed_edges, mask_edges};

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

    /// 描边用的轮廓线段（文档坐标）。
    pub fn outline(&self) -> &[Edge] {
        &self.outline
    }
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
