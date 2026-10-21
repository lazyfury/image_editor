//! 系统剪贴板：复制 / 剪切 / 粘贴（含无剪贴板实现的降级）。

use crate::document::Color;
use crate::selfcheck::*;
use crate::ui::EditorView;
use igui_core::{InputEvent, Key, PointerButton, Vec2};

/// 选区剪贴板：复制 → 删除 → 粘贴，像素去而复返且各算一步 undo。
pub(crate) fn check_clipboard(failures: &mut Vec<String>) {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 用矩形填充画一块黑，再框选它。
    let camera = view.canvas_camera();
    view.event(&InputEvent::KeyDown {
        key: Key::Character('r'),
    });
    view.update();
    let (from, to) = (
        camera.document_to_screen(Vec2::new(30.0, 30.0)),
        camera.document_to_screen(Vec2::new(70.0, 70.0)),
    );
    for event in [
        InputEvent::PointerDown {
            position: from,
            button: PointerButton::Left,
        },
        InputEvent::PointerMove { position: to },
        InputEvent::PointerUp {
            position: to,
            button: PointerButton::Left,
        },
    ] {
        view.event(&event);
    }
    view.update();
    view.set_selection(Some(crate::canvas::Selection::rect(
        crate::document::PixelRegion::new(30, 30, 40, 40),
        view.document_size(),
    )));

    let pixel = |view: &EditorView| {
        view.document_snapshot()
            .active_layer()
            .map(|layer| layer.pixels.get_pixel(50, 50))
            .unwrap_or(Color::TRANSPARENT)
    };
    if pixel(&view) != Color::BLACK {
        failures.push("矩形填充没有画出黑块".to_string());
        return;
    }

    let before = view.history_len().0;
    if !view.copy_selection() {
        failures.push("复制失败".to_string());
    }
    if !view.has_clipboard() {
        failures.push("复制后剪贴板应有内容".to_string());
    }
    if !view.delete_selection() {
        failures.push("删除选区内容失败".to_string());
    } else if pixel(&view).a != 0 {
        failures.push("删除后选区内容应透明".to_string());
    }
    if view.history_len().0 != before + 1 {
        failures.push("删除应只加一步 undo".to_string());
    }
    if !view.paste_clipboard() {
        failures.push("粘贴失败".to_string());
    } else if pixel(&view) != Color::BLACK {
        failures.push(format!("粘贴没有复原像素：{:?}", pixel(&view)));
    }
    if view.history_len().0 != before + 2 {
        failures.push("粘贴应只加一步 undo".to_string());
    }

    // 粘贴为新图层：多一层、不动原图层、同样一步 undo。
    let layers = view.layer_count();
    if !view.paste_as_new_layer() {
        failures.push("粘贴为新图层失败".to_string());
    } else if view.layer_count() != layers + 1 {
        failures.push("粘贴为新图层应多一层".to_string());
    }
}
