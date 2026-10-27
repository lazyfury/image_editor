//! 视图旋转：0° 走轴对齐快路径，转过以后走 `SetTransform`；转完再点还得落到
//! 正确的文档像素（绘制与命中测试共用同一个相机）。
//!
//! 这些都是读命令 / 读像素的断言，不看画面。

use crate::app::state::{ActiveTool, AppState};
use crate::document::Color;
use crate::selfcheck::*;
use crate::ui::EditorView;
use igui_core::{InputEvent, Key, Transform2D, Vec2};
use igui_render::DrawCommand;

/// 一帧里的相机变换（`SetTransform`）。
fn transforms(frame: &igui_backend_recording::RecordedFrame) -> Vec<Transform2D> {
    frame
        .commands()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::SetTransform(transform) => Some(*transform),
            _ => None,
        })
        .collect()
}

pub(crate) fn check_rotation(failures: &mut Vec<String>) {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    click_tool(&mut view, ActiveTool::Brush);

    // 0°：轴对齐快路径，文档图像不发变换。
    let (mut view, upright) = record(view);
    if transforms(&upright)
        .iter()
        .any(|transform| !transform.is_identity())
    {
        failures.push("0° 时文档图像不该走 SetTransform（轴对齐快路径）".to_string());
    }

    // 两次 `T` = 30°：应发出正好等于相机矩阵的那条变换。
    for _ in 0..2 {
        view.event(&InputEvent::KeyDown {
            key: Key::Character('t'),
        });
    }
    let camera = view.canvas_camera();
    let expected = camera.to_transform();
    let (mut view, rotated) = record(view);
    if !transforms(&rotated).contains(&expected) {
        failures.push(format!("30° 时应发出相机矩阵的 SetTransform：{expected:?}"));
    }

    // 旋转后点文档中心：命中的必须还是 (64, 64) 这个像素。
    let point = camera.document_to_screen(Vec2::new(64.5, 64.5));
    click_at(&mut view, point);
    view.mark_texture_dirty();
    match view.take_texture_upload() {
        Some(composite) if composite.get_pixel(64, 64) == Color::BLACK => {}
        Some(composite) => failures.push(format!(
            "30° 下点击没有命中 (64, 64)：{:?}",
            composite.get_pixel(64, 64)
        )),
        None => failures.push("旋转后点击没有产出合成结果".to_string()),
    }

    // `0` 归零（重置视图），角度必须真的回到 0。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('0'),
    });
    if view.canvas_camera().angle != 0.0 {
        failures.push("`0` 没有把视图角度归零".to_string());
    }
}
