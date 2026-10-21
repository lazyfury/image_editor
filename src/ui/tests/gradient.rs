//! 渐变：拖动填充、类型切换、端点跟随前景 / 背景 + 编辑器。

use super::*;

/// 渐变工具：拖一条轴把选区（整层）填成前景→背景的渐变，且能撤销。
#[test]
fn a_gradient_drag_fills_the_layer() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();
    view.event(&InputEvent::KeyDown {
        key: Key::Character('n'),
    });
    view.update();
    assert_eq!(view.active_tool(), ActiveTool::Gradient);

    let from = camera.document_to_screen(Vec2::new(0.5, 0.5));
    let to = camera.document_to_screen(Vec2::new(127.5, 0.5));
    view.event(&InputEvent::PointerDown {
        position: from,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove { position: to });
    view.event(&InputEvent::PointerUp {
        position: to,
        button: PointerButton::Left,
    });
    view.update();
    view.mark_texture_dirty();
    let frame = view.take_texture_upload().expect("渐变后应重合成");
    assert_eq!(frame.get_pixel(0, 0), Color::BLACK, "起点是前景色");
    assert_eq!(frame.get_pixel(127, 0), Color::WHITE, "终点是背景色");
    assert!(view.can_undo(), "渐变应入历史");
}

/// 渐变类型从选项栏 `+` 切换。
#[test]
fn the_gradient_kind_cycles_from_the_options_bar() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('n'),
    });
    view.update();
    view.layout(viewport());
    assert_eq!(view.gradient_kind(), GradientKind::Linear);
    let center = view
        .gradient_button_center(GradientAdjust::KindNext)
        .expect("渐变类型 + 按钮");
    click(&mut view, center);
    view.update();
    assert_eq!(view.gradient_kind(), GradientKind::Reflected);
}

/// 默认渐变端点跟随前景 / 背景；「编辑…」打开多节点编辑器。
#[test]
fn the_gradient_endpoints_follow_the_colors_and_the_editor_opens() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('n'),
    });
    view.update();
    view.layout(viewport());
    let stops = view.gradient_stops();
    assert_eq!(stops.first().map(|(_, c)| *c), Some(Color::BLACK));
    assert_eq!(stops.last().map(|(_, c)| *c), Some(Color::WHITE));

    let edit = view.gradient_edit_center().expect("编辑… 按钮");
    click(&mut view, edit);
    view.update();
    assert!(view.gradient_editor_open(), "点编辑应打开编辑器");
}
