//! 钢笔的指针适配 + 悬停 / 键位转发。
//!
//! 钢笔自己（[`crate::tools::PenTool`]）只攒锚点；路径怎么变成选区 / 填充 /
//! 描边在 [`crate::ui::pen`] 里，这里只负责把事件转进去。

use igui_core::{Key, Vec2};

use crate::app::state::ActiveTool;
use crate::ui::{EditorView, PEN_HIT_RADIUS};

use super::{drive, Phase};

impl EditorView {
    pub(in crate::ui) fn begin_pen(&mut self, position: Vec2) {
        self.sync_pen_radius();
        drive(&self.state, &mut self.pen, Phase::Down, position);
    }

    pub(in crate::ui) fn continue_pen(&mut self, position: Vec2) {
        drive(&self.state, &mut self.pen, Phase::Move, position);
    }

    pub(in crate::ui) fn end_pen(&mut self, position: Vec2) {
        drive(&self.state, &mut self.pen, Phase::Up, position);
    }

    /// 把「命中锚点 / 闭合」的半径换算到文档单位（屏幕上恒为 `PEN_HIT_RADIUS`）。
    pub(in crate::ui) fn sync_pen_radius(&mut self) {
        let zoom = self.state.borrow().canvas.zoom.max(0.01);
        self.pen.radius = PEN_HIT_RADIUS / zoom;
    }

    /// 钢笔画到一半时的悬停：更新橡皮筋与闭合提示。
    ///
    /// 不按键的移动不会走 `continue_tool`（那里要求 `is_dragging`），所以指针
    /// 进 / 出画布都要单独转发一次。
    pub(in crate::ui) fn hover_pen(&mut self, screen: Vec2) {
        self.sync_pen_radius();
        let inside = self.canvas_area_contains(screen);
        self.pen
            .set_hover(inside.then(|| self.document_position(screen)));
    }

    /// 钢笔的键位：Esc 取消 / Enter 收尾 / Backspace 删最后一个锚点。
    ///
    /// 只在钢笔是当前工具、且确实有路径可动时消费事件。
    pub(in crate::ui) fn handle_pen_key(&mut self, key: Key) -> bool {
        if self.state.borrow().active_tool != ActiveTool::Pen {
            return false;
        }
        match key {
            Key::Escape if self.pen.cancel() => {
                *self.message.borrow_mut() = Some("钢笔：已取消路径".to_string());
                return true;
            }
            Key::Enter => return self.finish_pen_path(),
            Key::Backspace if self.pen.undo_anchor() => {
                *self.message.borrow_mut() = Some(format!("钢笔：{}", self.pen_summary()));
                return true;
            }
            _ => {}
        }
        false
    }
}
