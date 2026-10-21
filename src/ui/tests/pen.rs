//! 钢笔：多击建路径、闭合、Esc / Backspace、路径 -> 选区 / 填充 / 描边。

use super::*;

/// 三个锚点围出的直角三角形（文档坐标）。
const PEN_TRIANGLE: [(f32, f32); 3] = [(20.0, 20.0), (100.0, 20.0), (100.0, 100.0)];

/// 切到钢笔并排好版的新视图（默认 128×128 文档）。
fn pen_view() -> EditorView {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('p'),
    });
    view.update();
    view.layout(viewport());
    view
}

/// 在文档坐标 `point` 处点一下画布（按下 + 松手，不拖动）。
fn click_document(view: &mut EditorView, point: Vec2) {
    let screen = view.canvas_camera().document_to_screen(point);
    for event in [
        InputEvent::PointerDown {
            position: screen,
            button: PointerButton::Left,
        },
        InputEvent::PointerUp {
            position: screen,
            button: PointerButton::Left,
        },
    ] {
        view.event(&event);
    }
    view.update();
}

fn click_pen_triangle(view: &mut EditorView) {
    for (x, y) in PEN_TRIANGLE {
        click_document(view, Vec2::new(x, y));
    }
}

#[test]
fn each_pen_click_adds_one_anchor() {
    let mut view = pen_view();
    click_pen_triangle(&mut view);
    assert_eq!(view.pen_anchor_count(), 3);
    assert!(!view.pen_path_closed(), "还没点回起点，路径是开放的");
    assert!(
        label_text(&view, view.options.pen_summary).contains("3 个锚点"),
        "选项栏应显示锚点数：{}",
        label_text(&view, view.options.pen_summary)
    );
}

#[test]
fn clicking_the_first_anchor_closes_the_pen_path() {
    let mut view = pen_view();
    click_pen_triangle(&mut view);
    // 起点附近（屏幕上 3px，命中半径是 8 屏幕像素，和缩放无关）：应闭合而不是
    // 多落一个锚点。
    let first = view
        .canvas_camera()
        .document_to_screen(Vec2::new(PEN_TRIANGLE[0].0, PEN_TRIANGLE[0].1));
    click_at(&mut view, first + Vec2::new(3.0, 2.0));
    assert_eq!(view.pen_anchor_count(), 3, "闭合不该再加锚点");
    assert!(view.pen_path_closed());
}

#[test]
fn escape_cancels_the_pen_path_and_backspace_drops_one_anchor() {
    let mut view = pen_view();
    click_pen_triangle(&mut view);
    view.event(&InputEvent::KeyDown {
        key: Key::Backspace,
    });
    view.update();
    assert_eq!(view.pen_anchor_count(), 2, "Backspace 只删最后一个锚点");
    view.event(&InputEvent::KeyDown { key: Key::Escape });
    view.update();
    assert_eq!(view.pen_anchor_count(), 0, "Esc 取消整条路径");
}

#[test]
fn the_pen_selection_button_turns_the_path_into_a_selection() {
    let mut view = pen_view();
    click_pen_triangle(&mut view);
    let center = view
        .pen_action_center(PenAction::Select)
        .expect("select button");
    click_at(&mut view, center);
    let selection = view.selection().expect("路径应产生选区");
    // 直角边 80 的三角形：≈ 3200 像素。
    let area = selection.area();
    assert!((2800..=3400).contains(&area), "面积 {area}");
}

#[test]
fn the_pen_fill_button_fills_the_path_in_one_undo_step() {
    let mut view = pen_view();
    click_pen_triangle(&mut view);
    let before = view.history_len().0;
    let center = view
        .pen_action_center(PenAction::Fill)
        .expect("fill button");
    click_at(&mut view, center);
    assert_eq!(view.history_len().0, before + 1, "填充 = 一步 undo");
    // 三角形内部（40, 40）被前景色填满，右下角外面还是白的。
    let (inside, outside) = {
        let state = view.state.borrow();
        let pixels = &state.document.active_layer().unwrap().pixels;
        (pixels.get_pixel(40, 40), pixels.get_pixel(120, 120))
    };
    assert_eq!(inside, Color::BLACK, "三角形内部应被前景色填满");
    assert_eq!(outside, Color::WHITE, "路径外不该动");
}

#[test]
fn the_pen_stroke_button_paints_along_the_path_in_one_undo_step() {
    let mut view = pen_view();
    click_pen_triangle(&mut view);
    let before = view.history_len().0;
    let center = view
        .pen_action_center(PenAction::Stroke)
        .expect("stroke button");
    click_at(&mut view, center);
    assert_eq!(view.history_len().0, before + 1, "描边 = 一步 undo");
}
