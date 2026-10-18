//! 任意形状的选区：与文档同尺寸的像素掩码 + 描边轮廓。
//!
//! 框选是矩形、套索是自由折线，统一成一张 `Vec<bool>` 掩码：`contains` 是
//! O(1)，画笔 / 填充裁剪直接查它，以后加魔棒 / 加选减选也只改这一处。
//! 另外保留一条文档坐标的 `outline` 折线给「马赛克线段」描边用（矩形 4 点 /
//! 套索原折线），所以边框始终跟着用户画出来的路径。

use igui_core::Vec2;

use crate::document::PixelRegion;

/// 与文档同尺寸的像素掩码 + 描边轮廓。
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    width: u32,
    height: u32,
    mask: Vec<bool>,
    outline: Vec<Vec2>,
}

impl Selection {
    /// 矩形选区（框选）。区域会裁剪到文档内。
    pub fn rect(region: PixelRegion, document: (u32, u32)) -> Self {
        let (width, height) = document;
        let mut mask = vec![false; width as usize * height as usize];
        let x0 = region.x.min(width);
        let y0 = region.y.min(height);
        let x1 = region.right().min(width);
        let y1 = region.bottom().min(height);
        for y in y0..y1 {
            for x in x0..x1 {
                mask[y as usize * width as usize + x as usize] = true;
            }
        }
        let outline = vec![
            Vec2::new(x0 as f32, y0 as f32),
            Vec2::new(x1 as f32, y0 as f32),
            Vec2::new(x1 as f32, y1 as f32),
            Vec2::new(x0 as f32, y1 as f32),
        ];
        Self {
            width,
            height,
            mask,
            outline,
        }
    }

    /// 自由折线围成的选区（套索）。偶奇规则扫描线光栅化，取像素中心。
    pub fn polygon(points: &[Vec2], document: (u32, u32)) -> Self {
        let (width, height) = document;
        let mut mask = vec![false; width as usize * height as usize];
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
                        mask[y as usize * width as usize + x as usize] = true;
                    }
                }
            }
        }
        Self {
            width,
            height,
            mask,
            outline: points.to_vec(),
        }
    }

    /// 像素是否被选中（O(1)）。
    pub fn contains(&self, x: u32, y: u32) -> bool {
        x < self.width
            && y < self.height
            && self.mask[y as usize * self.width as usize + x as usize]
    }

    /// 选中的像素个数。
    pub fn area(&self) -> usize {
        self.mask.iter().filter(|selected| **selected).count()
    }

    /// 有没有选中任何像素（套索画出退化的形状时用它判断）。
    pub fn is_empty(&self) -> bool {
        self.area() == 0
    }

    /// 选中像素的包围盒；没有选中任何像素返回 `None`。
    pub fn bounds(&self) -> Option<PixelRegion> {
        let (mut min_x, mut min_y) = (u32::MAX, u32::MAX);
        let (mut max_x, mut max_y) = (0u32, 0u32);
        for y in 0..self.height {
            for x in 0..self.width {
                if !self.contains(x, y) {
                    continue;
                }
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        (min_x != u32::MAX)
            .then(|| PixelRegion::new(min_x, min_y, max_x - min_x + 1, max_y - min_y + 1))
    }

    /// 描边用的轮廓（文档坐标）：矩形 4 点、套索原折线。
    pub fn outline(&self) -> &[Vec2] {
        &self.outline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_rect_outline_is_four_corners() {
        let selection = Selection::rect(PixelRegion::new(2, 3, 4, 5), (10, 10));
        assert_eq!(
            selection.outline(),
            &[
                Vec2::new(2.0, 3.0),
                Vec2::new(6.0, 3.0),
                Vec2::new(6.0, 8.0),
                Vec2::new(2.0, 8.0),
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
}
