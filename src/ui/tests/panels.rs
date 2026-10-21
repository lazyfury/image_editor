//! 面板：调色盘（色块 / 取色器）、分隔条拖动、历史面板、图层面板列表。

use super::*;

#[test]
fn palette_swatches_wrap_within_the_panel() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let c0 = view.palette_swatch_center(0).expect("swatch 0");
    let c1 = view.palette_swatch_center(1).expect("swatch 1");
    let last = view.palette_swatch_center(15).expect("swatch 15");
    assert!((c0.y - c1.y).abs() < 0.5, "同一行的前两个 y 应相同");
    assert!(last.y > c0.y + 1.0, "最后一个色块应换到下面的行");
    // 每行不超过面板宽度。
    assert!(c1.x > c0.x, "同一行从左往右排");
}

#[test]
fn clicking_a_palette_swatch_sets_the_foreground() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    view.layout(viewport());
    assert_eq!(view.foreground(), Color::BLACK, "默认前景是黑");
    assert!(view.palette_swatch_count() >= 16);

    // SWATCHES[4] = (255, 0, 0)。
    let center = view.palette_swatch_center(4).expect("swatch mounted");
    view.event(&InputEvent::PointerDown {
        position: center,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: center,
        button: PointerButton::Left,
    });
    view.update();
    assert_eq!(view.foreground(), Color::RED);
}

#[test]
fn dragging_the_picker_sets_the_foreground() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    view.layout(viewport());
    assert_eq!(view.foreground(), Color::BLACK, "默认前景是黑");

    let center = view.palette_picker_center().expect("picker mounted");
    view.event(&InputEvent::PointerDown {
        position: center,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: center,
        button: PointerButton::Left,
    });
    view.update();
    let fg = view.foreground();
    assert_ne!(fg, Color::BLACK, "取色器应改前景色");
    assert!(
        fg.r > fg.g && fg.g == fg.b,
        "中心应是色相 0 / s=v=0.5 的暗红，得到 {fg:?}"
    );
}

#[test]
fn the_history_panel_tracks_the_command_count() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.update();
    assert_eq!(view.history_rows(), 1, "空历史只有「当前」一行");
    paint_a_stroke(&mut view);
    view.update();
    assert_eq!(view.history_rows(), 2, "一笔之后多一行");
}

#[test]
fn dragging_the_sidebar_handle_resizes_the_sidebar() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let before = view.sidebar_width();
    let start = view.sidebar_handle_center().expect("sidebar handle");
    // The sidebar is on the right, so dragging the handle left grows it.
    let end = start - Vec2::new(40.0, 0.0);
    view.event(&InputEvent::PointerDown {
        position: start,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove { position: end });
    view.event(&InputEvent::PointerUp {
        position: end,
        button: PointerButton::Left,
    });
    view.layout(viewport());
    assert!(
        view.sidebar_width() > before,
        "{} should have grown from {before}",
        view.sidebar_width()
    );
}

#[test]
fn dragging_the_palette_handle_resizes_it() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());

    let before = view.palette_width();
    let start = view.palette_handle_center().expect("palette handle");
    let end = start + Vec2::new(24.0, 0.0);
    view.event(&InputEvent::PointerDown {
        position: start,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove { position: end });
    view.event(&InputEvent::PointerUp {
        position: end,
        button: PointerButton::Left,
    });
    view.layout(viewport());
    assert!(view.palette_width() > before, "调色盘面板应变宽");
}

#[test]
fn a_layer_panel_edit_is_undoable() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 走和面板按钮一样的路径：AppState::execute + LayerMetaCommand。
    {
        let mut state = view.state.borrow_mut();
        let id = state.document.active_layer().unwrap().id;
        let before = crate::document::LayerMetaCommand::capture(&state.document);
        state.document.set_layer_visible(id, false);
        let after = crate::document::LayerMetaCommand::capture(&state.document);
        state.execute(Box::new(crate::document::LayerMetaCommand::new(
            before,
            after,
            "显示/隐藏",
        )));
    }
    view.update();
    assert!(view.can_undo());
    assert!(view.undo());
    let visible = view.state.borrow().document.layers[0].visible;
    assert!(visible, "撤销后图层恢复可见");
}

#[test]
fn adding_a_layer_refreshes_the_list_and_recomposites() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.state.borrow_mut().document.add_layer("新图层");
    view.update();
    view.layout(viewport());

    assert_eq!(view.layer_count.get(), 2);
    assert!(
        frame_texts(&view)
            .iter()
            .any(|text| text.contains("新图层")),
        "图层列表应出现新图层"
    );
    assert!(view.take_texture_upload().is_some(), "改动后要重传纹理");
}
