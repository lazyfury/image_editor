//! 油漆桶工具：把洪水填充包成 [`Tool`]，一次点击一条 undo。

use std::rc::Rc;

use super::flood::flood_fill;
use crate::canvas::{document_to_pixel, Selection};
use crate::document::{Color, PaintCommand, Point};
use crate::tools::tool::{PointerEvent, Tool, ToolContext};

/// 油漆桶工具。颜色 / 容差 / 连续 / 选区由视图在落笔前同步。
#[derive(Debug, Clone, PartialEq)]
pub struct PaintBucketTool {
    pub color: Color,
    /// 0..=255，每个 RGBA 通道允许的最大差。
    pub tolerance: u8,
    /// `true` = 只填连通的同色区域；`false` = 填全图所有匹配像素。
    pub contiguous: bool,
    pub clip: Option<Rc<Selection>>,
}

impl PaintBucketTool {
    /// 容差步进。
    pub const TOLERANCE_STEP: u8 = 8;

    pub fn new() -> Self {
        Self {
            color: Color::BLACK,
            tolerance: 0,
            contiguous: true,
            clip: None,
        }
    }
}

impl Default for PaintBucketTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for PaintBucketTool {
    fn name(&self) -> &'static str {
        "油漆桶"
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolContext, event: PointerEvent) {
        let document_size = (ctx.document.width, ctx.document.height);
        let Some(seed) = document_to_pixel(event.position, document_size.0, document_size.1) else {
            return;
        };
        let Some(layer_id) = ctx.document.active_layer().map(|layer| layer.id) else {
            return;
        };
        // 缓冲区扩到覆盖整张文档，落点在文档内就一定能填到（跟画笔一致）。
        let (dx, dy) = ctx.document.ensure_layer_covers_document(layer_id);
        ctx.history.translate_layer(layer_id, dx, dy);
        let Some(before) = ctx
            .document
            .layer(layer_id)
            .map(|layer| layer.pixels.clone())
        else {
            return;
        };
        let offset = ctx
            .document
            .layer(layer_id)
            .map(|layer| layer.position)
            .unwrap_or(Point::new(0, 0));
        let changed = match ctx.document.layer_mut(layer_id) {
            Some(layer) => flood_fill(
                &mut layer.pixels,
                offset,
                document_size,
                seed,
                self.color,
                self.tolerance,
                self.contiguous,
                self.clip.as_deref(),
            ),
            None => false,
        };
        if !changed {
            return;
        }
        let command = ctx
            .document
            .layer(layer_id)
            .and_then(|layer| PaintCommand::capture(layer_id, &before, &layer.pixels, self.name()));
        if let Some(command) = command {
            ctx.history.execute(Box::new(command), ctx.document);
        }
    }

    fn on_pointer_move(&mut self, _ctx: &mut ToolContext, _event: PointerEvent) {}
    fn on_pointer_up(&mut self, _ctx: &mut ToolContext, _event: PointerEvent) {}
}
