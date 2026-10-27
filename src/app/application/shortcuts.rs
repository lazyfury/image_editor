//! 快捷键 / 确认结果解析（纯函数，便于单测）。

use super::*;

pub(super) fn resolve_confirm(
    result: Option<ConfirmResult>,
    closed: bool,
) -> Option<ConfirmResult> {
    match (result, closed) {
        (Some(result), _) => Some(result),
        (None, true) => Some(ConfirmResult::Cancel),
        (None, false) => None,
    }
}

/// Ctrl/Cmd 按下时的字母键（小写）。
pub(super) fn command_key(event: &InputEvent) -> Option<char> {
    match event {
        InputEvent::KeyDown {
            key: Key::Character(ch),
        } => Some(ch.to_ascii_lowercase()),
        _ => None,
    }
}

/// Ctrl/Cmd + S / O / E 的文件快捷键；命中返回意图。
pub(super) fn file_shortcut(command: bool, shift: bool, event: &InputEvent) -> Option<FileRequest> {
    if !command {
        return None;
    }
    match command_key(event)? {
        's' if shift => Some(FileRequest::SaveAs),
        's' => Some(FileRequest::Save),
        'o' => Some(FileRequest::Open),
        'e' => Some(FileRequest::ExportPng),
        _ => None,
    }
}

/// Ctrl/Cmd+X / C / V（Shift 时「粘贴为新图层」）：剪切 / 复制 / 粘贴；命中返回 `true`。
pub(super) fn clipboard_shortcut(
    editor: &mut EditorView,
    command: bool,
    shift: bool,
    event: &InputEvent,
) -> bool {
    if !command {
        return false;
    }
    let Some(key) = command_key(event) else {
        return false;
    };
    match key {
        'x' => {
            editor.cut_selection();
            true
        }
        'c' => {
            editor.copy_selection();
            true
        }
        'v' if shift => {
            editor.paste_as_new_layer();
            true
        }
        'v' => {
            editor.paste_clipboard();
            true
        }
        _ => false,
    }
}

/// Ctrl/Cmd+A 全选、Shift+Ctrl/Cmd+I 反选；命中返回 `true`。
pub(super) fn selection_shortcut(
    editor: &mut EditorView,
    command: bool,
    shift: bool,
    event: &InputEvent,
) -> bool {
    if !command {
        return false;
    }
    let Some(key) = command_key(event) else {
        return false;
    };
    match key {
        'a' => editor.select_all(),
        'i' if shift => editor.invert_selection(),
        _ => false,
    }
}

/// Ctrl/Cmd+G 编组、Shift+Ctrl/Cmd+G 取消编组、Cmd+J 复制、Shift+Cmd+N 新建图层、
/// Cmd+Backspace 删除当前节点；命中返回 `true`。
pub(super) fn layer_shortcut(
    editor: &mut EditorView,
    command: bool,
    shift: bool,
    event: &InputEvent,
) -> bool {
    if !command {
        return false;
    }
    if matches!(
        event,
        InputEvent::KeyDown {
            key: Key::Backspace | Key::Delete
        }
    ) {
        editor.layer_action(crate::ui::LayerAction::Delete);
        return true;
    }
    let Some(key) = command_key(event) else {
        return false;
    };
    let action = match key {
        'g' if shift => crate::ui::LayerAction::Ungroup,
        'g' => crate::ui::LayerAction::Group,
        'j' => crate::ui::LayerAction::Duplicate,
        'n' if shift => crate::ui::LayerAction::NewLayer,
        _ => return false,
    };
    editor.layer_action(action);
    true
}

/// Ctrl/Cmd+Z 撤销、Shift+Ctrl/Cmd+Z（或 Ctrl+Y）重做；命中返回 `true`。
pub(super) fn history_shortcut(
    editor: &mut EditorView,
    command: bool,
    shift: bool,
    event: &InputEvent,
) -> bool {
    if !command {
        return false;
    }
    let InputEvent::KeyDown {
        key: Key::Character(ch),
    } = event
    else {
        return false;
    };
    match ch.to_ascii_lowercase() {
        'z' if shift => {
            editor.redo();
            true
        }
        'z' => {
            editor.undo();
            true
        }
        'y' => {
            editor.redo();
            true
        }
        _ => false,
    }
}
