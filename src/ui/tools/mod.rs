//! 工具输入分发：把指针事件交给当前工具。
//!
//! 这里只回答「当前是哪个工具、交给谁」；每类工具的适配代码在子模块里：
//! [`paint`]（画笔 / 橡皮 / 移动 / 油漆桶 / 矩形填充 / 渐变 / 吸管）、
//! [`select`]（框选 / 套索 / 魔棒 / 自由填充 + 选区命令）、[`pen`]（钢笔）、
//! [`adjust`]（选项栏 `−` / `+` 的数值请求）。
//!
//! 工具本身（[`crate::tools`]）只认识文档坐标和 `Document`；这里负责屏幕 ->
//! 文档的换算、把 `AppState` 的前景色 / 选区喂进去、以及改完像素后标脏。

mod adjust;
mod paint;
mod pen;
mod select;

use std::cell::RefCell;

use igui_core::Vec2;

use crate::app::state::{ActiveTool, AppState};
use crate::tools::{Tool, ToolContext};

use super::{left_pointer, EditorView};

/// 一次指针事件在工具侧的三个阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Down,
    Move,
    Up,
}

/// 借出 `ToolContext` 跑一段会改文档的逻辑。
///
/// 上下文只借 `document` / `history` 两个字段，所以调用方可以同时可变借用
/// 工具本身（`with_ctx(&self.state, |ctx| self.brush.on_pointer_down(..))`）。
pub(super) fn with_ctx<R>(
    state: &RefCell<AppState>,
    f: impl FnOnce(&mut ToolContext<'_>) -> R,
) -> R {
    let mut state = state.borrow_mut();
    let AppState {
        document, history, ..
    } = &mut *state;
    let mut ctx = ToolContext { document, history };
    f(&mut ctx)
}

/// 把一次指针事件交给某个工具实现。
pub(super) fn drive<T: Tool>(
    state: &RefCell<AppState>,
    tool: &mut T,
    phase: Phase,
    position: Vec2,
) {
    let event = left_pointer(position);
    with_ctx(state, |ctx| match phase {
        Phase::Down => tool.on_pointer_down(ctx, event),
        Phase::Move => tool.on_pointer_move(ctx, event),
        Phase::Up => tool.on_pointer_up(ctx, event),
    });
}

impl EditorView {
    pub(super) fn is_dragging(&self) -> bool {
        self.brush.is_drawing()
            || self.move_tool.is_moving()
            || self.select_anchor.is_some()
            || !self.lasso_points.is_empty()
            || self.rect_fill.is_filling()
            || self.gradient.tool.is_drawing()
            || self.pen.is_dragging()
    }

    /// 左键落下：把屏幕点换算成文档坐标，交给当前工具。
    pub(super) fn begin_tool(&mut self, screen: Vec2) {
        let tool = self.state.borrow().active_tool;
        // 视图工具在 `input` 的导航分支就消费了；这里兜一层，保证它们永远
        // 不会落到文档上（蒙版 / 图层组的守卫也不该拦它们）。
        if matches!(tool, ActiveTool::Hand | ActiveTool::Zoom) {
            return;
        }
        // 蒙版只用画笔 / 橡皮编辑：其它工具会写到图层像素上，直接拦掉。
        if self.state.borrow().document.active_mask
            && !matches!(tool, ActiveTool::Brush | ActiveTool::Eraser)
        {
            *self.message.borrow_mut() = Some("蒙版只能用画笔编辑".to_string());
            return;
        }
        if self.state.borrow().document.active_is_group()
            && matches!(
                tool,
                ActiveTool::Brush
                    | ActiveTool::Eraser
                    | ActiveTool::PaintBucket
                    | ActiveTool::RectangleFill
                    | ActiveTool::Gradient
                    | ActiveTool::Move
            )
        {
            *self.message.borrow_mut() = Some("这是图层组：先选中组里的图层".to_string());
            return;
        }
        let position = self.document_position(screen);
        match tool {
            ActiveTool::Brush | ActiveTool::Eraser => self.begin_brush(tool, position),
            ActiveTool::Move => self.begin_move(position),
            ActiveTool::RectangleSelect => self.begin_rect_select(position),
            ActiveTool::LassoSelect => self.begin_lasso(position),
            ActiveTool::FreeFill => self.begin_free_fill(position),
            ActiveTool::Pen => self.begin_pen(position),
            ActiveTool::PaintBucket => self.begin_bucket(position),
            ActiveTool::MagicWand => self.begin_magic_wand(position),
            ActiveTool::RectangleFill => self.begin_rect_fill(position),
            ActiveTool::Gradient => self.begin_gradient(position),
            ActiveTool::Eyedropper => self.pick_color(position),
            ActiveTool::Hand | ActiveTool::Zoom => {}
        }
    }

    pub(super) fn continue_tool(&mut self, screen: Vec2) {
        let tool = self.state.borrow().active_tool;
        if self.state.borrow().document.active_is_group()
            && matches!(
                tool,
                ActiveTool::Brush
                    | ActiveTool::Eraser
                    | ActiveTool::PaintBucket
                    | ActiveTool::RectangleFill
                    | ActiveTool::Gradient
                    | ActiveTool::Move
            )
        {
            *self.message.borrow_mut() = Some("这是图层组：先选中组里的图层".to_string());
            return;
        }
        let position = self.document_position(screen);
        match tool {
            ActiveTool::Brush | ActiveTool::Eraser if self.brush.is_drawing() => {
                self.continue_brush(position);
            }
            ActiveTool::Move if self.move_tool.is_moving() => self.continue_move(position),
            ActiveTool::RectangleSelect => self.update_selection(position),
            ActiveTool::Pen if self.pen.is_dragging() => self.continue_pen(position),
            ActiveTool::LassoSelect | ActiveTool::FreeFill if !self.lasso_points.is_empty() => {
                self.continue_lasso(position);
            }
            ActiveTool::RectangleFill if self.rect_fill.is_filling() => {
                self.continue_rect_fill(position);
            }
            ActiveTool::Gradient if self.gradient.tool.is_drawing() => {
                self.continue_gradient(position);
            }
            _ => {}
        }
    }

    pub(super) fn end_tool(&mut self, screen: Vec2) {
        let tool = self.state.borrow().active_tool;
        if self.state.borrow().document.active_is_group()
            && matches!(
                tool,
                ActiveTool::Brush
                    | ActiveTool::Eraser
                    | ActiveTool::PaintBucket
                    | ActiveTool::RectangleFill
                    | ActiveTool::Gradient
                    | ActiveTool::Move
            )
        {
            *self.message.borrow_mut() = Some("这是图层组：先选中组里的图层".to_string());
            return;
        }
        let position = self.document_position(screen);
        match tool {
            ActiveTool::Brush | ActiveTool::Eraser if self.brush.is_drawing() => {
                self.end_brush(position);
            }
            ActiveTool::Move if self.move_tool.is_moving() => self.end_move(position),
            ActiveTool::Pen => self.end_pen(position),
            // 注意：`take()` 在守卫里，退化拖动（点一下没拖）也要清掉起点。
            ActiveTool::RectangleSelect if self.select_anchor.take().is_some() => {
                self.end_rect_select(position);
            }
            ActiveTool::LassoSelect if !self.lasso_points.is_empty() => {
                self.end_lasso(position);
            }
            ActiveTool::FreeFill if !self.lasso_points.is_empty() => {
                self.end_free_fill(position);
            }
            ActiveTool::RectangleFill if self.rect_fill.is_filling() => {
                self.end_rect_fill(position);
            }
            ActiveTool::Gradient if self.gradient.tool.is_drawing() => {
                self.end_gradient(position);
            }
            _ => {}
        }
    }
}
