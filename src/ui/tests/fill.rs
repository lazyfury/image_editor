//! 矩形填充 / 自由填充（含拖动预览）。

use super::*;

/// 矩形填充：拖出的矩形被前景色填充，且能撤销。
#[test]
fn a_rectangle_fill_drag_fills_the_rect() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();
    view.event(&InputEvent::KeyDown {
        key: Key::Character('r'),
    });
    view.update();

    let from = camera.document_to_screen(Vec2::new(20.0, 20.0));
    let to = camera.document_to_screen(Vec2::new(60.0, 60.0));
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
    let frame = view.take_texture_upload().expect("填充后应重合成");
    assert_eq!(frame.get_pixel(40, 40), Color::BLACK, "矩形内被填充");
    assert_eq!(frame.get_pixel(10, 10), Color::WHITE, "矩形外不变");
    assert!(view.can_undo(), "填充应入历史");
}

/// 矩形填充拖动中会画矩形预览。
#[test]
fn the_rectangle_fill_preview_is_drawn_while_dragging() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();
    view.event(&InputEvent::KeyDown {
        key: Key::Character('r'),
    });
    view.update();
    view.event(&InputEvent::PointerDown {
        position: camera.document_to_screen(Vec2::new(20.0, 20.0)),
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove {
        position: camera.document_to_screen(Vec2::new(60.0, 60.0)),
    });

    let commands = paint_commands(&view);
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, DrawCommand::Line { .. })),
        "拖动中应有矩形预览"
    );
}

/// 自由填充：拖出的自由形状内部被前景色填充。
#[test]
fn a_free_fill_traces_and_fills_the_enclosed_area() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();
    view.event(&InputEvent::KeyDown {
        key: Key::Character('u'),
    });
    view.update();

    let path = [
        Vec2::new(20.0, 20.0),
        Vec2::new(60.0, 20.0),
        Vec2::new(60.0, 60.0),
        Vec2::new(20.0, 60.0),
        Vec2::new(20.0, 20.0),
    ];
    let screen = |point: Vec2| camera.document_to_screen(point);
    view.event(&InputEvent::PointerDown {
        position: screen(path[0]),
        button: PointerButton::Left,
    });
    for point in &path[1..] {
        view.event(&InputEvent::PointerMove {
            position: screen(*point),
        });
    }
    view.event(&InputEvent::PointerUp {
        position: screen(path[4]),
        button: PointerButton::Left,
    });
    view.update();
    view.mark_texture_dirty();
    let frame = view.take_texture_upload().expect("填充后应重合成");
    assert_eq!(frame.get_pixel(40, 40), Color::BLACK, "自由形状内部被填");
    assert_eq!(frame.get_pixel(80, 80), Color::WHITE, "形状外不变");
}
