//! 图层：属性面板、裁剪到文档、重命名浮层。

use super::*;

/// 点图层面板的「重命名」按钮。
fn click_rename(view: &mut EditorView) {
    let center = view
        .rename_button_center()
        .expect("图层面板应有「重命名」按钮");
    click(view, center);
}

#[test]
fn the_properties_panel_follows_the_active_layer() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 新图层成为当前图层，属性面板应显示它的名字。
    view.state.borrow_mut().document.add_layer("上层");
    select_tab(&mut view, SidebarTab::Properties);
    assert!(frame_texts(&view).iter().any(|text| text.contains("上层")));
}

#[test]
fn the_properties_panel_shows_layer_geometry() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let id = view.state.borrow().document.active_layer().unwrap().id;
    view.state
        .borrow_mut()
        .document
        .set_layer_position(id, Point::new(3, -2));
    select_tab(&mut view, SidebarTab::Properties);
    let texts = frame_texts(&view);
    assert!(
        texts.iter().any(|text| text.contains("偏移 (3, -2)")),
        "texts = {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains("缓冲 128×128")),
        "texts = {texts:?}"
    );
}

#[test]
fn the_crop_action_shrinks_a_moved_layer_back_to_the_document() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let id = view.state.borrow().document.active_layer().unwrap().id;
    {
        let mut state = view.state.borrow_mut();
        state.document.set_layer_position(id, Point::new(10, 0));
        state.document.ensure_layer_covers_document(id);
    }
    assert!(view.state.borrow().document.layer(id).unwrap().pixels.width > 128);

    view.apply_menu_action(super::menu::MenuAction::CropLayerToDocument);
    {
        let state = view.state.borrow();
        let layer = state.document.layer(id).unwrap();
        assert_eq!((layer.pixels.width, layer.pixels.height), (128, 128));
        assert_eq!(layer.position, Point::ZERO);
    }
    assert!(view.take_texture_upload().is_some(), "裁剪后要重合成");
}

#[test]
fn renaming_the_active_layer_uses_text_input() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 点真实按钮：它把自己的 `NodeId` 写进请求格，兼作浮层锚点。
    click_rename(&mut view);
    view.update();
    view.layout(viewport());

    let edit = view
        .rename
        .as_ref()
        .expect("应打开重命名浮层")
        .edit
        .borrow()
        .clone()
        .expect("浮层内容应建好输入框");

    // 浮层锚在按钮上方，整块落在视口里（否则输入框点不到）。
    let overlay = view.rename.as_ref().expect("浮层已开").overlay;
    let popover = view.overlays.rect(overlay).expect("浮层应已布局");
    let screen = viewport().logical_size();
    assert!(
        popover.origin.x >= 0.0
            && popover.origin.y >= 0.0
            && popover.origin.x + popover.size.width <= screen.width
            && popover.origin.y + popover.size.height <= screen.height,
        "浮层应完整落在视口里：{popover:?}"
    );

    edit.borrow_mut().select_all();
    view.event(&InputEvent::TextInput {
        text: "Hero".into(),
    });
    view.event(&InputEvent::KeyDown { key: Key::Enter });

    assert!(view.rename.is_none(), "Enter 结束重命名");
    let name = view
        .state
        .borrow()
        .document
        .active_layer()
        .map(|layer| layer.name.clone());
    assert_eq!(name.as_deref(), Some("Hero"));
}

/// 「确定」按钮只写共享格；`update` 取走后走同一条提交路径。
#[test]
fn the_rename_confirm_request_commits() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    click_rename(&mut view);
    view.update();
    view.layout(viewport());

    {
        let rename = view.rename.as_ref().expect("应打开重命名浮层");
        rename
            .edit
            .borrow()
            .as_ref()
            .unwrap()
            .borrow_mut()
            .set_text("Hero");
        rename.confirm.set(true);
    }
    view.update();

    assert!(view.rename.is_none(), "确定后浮层关闭");
    assert_eq!(
        view.state.borrow().document.active_layer().unwrap().name,
        "Hero"
    );
}

#[test]
fn escape_cancels_a_rename() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    click_rename(&mut view);
    view.update();
    view.layout(viewport());
    assert!(view.rename.is_some(), "应打开重命名浮层");

    // Esc 由浮层自己吞掉（`dismiss_on_escape`），下一帧 `update` 清掉记录。
    view.event(&InputEvent::KeyDown { key: Key::Escape });
    view.update();
    assert!(view.rename.is_none());
    assert_eq!(
        view.state.borrow().document.active_layer().unwrap().name,
        "背景",
        "取消后名字不变"
    );
}
