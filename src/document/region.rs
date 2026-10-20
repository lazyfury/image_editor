//! 整数像素矩形：差异区域（撤销命令 / 以后的脏矩形）的单位。
//!
//! 坐标是文档 / 图层像素；`width` 或 `height` 为 0 就是空区域。越界不做假设，
//! 由使用方按 `PixelBuffer` 的尺寸裁剪。

use super::pixel_buffer::PixelBuffer;

/// 一个轴对齐的像素矩形，`x` / `y` 是左上角，宽高是**像素个数**。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PixelRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRegion {
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// 右边界（不含）。
    pub const fn right(self) -> u32 {
        self.x + self.width
    }

    /// 下边界（不含）。
    pub const fn bottom(self) -> u32 {
        self.y + self.height
    }

    pub const fn area(self) -> usize {
        self.width as usize * self.height as usize
    }

    /// 像素是否落在区域内（框选裁剪切像素用）。
    ///
    /// 运行时裁剪现在走 [`Selection::contains`](crate::canvas::Selection::contains)
    /// （支持任意形状），这里保留为矩形几何谓词 / 单测用。
    #[allow(dead_code)]
    pub const fn contains(self, x: u32, y: u32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }

    /// 整体平移 `(dx, dy)`（非负：只会在左上补空间时用到）。
    pub const fn translated(self, dx: u32, dy: u32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            width: self.width,
            height: self.height,
        }
    }

    /// 并集包围盒（空区域不影响另一侧）。
    ///
    /// 画笔按「每个 dab 的脏矩形」累积时用它，所以一次笔触的脏区始终是一个
    /// 矩形，局部合成 / 撤销区域都只依赖它。
    pub fn union(self, other: Self) -> Self {
        if self.width == 0 || self.height == 0 {
            return other;
        }
        if other.width == 0 || other.height == 0 {
            return self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Self::new(x, y, right - x, bottom - y)
    }

    /// 裁到 `width × height` 内；完全在外面就是空区域（`width` 或 `height` 为 0）。
    pub fn clamped(self, width: u32, height: u32) -> Self {
        let x = self.x.min(width);
        let y = self.y.min(height);
        let right = self.right().min(width);
        let bottom = self.bottom().min(height);
        Self::new(x, y, right.saturating_sub(x), bottom.saturating_sub(y))
    }

    /// 文档坐标的矩形 -> 图层缓冲区坐标（图层原点可能非零 / 为负），越界裁掉。
    ///
    /// 画笔提交撤销区域、剪贴板 / 删除这类「一步 undo」的像素操作都用它。
    pub fn in_buffer(self, offset: (i32, i32), width: u32, height: u32) -> Self {
        let x = self.x as i64 - offset.0 as i64;
        let y = self.y as i64 - offset.1 as i64;
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + self.width as i64).min(width as i64);
        let y1 = (y + self.height as i64).min(height as i64);
        if x1 <= x0 || y1 <= y0 {
            return Self::new(0, 0, 0, 0);
        }
        Self::new(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32)
    }

    /// 两张**同尺寸**缓冲之间的差异包围盒；完全相同或尺寸不同返回 `None`。
    ///
    /// 逐 4 字节比较原始数据，只有真正不同的像素才换算坐标，所以一笔的
    /// 撤销区域跟笔画大小成正比，跟画布大小无关（比较仍是整块扫描）。
    pub fn diff(before: &PixelBuffer, after: &PixelBuffer) -> Option<Self> {
        if before.width != after.width || before.height != after.height || before.width == 0 {
            return None;
        }
        let (mut min_x, mut min_y) = (u32::MAX, u32::MAX);
        let (mut max_x, mut max_y) = (0u32, 0u32);
        let mut changed = false;
        for (index, (a, b)) in before
            .data
            .chunks_exact(4)
            .zip(after.data.chunks_exact(4))
            .enumerate()
        {
            if a == b {
                continue;
            }
            let index = index as u32;
            let (x, y) = (index % before.width, index / before.width);
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            changed = true;
        }
        changed.then(|| Self::new(min_x, min_y, max_x - min_x + 1, max_y - min_y + 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Color;

    #[test]
    fn a_region_reports_its_area_and_edges() {
        assert_eq!(PixelRegion::new(2, 3, 4, 5).area(), 20);
        assert_eq!(PixelRegion::new(2, 3, 4, 5).right(), 6);
        assert_eq!(PixelRegion::new(2, 3, 4, 5).bottom(), 8);
    }

    #[test]
    fn contains_is_half_open_on_the_right_and_bottom() {
        let region = PixelRegion::new(2, 3, 4, 5);
        assert!(region.contains(2, 3));
        assert!(region.contains(5, 7));
        assert!(!region.contains(6, 3), "右边界不含");
        assert!(!region.contains(2, 8), "下边界不含");
        assert!(!region.contains(1, 3));
    }

    #[test]
    fn union_covers_both_rectangles() {
        let a = PixelRegion::new(2, 3, 2, 2);
        let b = PixelRegion::new(5, 6, 3, 1);
        assert_eq!(a.union(b), PixelRegion::new(2, 3, 6, 4));
        // 空区域不影响另一侧。
        let empty = PixelRegion::new(9, 9, 0, 0);
        assert_eq!(a.union(empty), a);
        assert_eq!(empty.union(a), a);
    }

    #[test]
    fn clamping_drops_what_falls_outside_the_document() {
        let region = PixelRegion::new(6, 6, 8, 8);
        assert_eq!(region.clamped(10, 10), PixelRegion::new(6, 6, 4, 4));
        // 完全在外面 -> 空区域。
        let outside = PixelRegion::new(20, 20, 4, 4).clamped(10, 10);
        assert_eq!(outside.area(), 0);
    }

    #[test]
    fn converting_to_buffer_coordinates_crops_to_the_layer() {
        // 文档矩形 (6,6,8,8)、图层原点 (4,4)、缓冲 10×10：右边到缓冲边界为止。
        let region = PixelRegion::new(6, 6, 8, 8);
        assert_eq!(
            region.in_buffer((4, 4), 10, 10),
            PixelRegion::new(2, 2, 8, 8)
        );
        // 缓冲更小：裁到缓冲区里。
        assert_eq!(region.in_buffer((4, 4), 8, 8), PixelRegion::new(2, 2, 6, 6));
        // 原点为负（画布外有内容）：整体右移。
        assert_eq!(
            region.in_buffer((-2, -2), 32, 32),
            PixelRegion::new(8, 8, 8, 8)
        );
        // 完全在缓冲外 -> 空区域。
        assert_eq!(
            PixelRegion::new(50, 50, 4, 4)
                .in_buffer((0, 0), 32, 32)
                .area(),
            0
        );
    }

    #[test]
    fn diff_is_the_bounding_box_of_changed_pixels() {
        let before = PixelBuffer::filled(8, 8, Color::WHITE);
        let mut after = before.clone();
        after.set_pixel(2, 3, Color::BLACK);
        after.set_pixel(5, 6, Color::BLACK);
        // 包围盒覆盖两个点：(2,3)..=(5,6)。
        assert_eq!(
            PixelRegion::diff(&before, &after),
            Some(PixelRegion::new(2, 3, 4, 4))
        );
    }

    #[test]
    fn diff_of_identical_buffers_is_none() {
        let buffer = PixelBuffer::filled(4, 4, Color::WHITE);
        assert_eq!(PixelRegion::diff(&buffer, &buffer.clone()), None);
    }

    #[test]
    fn diff_of_mismatched_sizes_is_none() {
        let a = PixelBuffer::new(4, 4);
        let mut b = PixelBuffer::new(5, 4);
        b.set_pixel(0, 0, Color::RED);
        assert_eq!(PixelRegion::diff(&a, &b), None);
    }
}
