//! 图层：重命名浮层。

use crate::selfcheck::*;
use crate::ui::EditorView;
use igui_core::{InputEvent, Key};

/// 图层树：新建组后再建图层会进组；双击组行折叠 / 展开（行数跟着变）。
pub(crate) fn check_layer_tree(failures: &mut Vec<String>) {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.layer_action(crate::ui::LayerAction::NewGroup);
    view.update();
    view.layout(viewport());
    view.layer_action(crate::ui::LayerAction::NewLayer);
    view.update();
    view.layout(viewport());

    let names = view.layer_row_names();
    if !names.iter().any(|name| name.starts_with("▾ ")) {
        failures.push(format!("新建组后应该有展开的组行：{names:?}"));
    }
    if view.layer_row_count() != 3 {
        failures.push(format!(
            "组 + 组内图层 + 背景 应该是 3 行，实际 {}：{names:?}",
            view.layer_row_count()
        ));
    }

    // 点三角格子折叠：子节点不再占行。
    let Some(center) = view.layer_cell_center(0, 1) else {
        failures.push("找不到三角格子".to_string());
        return;
    };
    click_at(&mut view, center);
    view.update();
    view.layout(viewport());
    let collapsed = view.layer_row_names();
    if view.layer_row_count() != 2 || !collapsed.iter().any(|name| name.starts_with("▸ ")) {
        failures.push(format!("双击组行应折叠成 2 行：{collapsed:?}"));
    }
}

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
