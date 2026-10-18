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
