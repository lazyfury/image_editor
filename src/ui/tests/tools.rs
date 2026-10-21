//! 移动 / 吸管。

use super::*;

#[test]
fn the_move_tool_offsets_the_active_layer() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('v'),
    });
    view.update();

    let camera = view.canvas_camera();
    let start = camera.document_to_screen(Vec2::new(64.5, 64.5));
    let end = start + Vec2::new(12.0, -8.0);
    let a = camera.screen_to_document(start);
    let b = camera.screen_to_document(end);
    let expected = Point::new((b.x - a.x).round() as i32, (b.y - a.y).round() as i32);

    view.event(&InputEvent::PointerDown {
        position: start,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove { position: end });
    view.event(&InputEvent::PointerUp {
        position: end,
        button: PointerButton::Left,
    });

    let position = view
        .state
        .borrow()
        .document
        .active_layer()
        .unwrap()
        .position;
    assert_eq!(position, expected);
    assert!(view.take_texture_upload().is_some(), "移动后要重合成");
}

#[test]
fn painting_after_moving_the_layer_lands_under_the_cursor() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 移动工具把当前图层右移 10 个文档像素。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('v'),
    });
    view.update();
    let camera = view.canvas_camera();
    let start = camera.document_to_screen(Vec2::new(64.5, 64.5));
    let end = start + Vec2::new(10.0 * camera.zoom, 0.0);
    view.event(&InputEvent::PointerDown {
        position: start,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove { position: end });
    view.event(&InputEvent::PointerUp {
        position: end,
        button: PointerButton::Left,
    });
    view.update();
    assert_eq!(view.active_layer_position(), Some(Point::new(10, 0)));

    // 切画笔，在文档 (64, 64) 落笔：应落在光标下，而不是被位移顶到 (74, 64)。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    let point = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));
    view.event(&InputEvent::PointerDown {
        position: point,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: point,
        button: PointerButton::Left,
    });
    view.update();

    let state = view.state.borrow();
    let layer = state.document.active_layer().unwrap();
    assert_eq!(layer.position, Point::ZERO, "落笔前把位移烘进像素");
    assert_eq!(layer.pixels.get_pixel(64, 64), Color::BLACK);
    // 合成后确实在光标下的文档坐标 (64, 64)，而不是被位移顶到 (74, 64)。
    assert_eq!(sample_pixel(&state.document, 64, 64), Color::BLACK);
    assert_eq!(sample_pixel(&state.document, 74, 64), Color::WHITE);
}

#[test]
fn the_eyedropper_picks_the_composited_color_as_the_foreground() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    paint_a_stroke(&mut view);

    view.event(&InputEvent::KeyDown {
        key: Key::Character('i'),
    });
    view.update();
    let point = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));
    view.event(&InputEvent::PointerDown {
        position: point,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: point,
        button: PointerButton::Left,
    });

    assert_eq!(view.foreground(), Color::BLACK);
    view.update();
    assert!(label_text(&view, view.message_label).contains("吸管"));
}
