//! 文件面板（Phase 7 / 23）：打开 / 保存 / 另存为 / 导入 / 导出。
//!
//! 不再有「手动路径」输入：每个按钮只往意图队列里塞一个 [`Intent::File`]，路径
//! 由宿主的原生文件对话框（[`crate::io::FileDialog`]）选出来。面板顶部一行是
//! **只读**的当前工作区文件路径（由宿主 [`crate::ui::EditorView::set_workspace_path`]
//! 喂进来）。
//!
//! 真正的文件 IO 在宿主（[`crate::app::application`]）：视图只发意图，因为
//! 回调拿不到 `&mut self`，也拿不到 `FileDialog`。

use igui_components::{Button, Component, Flex, NodeRef, Text};
use igui_core::Edges;
use igui_theme::{space, TextSize, Theme, Tone};
use igui_ui::MouseFilter;

use super::{FileRequest, Intent, Intents};

/// 文件标签页的内容（标题由 [`TabsView`](crate::ui::tabs::TabsView) 的标签提供）。
/// `buttons` 收集按钮节点，测试与自检靠它们真的点一下。
pub fn file_panel(
    theme: &'static dyn Theme,
    intents: Intents,
    path_label: &NodeRef,
    buttons: &mut Vec<(FileRequest, NodeRef)>,
) -> impl Component {
    let save_row = action_row(
        theme,
        &[FileRequest::Save, FileRequest::SaveAs],
        intents.clone(),
        buttons,
    );
    let open_row = action_row(theme, &[FileRequest::Open], intents.clone(), buttons);
    let png_row = action_row(
        theme,
        &[FileRequest::ImportPng, FileRequest::ExportPng],
        intents,
        buttons,
    );

    Flex::column()
        .gap(space::SM)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::caption("工作区文件", theme).tone(Tone::Muted))
        .child(
            Text::caption("未保存（⌘S 保存）", theme)
                .max_lines(2)
                .ellipsis(true)
                .ref_(path_label),
        )
        .child(save_row)
        .child(open_row)
        .child(png_row)
}

/// 一行动作按钮。
fn action_row(
    theme: &'static dyn Theme,
    actions: &[FileRequest],
    intents: Intents,
    buttons: &mut Vec<(FileRequest, NodeRef)>,
) -> impl Component {
    let mut row = Flex::row()
        .wrap(true)
        .gap(space::XXS)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore);
    for action in actions {
        let slot = NodeRef::new();
        buttons.push((*action, slot.clone()));
        row = row.child(action_button(theme, *action, intents.clone(), &slot));
    }
    row
}

/// 一个动作按钮：点击只设置意图，宿主在 `update` 里弹对话框并执行。
fn action_button(
    theme: &'static dyn Theme,
    action: FileRequest,
    intents: Intents,
    slot: &NodeRef,
) -> impl Component {
    let button = match action {
        FileRequest::Save => Button::primary(action.label(), theme),
        _ => Button::secondary(action.label(), theme),
    };
    button
        .font_size(TextSize::Small.px())
        .on_click(move |_tree, _id| intents.push(Intent::File(action)))
        .ref_(slot)
}
