//! 图层：重命名浮层。

use crate::selfcheck::*;
use crate::ui::EditorView;
use igui_core::{InputEvent, Key};

/// 图层重命名：点「重命名」按钮弹输入框浮层，改文本 + Enter 提交。
pub(crate) fn check_rename(failures: &mut Vec<String>) {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let button = view.rename_button_center().expect("「重命名」按钮");
    click_at(&mut view, button);
    view.update();
    view.layout(viewport());

    let Some(field) = view.rename_field() else {
        failures.push("点「重命名」应打开输入框浮层".to_string());
        return;
    };
    field.borrow_mut().set_text("Hero");
    view.event(&InputEvent::KeyDown { key: Key::Enter });
    view.update();
    view.layout(viewport());
    if !contains(&paint_frame(&view), "Hero") {
        failures.push("重命名没有提交到图层列表".to_string());
    }
}
