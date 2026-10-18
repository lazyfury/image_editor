//! 确认对话框：关闭有未保存改动的工作区时弹「保存 / 不保存 / 取消」。
//!
//! 复用 [`crate::ui::window::Window`]（模拟桌面的浮动窗口：可拖动 / 缩放 /
//! 关闭，输入全吞掉）。结果只写一个 [`ConfirmResult`] 请求格，宿主取走执行；
//! 关窗口按钮 / Esc 等同于「取消」。

use std::cell::Cell;
use std::rc::Rc;

use igui_components::{Button, Component, Flex, Text};
use igui_core::{Edges, EventResult, InputEvent, Vec2, ViewportSize};
use igui_render::PaintContext;
use igui_theme::{space, Theme, Tone};
use igui_ui::{Justify, MouseFilter, TextMeasurer};

use super::window::Window;

/// 用户对「要不要保存」的选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmResult {
    /// 先保存，再关闭。
    Save,
    /// 不保存，直接关闭。
    Discard,
    /// 取消关闭。
    Cancel,
}

/// 确认对话框。
pub struct ConfirmDialog {
    window: Window,
    result: Rc<Cell<Option<ConfirmResult>>>,
}

impl ConfirmDialog {
    /// 建一个确认框（`title` 是标题栏文字，`message` 是正文）。
    pub fn new(theme: &'static dyn Theme, title: &str, message: &str) -> Self {
        let result: Rc<Cell<Option<ConfirmResult>>> = Rc::new(Cell::new(None));
        let save = result.clone();
        let discard = result.clone();
        let cancel = result.clone();
        let window =
            Window::new(
                theme,
                title,
                Vec2::new(420.0, 190.0),
                Vec2::new(320.0, 150.0),
                move |tree, content| {
                    Flex::column()
                        .gap(space::LG)
                        .padding(Edges::ZERO)
                        .mouse_filter(MouseFilter::Ignore)
                        .child(Text::small(message, theme).tone(Tone::Muted))
                        .child(
                            Flex::row()
                                .gap(space::SM)
                                .padding(Edges::ZERO)
                                .justify(Justify::End)
                                .mouse_filter(MouseFilter::Ignore)
                                .child(Button::primary("保存", theme).on_click(
                                    move |_tree, _id| save.set(Some(ConfirmResult::Save)),
                                ))
                                .child(Button::secondary("不保存", theme).on_click(
                                    move |_tree, _id| discard.set(Some(ConfirmResult::Discard)),
                                ))
                                .child(Button::ghost("取消", theme).on_click(move |_tree, _id| {
                                    cancel.set(Some(ConfirmResult::Cancel))
                                })),
                        )
                        .build(tree, content);
                },
            );
        Self { window, result }
    }

    /// 用后端真实字体度量排版。
    pub fn set_text_measurer(&mut self, measurer: Rc<dyn TextMeasurer>) {
        self.window.set_text_measurer(measurer);
    }

    /// 用户的选择；取走一次。
    pub fn take_result(&self) -> Option<ConfirmResult> {
        self.result.take()
    }

    /// 测试用：直接设一个结果（模拟点了某个按钮）。
    #[cfg(test)]
    pub(crate) fn choose(&self, result: ConfirmResult) {
        self.result.set(Some(result));
    }

    /// 关窗口按钮 / Esc / 点窗口外（等同于取消）；取走一次。
    pub fn take_close_request(&self) -> bool {
        self.window.take_close_request()
    }

    pub fn layout(&mut self, viewport: ViewportSize) {
        self.window.layout(viewport);
    }

    pub fn event(&mut self, event: &InputEvent) -> EventResult {
        self.window.event(event)
    }

    pub fn paint(&self, ctx: &mut PaintContext) {
        self.window.paint(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use igui_backend_recording::RecordingBackend;
    use igui_core::Size;
    use igui_render::{DrawCommand, RenderBackend};

    /// 确认框得把三个选项都画出来（保存 / 不保存 / 取消）。
    #[test]
    fn the_dialog_shows_save_discard_and_cancel() {
        let theme = crate::theme::editor_theme(false);
        let mut dialog = ConfirmDialog::new(theme, "未保存的改动", "要保存吗？");
        let viewport = ViewportSize::new(Size::new(800.0, 600.0));
        dialog.layout(viewport);

        let mut backend = RecordingBackend::new();
        backend.begin_frame(viewport).expect("begin frame");
        let mut ctx = PaintContext::new();
        dialog.paint(&mut ctx);
        backend.submit(&ctx.into_draw_list()).expect("submit frame");
        backend.end_frame().expect("end frame");
        let frame = backend.last_frame().expect("a frame was recorded").clone();

        let texts: Vec<String> = frame
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        for label in ["保存", "不保存", "取消"] {
            assert!(
                texts.iter().any(|text| text.contains(label)),
                "确认框应显示「{label}」"
            );
        }
    }
}
