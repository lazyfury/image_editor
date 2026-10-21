//! 画笔：光标圈、平滑、对称、预设、像素模式边缘。

use crate::document::Color;
use crate::selfcheck::*;
use crate::ui::EditorView;
use igui_core::{InputEvent, Key, Vec2};

/// 笔刷光标圈：画笔在画布上时出现、指针离开画布就消失。
pub(crate) fn check_brush_cursor(failures: &mut Vec<String>) {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 文档中心在屏幕上的位置：一定落在画布区域里。
    let pointer = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));
    let has_ring = |frame: &RecordedFrame| {
        frame.commands().iter().any(|command| {
            matches!(command, DrawCommand::StrokeRect { rect, .. } if rect.center() == pointer)
        })
    };

    view.event(&InputEvent::PointerMove { position: pointer });
    view.layout(viewport());
    if !has_ring(&paint_frame(&view)) {
        failures.push("画笔在画布上应显示笔刷光标圈".to_string());
    }

    let off_canvas = Vec2::new(pointer.x, 4.0);
    view.event(&InputEvent::PointerMove {
        position: off_canvas,
    });
    view.layout(viewport());
    if has_ring(&paint_frame(&view)) {
        failures.push("指针离开画布后不该再有光标圈".to_string());
    }
}

/// 选项栏的「平滑 −」：100% -> 80%（0% 是折线，像素画的「笔到手到」）。
pub(crate) fn check_brush_smooth(failures: &mut Vec<String>) {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    view.layout(viewport());

    for _ in 0..2 {
        match view.brush_adjust_center(crate::ui::BrushAdjust::SmoothDown) {
            Some(center) => {
                click_at(&mut view, center);
                view.update();
            }
            None => {
                failures.push("找不到选项栏的「平滑 −」按钮".to_string());
                return;
            }
        }
    }
    // 间距 − 两次：25% -> 21%（纹理笔靠它调连续）。
    for _ in 0..2 {
        if let Some(center) = view.brush_adjust_center(crate::ui::BrushAdjust::SpacingDown) {
            click_at(&mut view, center);
            view.update();
        }
    }
    if (view.brush_spacing() - 0.21).abs() > 1e-4 {
        failures.push(format!(
            "选项栏「间距 −」没生效：{:.0}%",
            view.brush_spacing() * 100.0
        ));
    }

    if (view.brush_smooth() - 0.8).abs() > 1e-4 {
        failures.push(format!(
            "「平滑 −」没生效：{:.0}%",
            view.brush_smooth() * 100.0
        ));
    }
}

/// 对称绘画：选项栏「切换」循环到垂直，画一笔两侧都要有像素。
pub(crate) fn check_symmetry(failures: &mut Vec<String>) {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    match view.symmetry_button_center() {
        Some(center) => {
            click_at(&mut view, center);
            view.update();
        }
        None => {
            failures.push("找不到对称「切换」按钮".to_string());
            return;
        }
    }
    if view.symmetry_mode() != crate::tools::Symmetry::Vertical {
        failures.push(format!(
            "对称应切到垂直，实为 {}",
            view.symmetry_mode().label()
        ));
    }

    let point = view
        .canvas_camera()
        .document_to_screen(Vec2::new(20.0, 64.0));
    click_at(&mut view, point);
    view.update();
    let snapshot = view.document_snapshot();
    let mirrored = snapshot
        .active_layer()
        .map(|layer| layer.pixels.get_pixel(127 - 20, 64))
        .unwrap_or(Color::TRANSPARENT);
    if mirrored != Color::BLACK {
        failures.push(format!("垂直对称的镜像位置没有落笔：{mirrored:?}"));
    }
}

/// 笔刷预设 + 硬度：下拉里能点、选中后参数 / 开关 / 标签都跟着变，硬度 − 生效。
pub(crate) fn check_brush_presets(failures: &mut Vec<String>) {
    use crate::tools::brush::preset;

    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    view.layout(viewport());

    if view.brush_preset_label() != preset::PRESETS[0].name {
        failures.push(format!("默认预设标签不对：{}", view.brush_preset_label()));
    }

    // 点「预设」触发按钮应打开下拉，帧里能看到所有预设名。
    match view.brush_preset_center() {
        Some(center) => {
            click_at(&mut view, center);
            view.update();
            // 覆盖层的内容在 layout 里才建出来（菜单栏下拉也是这个套路）。
            view.layout(viewport());
            let frame = paint_frame(&view);
            for item in preset::PRESETS {
                if !contains(&frame, item.name.as_ref()) {
                    failures.push(format!("预设下拉缺少「{}」", item.name));
                    break;
                }
            }
            let target = &preset::PRESETS[3];
            match text_center(&frame, target.name.as_ref()) {
                Some(item_center) => {
                    click_at(&mut view, item_center);
                    view.update();
                }
                None => failures.push(format!("预设下拉里点不到「{}」", target.name)),
            }
        }
        None => failures.push("找不到选项栏的「预设」按钮".to_string()),
    }

    let target = &preset::PRESETS[3];
    if view.brush_size() != target.size
        || (view.brush_hardness() - target.hardness).abs() > 1e-4
        || (view.brush_opacity() - target.opacity).abs() > 1e-4
    {
        failures.push(format!(
            "选预设没改到画笔：{}px / 硬度 {:.0}% / 不透明度 {:.0}%",
            view.brush_size(),
            view.brush_hardness() * 100.0,
            view.brush_opacity() * 100.0
        ));
    }
    if view.pixel_mode() {
        failures.push("柔边预设应关掉像素模式".to_string());
    }
    if view.brush_shape() != crate::tools::BrushShape::Round {
        failures.push("柔边预设应是圆头".to_string());
    }
    if view.brush_preset_label() != target.name {
        failures.push(format!(
            "选预设后标签应是「{}」，实际「{}」",
            target.name,
            view.brush_preset_label()
        ));
    }

    // 硬度 −：35% -> 30%。
    match view.brush_adjust_center(crate::ui::BrushAdjust::HardnessDown) {
        Some(center) => {
            click_at(&mut view, center);
            view.update();
        }
        None => failures.push("找不到选项栏的「硬度 −」按钮".to_string()),
    }
    if (view.brush_hardness() - target.hardness + 0.05).abs() > 1e-4 {
        failures.push(format!(
            "选项栏「硬度 −」没生效：{:.0}%",
            view.brush_hardness() * 100.0
        ));
    }
}

/// 在一个新视图上验证像素模式：size 3 硬边无灰边，关掉后软边（圆头）有灰边。
pub(crate) fn check_pixel_mode_edges(failures: &mut Vec<String>) {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    view.layout(viewport());

    if !view.pixel_mode() {
        failures.push("像素模式默认应为开".to_string());
    }
    while view.brush_size() < 3.0 {
        match view.brush_adjust_center(crate::ui::BrushAdjust::SizeUp) {
            Some(center) => {
                click_at(&mut view, center);
                view.update();
            }
            None => break,
        }
    }

    // 硬边：size 3 不该有抗锯齿灰边。
    let hard = view
        .canvas_camera()
        .document_to_screen(Vec2::new(40.0, 40.0));
    click_at(&mut view, hard);
    view.update();
    view.mark_texture_dirty();
    let frame = view.take_texture_upload().expect("应产出合成结果");
    let gray = gray_pixels(&frame, 40, 40, 3);
    if gray != 0 {
        failures.push(format!("像素模式仍有 {gray} 个抗锯齿灰边像素"));
    }

    // 软边：关掉像素模式，并用圆头（方形的软边在整数尺寸上会饱和成实心）。
    click_toggle(&mut view, crate::ui::BrushToggle::Hard);
    click_toggle(&mut view, crate::ui::BrushToggle::Square);
    if view.pixel_mode() || view.square_mode() {
        failures.push("选项栏开关没有关掉".to_string());
    }
    if view.brush_hard() || view.brush_shape() != crate::tools::BrushShape::Round {
        failures.push("画笔没有同步成软边圆头".to_string());
    }
    let soft = view
        .canvas_camera()
        .document_to_screen(Vec2::new(40.0, 50.0));
    click_at(&mut view, soft);
    view.update();
    view.mark_texture_dirty();
    let frame = view.take_texture_upload().expect("应产出合成结果");
    if gray_pixels(&frame, 40, 50, 3) == 0 {
        failures.push("关掉像素模式后应出现抗锯齿灰边".to_string());
    }
}
