//! 图层重命名浮层：`Overlays::popover` 里的一个小表单。
//!
//! 复用上游的 [`TextInput`]（自带光标 / 选区 / 剪贴板 / IME preedit），取代原先
//! 「手写按键缓冲 + 状态栏提示」的做法。输入框挂载即聚焦（`autofocus`）；Enter
//! 提交由 [`EditorView`](crate::ui::EditorView) 在事件层拦下，和「确定」按钮走
//! 同一条 [`EditorView::commit_rename`](crate::ui::EditorView)。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use igui_components::{Button, Component, Flex, TextInput};
use igui_core::NodeId;
use igui_scene::SceneTree;
use igui_theme::{space, Theme};
use igui_ui::{Justify, TextEdit};

/// 输入框最小宽度。
const FIELD_WIDTH: f32 = 180.0;

/// 输入框 [`TextEdit`] 的交回槽。
///
/// 浮层内容是在 `Overlays::layout` 里才建的，所以打开浮层的那一刻还拿不到字段
/// 句柄，只能先给个空槽，等 `content` 跑完再读。
pub type EditSlot = Rc<RefCell<Option<Rc<RefCell<TextEdit>>>>>;

/// 往浮层 `root` 挂「重命名图层」表单：输入框 + 取消 / 确定。
///
/// `value` 是打开时的图层名；`slot` 交回输入框的字段句柄；`confirm` / `cancel`
/// 是共享格，由 `EditorView::update` 轮询后提交 / 关闭。
pub fn content(
    tree: &mut SceneTree,
    root: NodeId,
    theme: &'static dyn Theme,
    value: &str,
    slot: &EditSlot,
    confirm: Rc<Cell<bool>>,
    cancel: Rc<Cell<bool>>,
) {
    // 浮层被重建（`Overlays::invalidate`）时把已输入的内容带过来，不丢字。
    let initial = slot
        .borrow()
        .as_ref()
        .map(|edit| edit.borrow().text().to_string())
        .unwrap_or_else(|| value.to_string());
    let input = TextInput::new(theme)
        .value(initial)
        .min_width(FIELD_WIDTH)
        .autofocus(true);
    *slot.borrow_mut() = Some(input.shared());

    let ok = Button::primary("确定", theme).on_click(move |_tree, _id| confirm.set(true));
    let no = Button::secondary("取消", theme).on_click(move |_tree, _id| cancel.set(true));

    Flex::column()
        .gap(space::SM)
        .child(input)
        .child(
            Flex::row()
                .gap(space::SM)
                .justify(Justify::End)
                .child(no)
                .child(ok),
        )
        .build(tree, root);
}
