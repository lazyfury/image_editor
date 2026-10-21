//! 撤销 / 重做：一笔一步、局部合成只回退笔迹区域、空栈是空操作、工具栏按钮。

use super::*;

#[test]
fn a_brush_stroke_is_exactly_one_undo_step() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    paint_a_stroke(&mut view);

    assert!(view.can_undo());
    assert_eq!(view.history_len(), (1, 0));
    assert!(view.undo());
    assert_eq!(view.history_len(), (0, 1));
    assert!(view.can_redo());
    assert!(view.redo());
    assert_eq!(view.history_len(), (1, 0));
}

#[test]
fn undo_restores_the_stroke_and_redo_reapplies_it_through_the_view() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    paint_a_stroke(&mut view);
    view.mark_texture_dirty();
    let painted = view.take_texture_upload().expect("脏文档应产出合成结果");
    assert_eq!(painted.get_pixel(64, 64), Color::BLACK);

    view.undo();
    let undone = view.take_texture_upload().expect("撤销后应重合成");
    assert_eq!(undone.get_pixel(64, 64), Color::WHITE);

    view.redo();
    let redone = view.take_texture_upload().expect("重做后应重合成");
    assert_eq!(redone.get_pixel(64, 64), Color::BLACK);
}

/// 画笔的局部合成必须和整图重合成逐像素一致，而且撤销区域贴着笔迹。
#[test]
fn a_brush_stroke_is_composited_locally_and_undoes_only_its_region() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    paint_a_stroke(&mut view);

    let local = view.take_texture_upload().expect("笔迹应产出合成结果");
    let region = view.undo_region().expect("应记录撤销区域");
    assert!(
        region.area() < 64 * 64,
        "撤销区域应贴着笔迹，实际 {region:?}"
    );

    // 整图重合成一次，和局部结果逐像素比对。
    view.mark_texture_dirty();
    let full = view.take_texture_upload().expect("整图重合成");
    assert_eq!(local, full, "局部合成应与整图一致");
}

#[test]
fn undo_and_redo_with_empty_history_are_no_ops() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    assert!(!view.can_undo());
    assert!(!view.undo());
    view.update();
    assert!(label_text(&view, view.message_label).contains("没有可撤销"));
    assert!(!view.redo());
    view.update();
    assert!(label_text(&view, view.message_label).contains("没有可重做"));
}

#[test]
fn the_history_toolbar_buttons_undo_and_redo_a_stroke() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    paint_a_stroke(&mut view);
    assert_eq!(view.history_len(), (1, 0));

    let undo = view.history_center(HistoryAction::Undo).expect("撤销按钮");
    click(&mut view, undo);
    view.update();
    assert_eq!(view.history_len(), (0, 1));
    assert!(label_text(&view, view.message_label).contains("撤销"));

    let redo = view.history_center(HistoryAction::Redo).expect("重做按钮");
    click(&mut view, redo);
    view.update();
    assert_eq!(view.history_len(), (1, 0));
}
