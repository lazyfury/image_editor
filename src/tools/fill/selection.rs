//! 把选区（矩形 / 自由折线 / 钢笔路径）填成纯色，按当前选区裁剪，一条 undo。

use crate::canvas::Selection;
use crate::document::{Color, PaintCommand, Point};
use crate::tools::tool::ToolContext;

/// 把 `region`（∩ `clip`，若有）内的像素全部填成 `color`，提交一条 undo。
///
/// 矩形填充 / 自由填充共用：`region` 是矩形或自由折线围出的 [`Selection`]。
pub fn fill_selection(
    ctx: &mut ToolContext,
    region: &Selection,
    clip: Option<&Selection>,
    color: Color,
) -> bool {
    let Some(layer_id) = ctx.document.active_layer().map(|layer| layer.id) else {
        return false;
    };
    let (dx, dy) = ctx.document.ensure_layer_covers_document(layer_id);
    ctx.history.translate_layer(layer_id, dx, dy);
    let Some(before) = ctx
        .document
        .layer(layer_id)
        .map(|layer| layer.pixels.clone())
    else {
        return false;
    };
    let (width, height) = (ctx.document.width, ctx.document.height);
    let offset = ctx
        .document
        .layer(layer_id)
        .map(|layer| layer.position)
        .unwrap_or(Point::new(0, 0));
    let changed = {
        let Some(layer) = ctx.document.layer_mut(layer_id) else {
            return false;
        };
        let mut changed = false;
        for y in 0..height {
            for x in 0..width {
                // 羽化过的选区按覆盖率混合，硬选区里就是 0 / 1。
                let mut alpha = region.coverage(x, y);
                if let Some(clip) = clip {
                    alpha *= clip.coverage(x, y);
                }
                if alpha <= 0.0 {
                    continue;
                }
                let bx = x as i64 - offset.x as i64;
                let by = y as i64 - offset.y as i64;
                if bx < 0
                    || by < 0
                    || bx >= layer.pixels.width as i64
                    || by >= layer.pixels.height as i64
                {
                    continue;
                }
                let (bx, by) = (bx as u32, by as u32);
                let before = layer.pixels.get_pixel(bx, by);
                if alpha >= 1.0 {
                    if before != color {
                        layer.pixels.set_pixel(bx, by, color);
                        changed = true;
                    }
                } else {
                    layer.pixels.blend_from(bx, by, before, color, alpha);
                    if layer.pixels.get_pixel(bx, by) != before {
                        changed = true;
                    }
                }
            }
        }
        changed
    };
    if !changed {
        return false;
    }
    let command = ctx
        .document
        .layer(layer_id)
        .and_then(|layer| PaintCommand::capture(layer_id, &before, &layer.pixels, "填充"));
    match command {
        Some(command) => {
            ctx.history.execute(Box::new(command), ctx.document);
            true
        }
        None => false,
    }
}
