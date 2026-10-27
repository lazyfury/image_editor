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
use igui_ui::{MouseFilter, TextMeasurer};

use super::window::{Window, FOOTER_BUTTON_MIN_WIDTH};

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
    /// 指针下的控件光标（宿主选光标用）。
    pub fn hovered_cursor(&self) -> igui_core::Cursor {
        self.window.hovered_cursor()
    }

    /// 建一个确认框（`title` 是标题栏文字，`message` 是正文）。
    pub fn new(theme: &'static dyn Theme, title: &str, message: &str) -> Self {
        let result: Rc<Cell<Option<ConfirmResult>>> = Rc::new(Cell::new(None));
        let save = result.clone();
        let discard = result.clone();
        let cancel = result.clone();
        let window = Window::new(
            theme,
            title,
            Vec2::new(420.0, 190.0),
            Vec2::new(320.0, 150.0),
            move |tree, content, footer| {
                Text::small(message, theme)
                    .tone(Tone::Muted)
                    .build(tree, content);
                Flex::row()
                    .gap(space::SM)
                    .padding(Edges::ZERO)
                    .mouse_filter(MouseFilter::Ignore)
                    .child(
                        Button::primary("保存", theme)
                            .min_size(FOOTER_BUTTON_MIN_WIDTH, 0.0)
                            .on_click(move |_tree, _id| save.set(Some(ConfirmResult::Save))),
                    )
                    .child(
                        Button::secondary("不保存", theme)
                            .min_size(FOOTER_BUTTON_MIN_WIDTH, 0.0)
                            .on_click(move |_tree, _id| discard.set(Some(ConfirmResult::Discard))),
                    )
                    .child(
                        Button::ghost("取消", theme)
                            .min_size(FOOTER_BUTTON_MIN_WIDTH, 0.0)
                            .on_click(move |_tree, _id| cancel.set(Some(ConfirmResult::Cancel))),
                    )
                    .build(tree, footer);
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

    /// 页脚按钮要有统一的宽度，不能跟着文字长度 / 字形宽度跑。
    #[test]
    fn the_footer_buttons_are_the_same_size() {
        let theme = crate::theme::editor_theme(false);
        let mut dialog = ConfirmDialog::new(theme, "t", "m");
        dialog.layout(ViewportSize::new(Size::new(800.0, 600.0)));
        let tree = dialog.window.tree();
        // 页脚里的按钮：高 24，宽在 [40, 120]（排除外层行与关闭按钮）。
        let sizes: Vec<Size> = tree
            .iter()
            .filter_map(|id| igui_ui::control(tree, id))
            .map(|control| control.rect.size)
            .filter(|size| (size.height - 24.0).abs() < 0.5 && (40.0..=120.0).contains(&size.width))
            .collect();
        assert_eq!(sizes.len(), 3, "应有三个页脚按钮：{sizes:?}");
        assert!(
            sizes.iter().all(|size| *size == sizes[0]),
            "页脚按钮应一样大：{sizes:?}"
        );
    }

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
