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
