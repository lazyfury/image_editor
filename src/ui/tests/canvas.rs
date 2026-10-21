//! 画布：相机（适配 / 滚轮缩放 / 中键平移 / 快捷键）、裁剪、棋盘格、像素网格、设备像素对齐。

use super::*;

/// 文档图像只在画布区域的裁剪里画一次，且在所有 UI 之前（不会盖住面板）。
#[test]
fn the_document_is_clipped_to_the_canvas_area_and_painted_once() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let commands = paint_commands(&view);
    let area = view.canvas_area_rect().expect("画布区域");

    let draws: Vec<usize> = commands
            .iter()
            .enumerate()
            .filter(|(_, command)| {
                matches!(command, DrawCommand::DrawImage { texture, .. } if *texture == DOCUMENT_TEXTURE)
            })
            .map(|(index, _)| index)
            .collect();
    assert_eq!(draws.len(), 1, "文档图像只该画一次：{draws:?}");

    let index = draws[0];
    assert!(
        matches!(commands.get(index - 1), Some(DrawCommand::ClipRect(rect)) if *rect == area),
        "文档图像应紧跟画布区域的 ClipRect：{:?}",
        commands.get(index - 1)
    );
    assert!(
        matches!(commands.get(index - 2), Some(DrawCommand::Save)),
        "裁剪块应以 Save 开头：{:?}",
        commands.get(index - 2)
    );
}

#[test]
fn transparent_document_pixels_show_the_checkerboard() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 默认文档带白色「背景」图层：棋盘格被盖住。
    view.mark_texture_dirty();
    let covered = view.take_texture_upload().expect("应产出合成结果");
    assert_eq!(covered.get_pixel(0, 0), Color::WHITE);

    // 隐藏背景图层后，透明区域应露出棋盘格。
    let background = view.state.borrow().document.layers[0].id;
    view.state
        .borrow_mut()
        .document
        .set_layer_visible(background, false);
    view.mark_texture_dirty();
    let revealed = view.take_texture_upload().expect("应产出合成结果");
    assert_eq!(revealed.get_pixel(0, 0), crate::canvas::color_at(0, 0));
    assert_eq!(revealed.get_pixel(8, 0), crate::canvas::color_at(8, 0));
}

#[test]
fn the_checkerboard_can_be_toggled_off() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 隐藏背景图层，让透明区域露出来。
    let background = view.state.borrow().document.layers[0].id;
    view.state
        .borrow_mut()
        .document
        .set_layer_visible(background, false);
    view.mark_texture_dirty();
    let shown = view.take_texture_upload().expect("应产出合成结果");
    assert_eq!(shown.get_pixel(0, 0), crate::canvas::color_at(0, 0));

    view.apply_menu_action(super::menu::MenuAction::ToggleCheckerboard);
    let hidden = view.take_texture_upload().expect("应产出合成结果");
    assert_eq!(hidden.get_pixel(0, 0), Color::TRANSPARENT, "关掉后保持透明");
}

#[test]
fn the_pixel_grid_shows_only_when_zoomed_in_and_in_pixel_mode() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let line_count = |view: &EditorView| {
        paint_commands(view)
            .iter()
            .filter(|command| matches!(command, DrawCommand::Line { .. }))
            .count()
    };

    // 像素模式关：放大也不画网格。
    view.options.pixel_mode.set(false);
    view.state.borrow_mut().canvas.zoom = 12.0;
    view.state.borrow_mut().canvas.offset = Vec2::new(120.0, 120.0);
    let without = line_count(&view);

    // 打开像素模式：多出网格线。
    view.options.pixel_mode.set(true);
    let with = line_count(&view);
    assert!(
        with > without,
        "放大 + 像素模式应画网格（{with} vs {without}）"
    );

    // 缩到阈值以下：回到没有网格。
    view.state.borrow_mut().canvas.zoom = 1.0;
    assert_eq!(line_count(&view), without, "缩小后不应画网格");
}

#[test]
fn the_first_layout_fits_and_centers_the_document() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let area = view.canvas_area_rect().expect("canvas area");
    let camera = view.canvas_camera();
    assert!(camera.zoom > 0.0);
    let center = camera.document_to_screen(Vec2::new(64.0, 64.0));
    assert!(
        (center.x - area.center().x).abs() < 0.5,
        "center = {center:?}"
    );
    assert!(
        (center.y - area.center().y).abs() < 0.5,
        "center = {center:?}"
    );
}

#[test]
fn the_wheel_zooms_around_the_pointer() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let point = view.canvas_area_rect().expect("canvas area").center();
    let before = view.canvas_camera();
    let anchor = before.screen_to_document(point);

    assert!(view
        .event(&InputEvent::Wheel {
            position: point,
            delta: Vec2::new(0.0, -40.0),
        })
        .is_handled());

    let after = view.canvas_camera();
    assert!(after.zoom > before.zoom, "向上滚应放大");
    assert!(
        (after.screen_to_document(point) - anchor).length_squared() < 0.25,
        "缩放应锚定在指针下的文档点"
    );
}

#[test]
fn middle_drag_pans_the_canvas() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let start = view.canvas_area_rect().expect("canvas area").center();
    let before = view.canvas_camera().offset;

    assert!(view
        .event(&InputEvent::PointerDown {
            position: start,
            button: PointerButton::Middle,
        })
        .is_handled());
    view.event(&InputEvent::PointerMove {
        position: start + Vec2::new(20.0, -10.0),
    });
    view.event(&InputEvent::PointerUp {
        position: start + Vec2::new(20.0, -10.0),
        button: PointerButton::Middle,
    });

    assert_eq!(view.canvas_camera().offset, before + Vec2::new(20.0, -10.0));
}

#[test]
fn the_zoom_shortcuts_change_the_camera() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let before = view.canvas_camera().zoom;
    assert!(view
        .event(&InputEvent::KeyDown {
            key: Key::Character('+'),
        })
        .is_handled());
    assert!(view.canvas_camera().zoom > before);
    assert!(view
        .event(&InputEvent::KeyDown {
            key: Key::Character('0'),
        })
        .is_handled());
    assert_eq!(view.canvas_camera().zoom, 1.0);
}

/// 最近邻采样时文档矩形对齐设备像素网格，避免 1px / 2px 交替的锯齿。
#[test]
fn the_document_rect_snaps_to_device_pixels_when_using_nearest() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.set_scale_factor(2.0);
    view.layout(viewport());
    // 默认相机把 128² 适配到画布区域（倍率 > 1）-> 最近邻。
    assert_eq!(
        view.image_smoothing(),
        crate::canvas::ImageSmoothing::Nearest
    );

    let rect_of = |view: &EditorView| {
        paint_commands(view)
            .into_iter()
            .find_map(|command| match command {
                DrawCommand::DrawImage {
                    texture,
                    destination,
                    ..
                } if texture == DOCUMENT_TEXTURE => Some(destination),
                _ => None,
            })
    };
    let snapped = rect_of(&view).expect("文档图像");
    // 设备像素 = 0.5 逻辑像素（scale 2），矩形四边都该落在网格上。
    for value in [
        snapped.left(),
        snapped.top(),
        snapped.right(),
        snapped.bottom(),
    ] {
        let grid = value * 2.0;
        assert!((grid - grid.round()).abs() < 1e-4, "{value} 没对齐设备像素");
    }

    // 缩小到 1 倍以下 -> 线性采样，矩形不再需要对齐。
    while view.canvas_camera().zoom >= 0.9 {
        view.apply_canvas_action(crate::ui::CanvasAction::ZoomOut);
    }
    assert_eq!(
        view.image_smoothing(),
        crate::canvas::ImageSmoothing::Linear
    );
}
