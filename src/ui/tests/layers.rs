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

/// 编辑蒙版时，被遮住的像素在**显示纹理**里染红；退出蒙版编辑就恢复。
#[test]
fn editing_a_mask_tints_the_hidden_pixels_red() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let id = view.state.borrow().document.active_layer().unwrap().id;
    {
        let mut state = view.state.borrow_mut();
        let mut mask = crate::document::LayerMask::opaque(128, 128);
        mask.pixels.set_pixel(10, 10, Color::BLACK);
        state.document.set_layer_mask(id, mask);
        assert!(state.document.set_active_mask(true));
    }
    view.update();
    let shown = view.take_texture_upload().expect("切换编辑蒙版应重上传");
    let hidden = shown.get_pixel(10, 10);
    let visible = shown.get_pixel(100, 100);
    assert!(
        hidden.r > hidden.g && hidden.r > hidden.b,
        "遮住的地方该偏红：{hidden:?}"
    );
    assert_eq!(visible, Color::WHITE, "没遮的地方保持原样");

    // 退出蒙版编辑：红色消失。
    view.state.borrow_mut().document.set_active_mask(false);
    view.update();
    let shown = view.take_texture_upload().expect("退出编辑蒙版应重上传");
    assert_eq!(
        shown.get_pixel(10, 10),
        Color::WHITE,
        "红色叠加只属于显示层"
    );
}

/// 涂蒙版的局部更新也带叠加：脏矩形内变红，区域外沿用上一帧。
#[test]
fn the_mask_tint_follows_the_dirty_region() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let id = view.state.borrow().document.active_layer().unwrap().id;
    {
        let mut state = view.state.borrow_mut();
        state
            .document
            .set_layer_mask(id, crate::document::LayerMask::opaque(128, 128));
        state.document.set_active_mask(true);
    }
    view.mark_texture_dirty();
    let _ = view.take_texture_upload().expect("首帧");

    // 在 (10, 10) 遮住一个像素，只报这一块脏。
    {
        let mut state = view.state.borrow_mut();
        let mask = state.document.layer_mut(id).unwrap().mask.as_mut().unwrap();
        mask.pixels.set_pixel(10, 10, Color::BLACK);
    }
    view.mark_region_dirty(crate::document::PixelRegion::new(10, 10, 1, 1));
    let shown = view.take_texture_upload().expect("局部更新");
    let hidden = shown.get_pixel(10, 10);
    let elsewhere = shown.get_pixel(64, 64);
    assert!(hidden.r > hidden.g, "脏区里应该变红：{hidden:?}");
    assert_eq!(elsewhere, Color::WHITE, "区域外不变");
}

/// 红色叠加只在显示纹理里：导出的 PNG 仍然是真实合成结果。
#[test]
fn exporting_ignores_the_mask_tint() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let id = view.state.borrow().document.active_layer().unwrap().id;
    {
        let mut state = view.state.borrow_mut();
        let mut mask = crate::document::LayerMask::opaque(128, 128);
        mask.pixels.set_pixel(10, 10, Color::BLACK);
        state.document.set_layer_mask(id, mask);
        assert!(state.document.set_active_mask(true));
    }
    view.update();
    let _ = view.take_texture_upload().expect("显示纹理");

    let path = std::env::temp_dir().join(format!(
        "image_editor_mask_export_{}.png",
        std::process::id()
    ));
    view.export_png_to(&path.to_string_lossy()).expect("export");
    let exported = crate::io::read_png(&path).expect("decode");
    let _ = std::fs::remove_file(&path);
    assert_eq!(
        exported.get_pixel(10, 10),
        Color::TRANSPARENT,
        "背景层被遮住 -> 导出是透明，不该带红色叠加"
    );
    assert_eq!(exported.get_pixel(100, 100), Color::WHITE);
}

/// 眼睛格子：点一下切换可见性（可撤销），行照旧选中。
#[test]
fn clicking_the_eye_cell_toggles_visibility() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let id = view.state.borrow().document.active_layer().unwrap().id;
    let eye = view.layer_cell_center(0, 3).expect("眼睛格子");
    super::click_at(&mut view, eye);
    assert!(!view.state.borrow().document.layer(id).unwrap().visible);

    view.undo();
    assert!(view.state.borrow().document.layer(id).unwrap().visible);
}

/// 三角格子：点一下折叠 / 展开组。
#[test]
fn clicking_the_triangle_cell_collapses_the_group() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.state.borrow_mut().document.add_layer("上层");
    view.update();
    view.layer_action(LayerAction::Group);
    view.update();
    view.layout(viewport());
    assert_eq!(view.layer_row_count(), 3);

    let triangle = view.layer_cell_center(0, 1).expect("三角格子");
    super::click_at(&mut view, triangle);
    assert_eq!(view.layer_row_count(), 2, "点三角折叠，子节点不占行");
    assert!(view.layer_row_names()[0].starts_with("▸ "));
}

/// 蒙版格子：点一下把编辑目标切到蒙版（没有蒙版时只是选中该行）。
#[test]
fn clicking_the_mask_cell_switches_the_edit_target() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let id = view.state.borrow().document.active_layer().unwrap().id;
    view.state
        .borrow_mut()
        .document
        .set_layer_mask(id, crate::document::LayerMask::opaque(128, 128));
    view.update();
    view.layout(viewport());

    let mask = view.layer_cell_center(0, 2).expect("蒙版格子");
    super::click_at(&mut view, mask);
    assert!(view.state.borrow().document.active_mask);

    // 再点一下回到图层像素。
    let mask = view.layer_cell_center(0, 2).expect("蒙版格子");
    super::click_at(&mut view, mask);
    assert!(!view.state.borrow().document.active_mask);
}

/// 名字格子：单击选中（格子点击不会走整行激活，需要自己兜底）。
#[test]
fn clicking_the_name_cell_selects_the_row() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let background = view.state.borrow().document.layers[0].id;
    let top = view.state.borrow_mut().document.add_layer("上层");
    view.update();
    view.layout(viewport());
    assert_eq!(view.state.borrow().document.active_layer, Some(top));

    let name = view.layer_cell_center(1, 4).expect("名字格子");
    super::click_at(&mut view, name);
    assert_eq!(view.state.borrow().document.active_layer, Some(background));
}
