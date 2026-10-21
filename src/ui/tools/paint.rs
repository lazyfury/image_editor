//! 落笔类工具的指针适配：画笔 / 橡皮 / 移动 / 油漆桶 / 矩形填充 / 渐变 / 吸管。
//!
//! 每个适配函数只做三件事：把 `AppState` 里的前景色 / 背景色 / 选区喂给工具、
//! 转发指针事件、在工具改过像素后标脏（局部合成）。真正的绘制逻辑在
//! [`crate::tools`]。

use igui_core::Vec2;

use crate::app::state::ActiveTool;
use crate::canvas::document_to_pixel;
use crate::renderer::sample_pixel;
use crate::tools::{BrushMode, Tool};
use crate::ui::EditorView;

use super::{drive, Phase};

impl EditorView {
    // -- 画笔 / 橡皮 ------------------------------------------------------

    pub(in crate::ui) fn begin_brush(&mut self, tool: ActiveTool, position: Vec2) {
        self.brush.mode = if tool == ActiveTool::Eraser {
            BrushMode::Erase
        } else {
            BrushMode::Paint
        };
        let (foreground, selection) = {
            let state = self.state.borrow();
            (state.foreground, state.selection.clone())
        };
        self.brush.color = foreground;
        self.brush.clip = selection;
        tracing::debug!(target: "image_editor", tool = self.brush.name(), "stroke_started");
        drive(&self.state, &mut self.brush, Phase::Down, position);
        // 借用结束再标脏：画笔把写过的矩形记在 `brush` 里。
        self.mark_brush_dirty();
    }

    pub(in crate::ui) fn continue_brush(&mut self, position: Vec2) {
        drive(&self.state, &mut self.brush, Phase::Move, position);
        self.mark_brush_dirty();
    }

    pub(in crate::ui) fn end_brush(&mut self, position: Vec2) {
        drive(&self.state, &mut self.brush, Phase::Up, position);
        self.mark_brush_dirty();
    }

    /// 画笔刚写过的地方：只把那一块标脏（局部合成），别整图重算。
    pub(in crate::ui) fn mark_brush_dirty(&mut self) {
        if let Some(region) = self.brush.take_dirty() {
            self.mark_region_dirty(region);
        }
    }

    // -- 移动 ------------------------------------------------------------

    pub(in crate::ui) fn begin_move(&mut self, position: Vec2) {
        drive(&self.state, &mut self.move_tool, Phase::Down, position);
        self.texture_dirty = true;
    }

    pub(in crate::ui) fn continue_move(&mut self, position: Vec2) {
        drive(&self.state, &mut self.move_tool, Phase::Move, position);
        self.texture_dirty = true;
    }

    pub(in crate::ui) fn end_move(&mut self, position: Vec2) {
        drive(&self.state, &mut self.move_tool, Phase::Up, position);
    }

    // -- 油漆桶 ----------------------------------------------------------

    pub(in crate::ui) fn begin_bucket(&mut self, position: Vec2) {
        let (foreground, selection) = {
            let state = self.state.borrow();
            (state.foreground, state.selection.clone())
        };
        self.paint_bucket.color = foreground;
        self.paint_bucket.clip = selection;
        drive(&self.state, &mut self.paint_bucket, Phase::Down, position);
        self.texture_dirty = true;
    }

    // -- 矩形填充 --------------------------------------------------------

    pub(in crate::ui) fn begin_rect_fill(&mut self, position: Vec2) {
        let (foreground, selection) = {
            let state = self.state.borrow();
            (state.foreground, state.selection.clone())
        };
        self.rect_fill.color = foreground;
        self.rect_fill.clip = selection;
        drive(&self.state, &mut self.rect_fill, Phase::Down, position);
    }

    pub(in crate::ui) fn continue_rect_fill(&mut self, position: Vec2) {
        drive(&self.state, &mut self.rect_fill, Phase::Move, position);
    }

    pub(in crate::ui) fn end_rect_fill(&mut self, position: Vec2) {
        drive(&self.state, &mut self.rect_fill, Phase::Up, position);
        self.texture_dirty = true;
        *self.message.borrow_mut() = Some("矩形填充".to_string());
    }

    // -- 渐变 ------------------------------------------------------------

    pub(in crate::ui) fn begin_gradient(&mut self, position: Vec2) {
        let (foreground, background, selection) = {
            let state = self.state.borrow();
            (state.foreground, state.background, state.selection.clone())
        };
        // 用户没动过色带时，首尾颜色跟随前景 / 背景色。
        if !self.gradient_customized.get() {
            self.gradient
                .borrow_mut()
                .set_endpoints(foreground, background);
        }
        self.gradient_tool.clip = selection;
        self.gradient_tool.kind = self.gradient_kind;
        self.gradient_tool.reverse = self.gradient_reverse.get();
        drive(&self.state, &mut self.gradient_tool, Phase::Down, position);
    }

    pub(in crate::ui) fn continue_gradient(&mut self, position: Vec2) {
        drive(&self.state, &mut self.gradient_tool, Phase::Move, position);
        self.texture_dirty = true;
    }

    pub(in crate::ui) fn end_gradient(&mut self, position: Vec2) {
        drive(&self.state, &mut self.gradient_tool, Phase::Up, position);
        self.texture_dirty = true;
        *self.message.borrow_mut() = Some("渐变".to_string());
    }

    // -- 吸管 ------------------------------------------------------------

    /// 吸管：取合成后 `position` 处的颜色作为前景色。
    pub(in crate::ui) fn pick_color(&mut self, position: Vec2) {
        let (width, height) = self.document_size();
        let Some((x, y)) = document_to_pixel(position, width, height) else {
            *self.message.borrow_mut() = Some("吸管：点在画布外".to_string());
            return;
        };
        let color = {
            let state = self.state.borrow();
            sample_pixel(&state.document, x, y)
        };
        self.state.borrow_mut().foreground = color;
        tracing::info!(
            target: "image_editor",
            r = color.r,
            g = color.g,
            b = color.b,
            "color_picked"
        );
        *self.message.borrow_mut() = Some(format!(
            "吸管：({x}, {y}) → #{:02X}{:02X}{:02X}",
            color.r, color.g, color.b
        ));
    }
}
