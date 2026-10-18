//! 矩形填充：拖一个矩形，松手用前景色填充（按选区裁剪）。
//!
//! 拖动中把 `anchor` / `current`（文档坐标）暴露给视图画预览；松手时把矩形
//! 光栅成 [`Selection`]，复用 [`fill_selection`] 提交一条 undo。

use std::rc::Rc;

use igui_core::Vec2;

use super::fill::fill_selection;
use super::tool::{PointerEvent, Tool, ToolContext};
use crate::canvas::{pixel_selection, Selection};
use crate::document::Color;

/// 矩形填充工具。
#[derive(Debug, Clone, PartialEq)]
pub struct RectangleFillTool {
    pub color: Color,
    pub clip: Option<Rc<Selection>>,
    anchor: Option<Vec2>,
    current: Option<Vec2>,
}

impl RectangleFillTool {
    pub fn new() -> Self {
        Self {
            color: Color::BLACK,
            clip: None,
            anchor: None,
            current: None,
        }
    }

    /// 是否正处在一次拖拽中。
    pub fn is_filling(&self) -> bool {
        self.anchor.is_some()
    }

    /// 拖动中的两个角点（文档坐标），给视图画预览。
    pub fn preview(&self) -> Option<(Vec2, Vec2)> {
        Some((self.anchor?, self.current?))
    }
}

impl Default for RectangleFillTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for RectangleFillTool {
    fn name(&self) -> &'static str {
        "矩形填充"
    }

    fn on_pointer_down(&mut self, _ctx: &mut ToolContext, event: PointerEvent) {
        self.anchor = Some(event.position);
        self.current = Some(event.position);
    }

    fn on_pointer_move(&mut self, _ctx: &mut ToolContext, event: PointerEvent) {
        if self.anchor.is_some() {
            self.current = Some(event.position);
        }
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolContext, event: PointerEvent) {
        let Some(anchor) = self.anchor.take() else {
            return;
        };
        self.current = None;
        let (width, height) = (ctx.document.width, ctx.document.height);
        let Some(region) = pixel_selection(anchor, event.position, width, height) else {
            return;
        };
        let selection = Selection::rect(region, (width, height));
        fill_selection(ctx, &selection, self.clip.as_deref(), self.color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, History, PixelRegion};

    fn at(document: &Document, x: u32, y: u32) -> Color {
        document.active_layer().unwrap().pixels.get_pixel(x, y)
    }

    fn drag(tool: &mut RectangleFillTool, document: &mut Document, from: Vec2, to: Vec2) {
        let mut history = History::new();
        let mut ctx = ToolContext {
            document,
            history: &mut history,
        };
        tool.on_pointer_down(&mut ctx, pointer(from));
        tool.on_pointer_move(&mut ctx, pointer(to));
        tool.on_pointer_up(&mut ctx, pointer(to));
        assert_eq!(history.undo_len(), 1, "一次填充 = 一步 undo");
    }

    fn pointer(position: Vec2) -> PointerEvent {
        PointerEvent {
            position,
            button: igui_core::PointerButton::Left,
        }
    }

    #[test]
    fn a_drag_fills_only_the_dragged_rectangle() {
        let mut document = Document::new("d", 16, 16);
        let mut tool = RectangleFillTool::new();
        tool.color = Color::RED;
        drag(
            &mut tool,
            &mut document,
            Vec2::new(2.0, 2.0),
            Vec2::new(6.0, 6.0),
        );
        assert_eq!(at(&document, 3, 3), Color::RED, "矩形内被填");
        assert_eq!(at(&document, 1, 1), Color::WHITE, "矩形外不填");
        assert_eq!(at(&document, 6, 6), Color::WHITE, "右下边界不含");
    }

    #[test]
    fn a_rectangle_fill_respects_the_selection() {
        let mut document = Document::new("d", 16, 16);
        let mut tool = RectangleFillTool::new();
        tool.color = Color::RED;
        tool.clip = Some(Rc::new(Selection::rect(
            PixelRegion::new(0, 0, 4, 4),
            (16, 16),
        )));
        drag(
            &mut tool,
            &mut document,
            Vec2::new(0.0, 0.0),
            Vec2::new(8.0, 8.0),
        );
        assert_eq!(at(&document, 3, 3), Color::RED, "选区内被填");
        assert_eq!(at(&document, 5, 5), Color::WHITE, "选区外不填");
    }

    #[test]
    fn a_click_without_dragging_fills_nothing() {
        let mut document = Document::new("d", 16, 16);
        let mut tool = RectangleFillTool::new();
        tool.color = Color::RED;
        let mut history = History::new();
        let mut ctx = ToolContext {
            document: &mut document,
            history: &mut history,
        };
        tool.on_pointer_down(&mut ctx, pointer(Vec2::new(3.0, 3.0)));
        tool.on_pointer_up(&mut ctx, pointer(Vec2::new(3.0, 3.0)));
        assert_eq!(history.undo_len(), 0, "零面积不产生命令");
        assert_eq!(at(&document, 3, 3), Color::WHITE);
    }
}
