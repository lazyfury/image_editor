//! 像素剪贴板：从图层上裁一块像素，再贴回去。
//!
//! 只管「一块 RGBA + 它在文档里的位置」和三次纯像素操作（抓 / 贴 / 清空），
//! 不认识 UI 和撤销栈 —— 视图负责快照 + 提交一条 [`PaintCommand`]。
//!
//! 剪贴板的 alpha 在抓取时**已经乘过选区覆盖率**，所以羽化过的选区复制出来边缘
//! 也是软的，贴回去不会突然变硬。粘贴按「原位置」放回，再乘目标选区的覆盖率
//! （没有选区就是不裁）。
//!
//! [`PaintCommand`]: crate::document::PaintCommand

use crate::canvas::Selection;
use crate::document::{Color, Layer, PixelRegion};

/// 一次「复制 / 剪切」抓下来的像素（跨工作区共享，进程内有效）。
#[derive(Debug, Clone, PartialEq)]
pub struct PixelClipboard {
    /// 抓取区域在**文档坐标**里的位置（粘贴时放回原处）。
    pub origin: PixelRegion,
    /// RGBA 像素，行优先；alpha 已乘过选区覆盖率。
    pixels: Vec<Color>,
}

impl PixelClipboard {
    /// 从图层上按选区裁一块。
    ///
    /// 图层缓冲区没覆盖到的文档像素当透明（画布外的内容本来就不算选中）。
    pub fn capture(layer: &Layer, selection: &Selection) -> Option<Self> {
        let origin = selection.bounds()?;
        if origin.area() == 0 {
            return None;
        }
        let mut pixels = Vec::with_capacity(origin.area());
        for y in origin.y..origin.bottom() {
            for x in origin.x..origin.right() {
                let coverage = selection.coverage(x, y);
                let bx = x as i64 - layer.position.x as i64;
                let by = y as i64 - layer.position.y as i64;
                let mut color = if bx < 0
                    || by < 0
                    || bx >= layer.pixels.width as i64
                    || by >= layer.pixels.height as i64
                {
                    Color::TRANSPARENT
                } else {
                    layer.pixels.get_pixel(bx as u32, by as u32)
                };
                color.a = (color.a as f32 * coverage).round() as u8;
                pixels.push(color);
            }
        }
        Some(Self { origin, pixels })
    }

    /// 剪贴板的尺寸（像素）。
    pub fn size(&self) -> (u32, u32) {
        (self.origin.width, self.origin.height)
    }

    pub fn is_empty(&self) -> bool {
        self.pixels.is_empty()
    }

    /// 贴回图层：位置 = 抓取时的原点，按 `clip`（当前选区）的覆盖率裁剪。
    ///
    /// 返回是否有像素真的变了（全落在选区外 / 完全透明时是 `false`）。
    pub fn paste(&self, layer: &mut Layer, clip: Option<&Selection>) -> bool {
        let mut changed = false;
        for (index, color) in self.pixels.iter().enumerate() {
            if color.a == 0 {
                continue;
            }
            let x = self.origin.x + (index as u32 % self.origin.width);
            let y = self.origin.y + (index as u32 / self.origin.width);
            let coverage = clip.map_or(1.0, |clip| clip.coverage(x, y));
            let alpha = color.a as f32 / 255.0 * coverage;
            if alpha <= 0.0 {
                continue;
            }
            let bx = x as i64 - layer.position.x as i64;
            let by = y as i64 - layer.position.y as i64;
            if bx < 0
                || by < 0
                || bx >= layer.pixels.width as i64
                || by >= layer.pixels.height as i64
            {
                continue;
            }
            let (bx, by) = (bx as u32, by as u32);
            let base = layer.pixels.get_pixel(bx, by);
            layer.pixels.blend_from(bx, by, base, *color, alpha);
            if layer.pixels.get_pixel(bx, by) != base {
                changed = true;
            }
        }
        changed
    }

    /// 清空选区里的像素（Delete / 剪切）：按覆盖率把 alpha 往 0 拉。
    pub fn clear_selection(layer: &mut Layer, selection: &Selection) -> bool {
        let Some(bounds) = selection.bounds() else {
            return false;
        };
        let mut changed = false;
        for y in bounds.y..bounds.bottom() {
            for x in bounds.x..bounds.right() {
                let coverage = selection.coverage(x, y);
                if coverage <= 0.0 {
                    continue;
                }
                let bx = x as i64 - layer.position.x as i64;
                let by = y as i64 - layer.position.y as i64;
                if bx < 0
                    || by < 0
                    || bx >= layer.pixels.width as i64
                    || by >= layer.pixels.height as i64
                {
                    continue;
                }
                let (bx, by) = (bx as u32, by as u32);
                let base = layer.pixels.get_pixel(bx, by);
                if base.a == 0 {
                    continue;
                }
                layer.pixels.erase_from(bx, by, base, coverage);
                if layer.pixels.get_pixel(bx, by) != base {
                    changed = true;
                }
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, PixelBuffer};

    fn layer_with(paint: &[(u32, u32)]) -> Layer {
        let mut document = Document::with_background("d", 16, 16, Color::TRANSPARENT);
        let id = document.active_layer().unwrap().id;
        for (x, y) in paint {
            document
                .layer_mut(id)
                .unwrap()
                .pixels
                .set_pixel(*x, *y, Color::RED);
        }
        document.layer(id).unwrap().clone()
    }

    fn at(layer: &Layer, x: u32, y: u32) -> Color {
        layer.pixels.get_pixel(x, y)
    }

    #[test]
    fn capturing_crops_to_the_selection_bounds() {
        let layer = layer_with(&[(2, 3), (5, 6)]);
        let selection = Selection::rect(PixelRegion::new(2, 3, 4, 4), (16, 16));
        let clipboard = PixelClipboard::capture(&layer, &selection).expect("应有内容");
        assert_eq!(clipboard.origin, PixelRegion::new(2, 3, 4, 4));
        assert_eq!(clipboard.size(), (4, 4));
        assert_eq!(clipboard.pixels.len(), 16, "整块矩形都存下来");
        // 选区外的像素不会被抓进来。
        let selection = Selection::rect(PixelRegion::new(0, 0, 1, 1), (16, 16));
        let single = PixelClipboard::capture(&layer, &selection).expect("一块");
        assert_eq!(single.pixels, vec![Color::TRANSPARENT]);
    }

    #[test]
    fn a_feathered_selection_is_captured_with_soft_alpha() {
        let mut layer = layer_with(&[]);
        layer.pixels = PixelBuffer::filled(32, 32, Color::RED);
        let selection = Selection::all((32, 32)).feathered(2.0);
        let clipboard = PixelClipboard::capture(&layer, &selection).expect("应有内容");
        assert_eq!(clipboard.origin.area(), 32 * 32);
        // 中心满 alpha、边缘被羽化掉一部分。
        assert_eq!(clipboard.pixels[32 * 16 + 16].a, 255, "中心仍是满覆盖");
        assert!(
            clipboard.pixels[32 * 16].a < 200,
            "左边缘应变淡：{}",
            clipboard.pixels[32 * 16].a
        );
    }

    #[test]
    fn pasting_puts_the_pixels_back_at_the_original_origin() {
        let mut layer = layer_with(&[(2, 3), (3, 3)]);
        let selection = Selection::rect(PixelRegion::new(2, 3, 2, 1), (16, 16));
        let clipboard = PixelClipboard::capture(&layer, &selection).expect("复制");
        // 剪掉再贴回去：像素应该原地复原。
        assert!(PixelClipboard::clear_selection(&mut layer, &selection));
        assert_eq!(at(&layer, 2, 3).a, 0, "清空只把 alpha 拉成 0");
        assert!(clipboard.paste(&mut layer, None));
        assert_eq!(at(&layer, 2, 3), Color::RED);
        assert_eq!(at(&layer, 3, 3), Color::RED);
        assert_eq!(at(&layer, 4, 3), Color::TRANSPARENT, "不会糊到旁边");
    }

    #[test]
    fn pasting_respects_the_current_selection() {
        let mut layer = layer_with(&[(2, 3)]);
        let source = Selection::rect(PixelRegion::new(2, 3, 1, 1), (16, 16));
        let clipboard = PixelClipboard::capture(&layer, &source).expect("复制");
        // 目标选区在别处：粘贴被裁掉，什么都不变。
        let elsewhere = Selection::rect(PixelRegion::new(10, 10, 2, 2), (16, 16));
        assert!(!clipboard.paste(&mut layer, Some(&elsewhere)));
        assert_eq!(at(&layer, 2, 3), Color::RED, "原像素还在");
        // 目标选区覆盖原点、目标像素本来就是空的：贴成功。
        let over = Selection::rect(PixelRegion::new(0, 0, 8, 8), (16, 16));
        let mut layer = layer_with(&[]);
        assert!(clipboard.paste(&mut layer, Some(&over)));
        assert_eq!(at(&layer, 2, 3), Color::RED);
    }

    #[test]
    fn clearing_only_touches_the_selection() {
        let mut layer = layer_with(&[(2, 3), (9, 9)]);
        let selection = Selection::rect(PixelRegion::new(0, 0, 5, 5), (16, 16));
        assert!(PixelClipboard::clear_selection(&mut layer, &selection));
        assert_eq!(at(&layer, 2, 3).a, 0, "选区内被清掉");
        assert_eq!(at(&layer, 9, 9), Color::RED, "选区外不动");
        // 再清一次已经没有变化。
        assert!(!PixelClipboard::clear_selection(&mut layer, &selection));
    }
}
