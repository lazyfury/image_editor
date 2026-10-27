//! 视图导航：抓手 / 缩放工具、空格临时抓手（与手势共用相机）。

use super::*;
use crate::app::state::ActiveTool;
use igui_core::{Key, Modifiers, PointerButton, Vec2};

fn key_down(view: &mut EditorView, key: Key) {
    view.event(&InputEvent::KeyDown { key });
    view.update();
}

fn drag(view: &mut EditorView, from: Vec2, delta: Vec2) {
    for event in [
        InputEvent::PointerDown {
            position: from,
            button: PointerButton::Left,
        },
        InputEvent::PointerMove {
            position: from + delta,
        },
        InputEvent::PointerUp {
            position: from + delta,
            button: PointerButton::Left,
        },
    ] {
        view.event(&event);
    }
    view.update();
}

fn canvas_center(view: &EditorView) -> Vec2 {
    view.canvas_area_rect().expect("canvas area").center()
}

/// 抓手工具只动相机，不产生像素 / 历史。
#[test]
fn the_hand_tool_pans_without_touching_the_document() {
    let mut view = brush_view();
    key_down(&mut view, Key::Character('h'));
    assert_eq!(view.state.borrow().active_tool, ActiveTool::Hand);

    let start = canvas_center(&view);
    let before = view.canvas_camera().offset;
    let revision = view.state.borrow().document.revision();
    let history = view.history_len();
    let delta = Vec2::new(36.0, -14.0);

    drag(&mut view, start, delta);

    assert!((view.canvas_camera().offset - before - delta).length() < 1e-3);
    assert_eq!(
        view.state.borrow().document.revision(),
        revision,
        "不改像素"
    );
    assert_eq!(view.history_len(), history, "不进撤销栈");
}

/// 空格是临时抓手：拖动时不落笔，松开后回到原工具，后续还能正常画。
#[test]
fn holding_space_pans_temporarily_and_restores_the_tool() {
    let mut view = brush_view();
    let start = canvas_center(&view);
    let before = view.canvas_camera().offset;
    let revision = view.state.borrow().document.revision();

    view.event(&InputEvent::KeyDown { key: Key::Space });
    assert!(view.space_held());
    drag(&mut view, start, Vec2::new(24.0, 10.0));
    assert!((view.canvas_camera().offset - before - Vec2::new(24.0, 10.0)).length() < 1e-3);
    assert_eq!(
        view.state.borrow().document.revision(),
        revision,
        "空间拖动不画"
    );
    assert_eq!(
        view.state.borrow().active_tool,
        ActiveTool::Brush,
        "工具不变"
    );

    view.event(&InputEvent::KeyUp { key: Key::Space });
    assert!(!view.space_held());
    assert_eq!(view.state.borrow().active_tool, ActiveTool::Brush);

    // 恢复后同样的一拖就应该落笔了。
    drag(&mut view, start, Vec2::new(20.0, 0.0));
    assert!(
        view.state.borrow().document.revision() > revision,
        "松开空格后画笔恢复"
    );
}

/// 缩放工具以指针为锚放大一档；Option 反向。
#[test]
fn the_zoom_tool_zooms_around_the_pointer_and_alt_reverses_it() {
    let mut view = brush_view();
    key_down(&mut view, Key::Character('z'));
    assert_eq!(view.state.borrow().active_tool, ActiveTool::Zoom);

    let anchor = canvas_center(&view) + Vec2::new(18.0, -12.0);
    let document_point = view.canvas_camera().screen_to_document(anchor);
    let before = view.canvas_camera().zoom;

    click_at(&mut view, anchor);
    assert!(
        (view.canvas_camera().zoom / before - 1.25).abs() < 1e-4,
        "放大 1.25 倍，实际 {}",
        view.canvas_camera().zoom / before
    );
    assert!(
        (view.canvas_camera().screen_to_document(anchor) - document_point).length() < 1e-3,
        "指针下的文档点不动"
    );

    let before = view.canvas_camera().zoom;
    view.set_modifiers(Modifiers {
        alt: true,
        ..Modifiers::NONE
    });
    click_at(&mut view, anchor);
    assert!(
        (view.canvas_camera().zoom / before - 0.8).abs() < 1e-4,
        "Option 点击缩小，实际 {}",
        view.canvas_camera().zoom / before
    );
}

/// 蒙版不让文档工具动，但视图工具照常平移。
#[test]
fn view_tools_bypass_the_mask_message() {
    let mut view = brush_view();
    view.state.borrow_mut().document.active_mask = true;
    key_down(&mut view, Key::Character('h'));
    let before = view.canvas_camera().offset;
    let center = canvas_center(&view);
    drag(&mut view, center, Vec2::new(-30.0, 8.0));
    assert!((view.canvas_camera().offset - before - Vec2::new(-30.0, 8.0)).length() < 1e-3);
    assert!(view.message.borrow().is_none(), "视图工具不弹蒙版提示");
}

/// 带 Cmd/Ctrl 的字母不触发工具切换（Cmd+Z 撤销、Cmd+H 隐藏窗口）。
#[test]
fn command_letters_do_not_switch_tools() {
    let mut view = brush_view();
    view.set_modifiers(Modifiers {
        meta: true,
        ..Modifiers::NONE
    });
    key_down(&mut view, Key::Character('h'));
    assert_eq!(view.state.borrow().active_tool, ActiveTool::Brush);

    view.set_modifiers(Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    });
    key_down(&mut view, Key::Character('z'));
    assert_eq!(view.state.borrow().active_tool, ActiveTool::Brush);

    view.set_modifiers(Modifiers::NONE);
    key_down(&mut view, Key::Character('h'));
    assert_eq!(view.state.borrow().active_tool, ActiveTool::Hand);
    key_down(&mut view, Key::Character('z'));
    assert_eq!(view.state.borrow().active_tool, ActiveTool::Zoom);
}
