//! 填充类工具：油漆桶与渐变。

use crate::app::state::ActiveTool;
use crate::document::Color;
use crate::selfcheck::*;
use crate::ui::EditorView;
use igui_core::{InputEvent, Key, Vec2};

/// 油漆桶：`G` 切换；选项栏容差 / 连续可用；点画布把连通区域填成前景色，且能撤销。
pub(crate) fn check_bucket(failures: &mut Vec<String>) {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('g'),
    });
    view.update();
    view.layout(viewport());
    if view.active_tool() != ActiveTool::PaintBucket {
        failures.push("G 没有切到油漆桶".to_string());
    }

    let before = view.bucket_tolerance();
    match view.bucket_button_center(crate::ui::BucketAdjust::ToleranceUp) {
        Some(center) => {
            click_at(&mut view, center);
            view.update();
        }
        None => failures.push("找不到油漆桶「容差 +」按钮".to_string()),
    }
    if view.bucket_tolerance() <= before {
        failures.push("油漆桶容差 + 没生效".to_string());
    }

    let before = view.bucket_contiguous();
    match view.bucket_toggle_center(crate::ui::BucketToggle::Contiguous) {
        Some(center) => {
            click_at(&mut view, center);
            view.update();
        }
        None => failures.push("找不到油漆桶「连续」开关".to_string()),
    }
    if view.bucket_contiguous() == before {
        failures.push("油漆桶连续开关没翻转".to_string());
    }

    let point = view
        .canvas_camera()
        .document_to_screen(Vec2::new(10.0, 10.0));
    click_at(&mut view, point);
    view.update();
    view.mark_texture_dirty();
    let frame = view.take_texture_upload().expect("填充后应重合成");
    if frame.get_pixel(10, 10) != Color::BLACK {
        failures.push("油漆桶没有把画布填成前景色".to_string());
    }
    view.undo();
    view.update();
    view.mark_texture_dirty();
    let undone = view.take_texture_upload().expect("撤销后应重合成");
    if undone.get_pixel(10, 10) != Color::WHITE {
        failures.push("油漆桶撤销没有还原".to_string());
    }
}

/// 渐变工具：`N` 切换；选项栏切类型 / 打开多节点编辑器；端点默认来自前景背景。
pub(crate) fn check_gradient(failures: &mut Vec<String>) {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('n'),
    });
    view.update();
    view.layout(viewport());
    if view.active_tool() != ActiveTool::Gradient {
        failures.push("N 没有切到渐变工具".to_string());
    }

    let before = view.gradient_kind();
    match view.gradient_button_center(crate::ui::GradientAdjust::KindNext) {
        Some(center) => {
            click_at(&mut view, center);
            view.update();
        }
        None => failures.push("找不到渐变「类型 +」按钮".to_string()),
    }
    if view.gradient_kind() == before {
        failures.push("渐变类型 + 没生效".to_string());
    }
    if view.gradient_stops().len() < 2 {
        failures.push("默认渐变应有两个端点".to_string());
    }

    match view.gradient_edit_center() {
        Some(center) => {
            click_at(&mut view, center);
            view.update();
        }
        None => failures.push("找不到渐变「编辑…」按钮".to_string()),
    }
    if !view.gradient_editor_open() {
        failures.push("点编辑没有打开多节点编辑器".to_string());
    }
    view.event(&InputEvent::KeyDown { key: Key::Escape });
    view.update();
}
