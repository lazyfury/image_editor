//! 编辑过程记录（延时动画的口子）：开录制 -> 画一笔 -> 落盘 -> 读回来。

use crate::document::Color;
use crate::selfcheck::*;
use crate::ui::EditorView;
use igui_core::{InputEvent, PointerButton, Vec2};

/// 开录制后画一笔：内存里应多出一步、补丁记的是**改后**像素；写盘再读回来
/// 必须逐字段一致（`.igtl` 的读写是一对，坏了回放就是错的）。
pub(crate) fn check_timeline(failures: &mut Vec<String>) {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.set_record_timeline(true);
    if view.timeline_len() != Some(0) {
        failures.push("刚开录制时不该有步骤".to_string());
    }

    // 画一笔（屏幕 -> 文档坐标由视图换算）。
    let point = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));
    view.event(&InputEvent::PointerDown {
        position: point,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove {
        position: point + Vec2::new(20.0, 0.0),
    });
    view.event(&InputEvent::PointerUp {
        position: point + Vec2::new(20.0, 0.0),
        button: PointerButton::Left,
    });
    view.update();

    let Some(steps) = view.timeline() else {
        failures.push("开录制后画一笔应记下一步".to_string());
        return;
    };
    if steps.len() != 1 {
        failures.push(format!("一笔应记 1 步，实际 {}", steps.len()));
    }
    let patch = steps[0].patch.as_ref();
    match patch {
        Some(patch) => {
            if patch.region.area() == 0 {
                failures.push("笔触补丁的区域是空的".to_string());
            }
            if !patch.pixels.iter().all(|color| *color == Color::BLACK) {
                failures.push("补丁里应只有笔刷的黑色（记的是改后像素）".to_string());
            }
        }
        None => failures.push("笔触这一步应有像素补丁".to_string()),
    }

    // 落盘再读回来。
    let path = temp_png("timeline").with_extension(crate::io::TIMELINE_EXTENSION);
    match crate::io::write_timeline(&path, &steps) {
        Ok(()) => {}
        Err(error) => {
            failures.push(format!("时间线写盘失败：{error}"));
            return;
        }
    }
    match crate::io::read_timeline(&path) {
        Ok(back) if back == steps => {}
        Ok(back) => failures.push(format!(
            "时间线读回来不一致：{} 步 vs {} 步",
            back.len(),
            steps.len()
        )),
        Err(error) => failures.push(format!("时间线读盘失败：{error}")),
    }
    let _ = std::fs::remove_file(&path);

    // 关掉录制后不再累加。
    view.set_record_timeline(false);
    if view.timeline_len().is_some() {
        failures.push("关掉录制后不该再有时间线".to_string());
    }
}
