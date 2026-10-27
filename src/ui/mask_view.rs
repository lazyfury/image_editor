//! 编辑蒙版时的画布提示：在合成结果上把「被遮住的像素」染成红色。
//!
//! 这是**显示层**的叠加，不进合成器、不进导出：`take_texture_upload` 在合成 /
//! 棋盘格之后调用，红色只出现在屏幕上的显示纹理里。覆盖率越低越红（黑 = 全遮、
//! 白 = 不遮），所以画笔涂黑时能立刻看见「藏起来了」。
//!
//! 只在 `Document.active_mask` 时生效；区域更新（画笔一笔）也只处理脏矩形，
//! 和合成器的局部更新同一套坐标。

use crate::document::{blend_over, Document, PixelBuffer, PixelRegion};

/// 被遮住区域的红色强度上限（0..=1）。半透明，底下的图像还看得见。
const TINT: f32 = 0.45;
/// 叠加的红色。
const RED: [u8; 4] = [220, 40, 40, 255];

/// 整图叠加（`take_texture_upload` 的整图路径）。
pub(super) fn paint_mask_overlay(document: &Document, pixels: &mut PixelBuffer) {
    let region = PixelRegion::new(0, 0, document.width, document.height);
    paint_mask_overlay_region(document, pixels, region);
}

/// 只在 `region`（文档坐标）内叠加红色。
pub(super) fn paint_mask_overlay_region(
    document: &Document,
    pixels: &mut PixelBuffer,
    region: PixelRegion,
) {
    if !document.active_mask {
        return;
    }
    let Some(layer) = document.active_layer() else {
        return;
    };
    let Some(mask) = layer.mask.as_ref() else {
        return;
    };
    let region = region.clamped(document.width, document.height);
    if region.area() == 0 {
        return;
    }
    // 蒙版只在所属图层缓冲的范围内有意义（组没有可编辑蒙版，早退）。
    let dx = layer.position.x as i64;
    let dy = layer.position.y as i64;
    let x_start = (dx).max(region.x as i64);
    let x_end = (dx + mask.pixels.width as i64).min(region.right() as i64);
    let y_start = (dy).max(region.y as i64);
    let y_end = (dy + mask.pixels.height as i64).min(region.bottom() as i64);
    if x_start >= x_end || y_start >= y_end {
        return;
    }
    for y in y_start..y_end {
        for x in x_start..x_end {
            let mx = (x - dx) as u32;
            let my = (y - dy) as u32;
            let coverage = mask.pixels.get_pixel(mx, my).r as f32 / 255.0;
            let alpha = (1.0 - coverage) * TINT;
            if alpha <= 0.0 {
                continue;
            }
            let index = (y as usize * pixels.width as usize + x as usize) * 4;
            if let Some(dst) = pixels.data.get_mut(index..index + 4) {
                blend_over(dst, &RED, alpha);
            }
        }
    }
}
