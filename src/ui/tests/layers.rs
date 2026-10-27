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

// -- 树形分组 / 蒙版 / 拖拽 / 右键菜单 -----------------------------------

/// 当前视图的图层行名字（从上到下）。
fn row_names(view: &EditorView) -> Vec<String> {
    view.layer_row_names()
}

#[test]
fn grouping_and_ungrouping_are_one_undo_each() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let top = {
        let mut state = view.state.borrow_mut();
        state.document.add_layer("上层")
    };
    view.update();
    assert_eq!(row_names(&view), vec!["上层", "背景"]);

    view.layer_action(LayerAction::Group);
    view.update();
    let names = row_names(&view);
    assert_eq!(names, vec!["▾ 上层 组", "上层", "背景"], "{names:?}");
    let group_id = view.state.borrow().document.active_layer.unwrap();
    assert!(view.state.borrow().document.children(Some(group_id)).len() == 1);

    view.undo();
    view.update();
    assert_eq!(row_names(&view), vec!["上层", "背景"], "撤销编组");
    assert!(view.state.borrow().document.layer(top).is_some());

    // 再编组 + 取消编组。
    view.layer_action(LayerAction::Group);
    view.update();
    view.layer_action(LayerAction::Ungroup);
    view.update();
    assert_eq!(row_names(&view), vec!["上层", "背景"]);
}

#[test]
fn the_layer_rows_collapse_and_expand() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.state.borrow_mut().document.add_layer("上层");
    view.update();
    view.layer_action(LayerAction::Group);
    view.update();
    let group = view.state.borrow().document.active_layer.unwrap();
    assert_eq!(row_names(&view), vec!["▾ 上层 组", "上层", "背景"]);

    // 双击组行：折叠。
    let center = view.layer_row_center(0).expect("组行");
    view.event(&InputEvent::DoubleClick { position: center });
    view.update();
    assert_eq!(row_names(&view), vec!["▸ 上层 组", "背景"]);
    assert!(!view.state.borrow().document.layer(group).unwrap().expanded);

    // 再双击：展开。
    let center = view.layer_row_center(0).expect("组行");
    view.event(&InputEvent::DoubleClick { position: center });
    view.update();
    assert_eq!(row_names(&view), vec!["▾ 上层 组", "上层", "背景"]);
}

#[test]
fn dragging_a_layer_into_a_group_reparents_it_in_one_undo() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let group = {
        let mut state = view.state.borrow_mut();
        state.document.add_group("组")
    };
    let layer = {
        let mut state = view.state.borrow_mut();
        // 组里的空组会被选为当前节点；在上面加一层（进组）。
        state.document.add_layer("组内")
    };
    view.update();
    view.state.borrow_mut().document.select_layer(group);
    view.update();
    view.layout(viewport());
    let names = row_names(&view);
    assert_eq!(names, vec!["▾ 组", "组内", "背景"]);

    // 拖「组内」到组行的中段 = 放进组（本来就在组里，改不动）。
    let group_row = view.layer_row_center(0).expect("组行");
    let child_row = view.layer_row_center(1).expect("组内行");
    view.event(&InputEvent::PointerDown {
        position: child_row,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove {
        position: child_row - Vec2::new(0.0, 6.0),
    });
    view.event(&InputEvent::PointerMove {
        position: group_row,
    });
    view.event(&InputEvent::PointerUp {
        position: group_row,
        button: PointerButton::Left,
    });
    view.update();
    view.layout(viewport());
    // 已经在组里，所以没有变化（也不该产生一条空命令）。
    assert!(view.state.borrow().document.layer(layer).unwrap().parent == Some(group));

    // 拖背景到组行中段：真的换父。
    let background_row = view.layer_row_center(2).expect("背景行");
    let background = view.state.borrow().document.layers[0].id;
    view.event(&InputEvent::PointerDown {
        position: background_row,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove {
        position: background_row - Vec2::new(0.0, 8.0),
    });
    view.event(&InputEvent::PointerMove {
        position: group_row,
    });
    view.event(&InputEvent::PointerUp {
        position: group_row,
        button: PointerButton::Left,
    });
    view.update();
    assert_eq!(
        view.state
            .borrow()
            .document
            .layer(background)
            .unwrap()
            .parent,
        Some(group),
        "拖进组"
    );

    // 一步撤销回到根层。
    view.undo();
    view.update();
    assert_eq!(
        view.state
            .borrow()
            .document
            .layer(background)
            .unwrap()
            .parent,
        None
    );
}

#[test]
fn a_group_cannot_be_dragged_into_its_own_child() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let group = {
        let mut state = view.state.borrow_mut();
        state.document.add_group("组")
    };
    view.state.borrow_mut().document.add_layer("组内");
    view.update();
    view.layout(viewport());
    let group_row = view.layer_row_center(0).expect("组行");
    let child_row = view.layer_row_center(1).expect("组内行");

    view.event(&InputEvent::PointerDown {
        position: group_row,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove {
        position: group_row - Vec2::new(0.0, 8.0),
    });
    view.event(&InputEvent::PointerMove {
        position: child_row,
    });
    view.event(&InputEvent::PointerUp {
        position: child_row,
        button: PointerButton::Left,
    });
    view.update();

    assert_eq!(
        view.state.borrow().document.layer(group).unwrap().parent,
        None,
        "组不能被拖进自己的子树"
    );
}

#[test]
fn the_layer_context_menu_offers_mask_and_group_actions() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.state.borrow_mut().document.add_layer("上层");
    view.update();
    let row = view.layer_row_center(0).expect("上层行");

    // 右键行：`List::on_context` -> `Intent::LayerMenu` -> `update` 打开菜单。
    view.event(&InputEvent::PointerDown {
        position: row,
        button: PointerButton::Right,
    });
    view.update();
    view.layout(viewport());

    let texts = frame_texts(&view);
    for needle in ["编组", "添加蒙版（选区）", "上移一层", "删除"] {
        assert!(
            texts.iter().any(|text| text.contains(needle)),
            "上下文菜单缺少「{needle}」：{texts:?}"
        );
    }
}

#[test]
fn a_mask_can_be_created_from_the_selection_and_removed_again() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.state.borrow_mut().selection = Some(std::rc::Rc::new(crate::canvas::Selection::rect(
        crate::document::PixelRegion::new(10, 10, 20, 20),
        (128, 128),
    )));
    let id = view.state.borrow().document.active_layer().unwrap().id;
    view.layer_action(LayerAction::AddMask);
    view.update();

    let coverage = {
        let state = view.state.borrow();
        let mask = state.document.layer(id).unwrap().mask.as_ref().unwrap();
        (mask.coverage(15, 15), mask.coverage(0, 0))
    };
    assert_eq!(coverage, (1.0, 0.0), "选区内的像素留下、外面遮住");

    view.layer_action(LayerAction::DeleteMask);
    view.update();
    assert!(view
        .state
        .borrow()
        .document
        .layer(id)
        .unwrap()
        .mask
        .is_none());
}

#[test]
fn painting_on_a_mask_writes_coverage_not_layer_pixels() {
    let mut view = super::brush_view();
    let id = view.state.borrow().document.active_layer().unwrap().id;
    {
        let mut state = view.state.borrow_mut();
        state
            .document
            .set_layer_mask(id, crate::document::LayerMask::opaque(128, 128));
        assert!(state.document.set_active_mask(true));
    }
    let point = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));
    super::click_at(&mut view, point);

    let (layer_pixel, mask_r) = {
        let state = view.state.borrow();
        let layer = state.document.layer(id).unwrap();
        (
            layer.pixels.get_pixel(64, 64),
            layer.mask.as_ref().unwrap().pixels.get_pixel(64, 64).r,
        )
    };
    assert_eq!(layer_pixel, Color::WHITE, "蒙版编辑不该改图层像素");
    assert_eq!(mask_r, 0, "画黑 = 遮住");

    view.undo();
    let mask_r = {
        let state = view.state.borrow();
        state
            .document
            .layer(id)
            .unwrap()
            .mask
            .as_ref()
            .unwrap()
            .pixels
            .get_pixel(64, 64)
            .r
    };
    assert_eq!(mask_r, 255, "撤销只撤蒙版");
}

#[test]
fn bucket_and_delete_are_blocked_while_editing_a_mask() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let id = view.state.borrow().document.active_layer().unwrap().id;
    view.state
        .borrow_mut()
        .document
        .set_layer_mask(id, crate::document::LayerMask::opaque(128, 128));
    view.state.borrow_mut().document.set_active_mask(true);
    view.state.borrow_mut().selection = Some(std::rc::Rc::new(crate::canvas::Selection::rect(
        crate::document::PixelRegion::new(0, 0, 8, 8),
        (128, 128),
    )));

    let before = view
        .state
        .borrow()
        .document
        .layer(id)
        .unwrap()
        .pixels
        .clone();
    assert!(!view.delete_selection(), "蒙版编辑时不删图层像素");
    let after = view
        .state
        .borrow()
        .document
        .layer(id)
        .unwrap()
        .pixels
        .clone();
    assert_eq!(before, after);
}

#[test]
fn the_context_menu_switches_between_layer_and_mask_editing() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let id = view.state.borrow().document.active_layer().unwrap().id;
    view.state
        .borrow_mut()
        .document
        .set_layer_mask(id, crate::document::LayerMask::opaque(128, 128));
    view.layer_action(LayerAction::ToggleEditTarget);
    assert!(view.state.borrow().document.active_mask);
    view.layer_action(LayerAction::ToggleEditTarget);
    assert!(!view.state.borrow().document.active_mask);
}
