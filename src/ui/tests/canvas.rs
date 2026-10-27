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
            phase: igui_core::TouchPhase::Moved,
            device: igui_core::ScrollDevice::Wheel,
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

// -- 视图旋转 ------------------------------------------------------------

/// 变换路径把文档矩形放在**局部坐标**里，缩放 / 旋转 / 平移交给 `SetTransform`。
#[test]
fn a_rotated_view_draws_the_document_through_one_transform() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.state.borrow_mut().canvas.angle = 30f32.to_radians();
    let camera = view.canvas_camera();

    let commands = paint_commands(&view);
    let transforms: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            DrawCommand::SetTransform(transform) => Some(*transform),
            _ => None,
        })
        .collect();
    assert!(
        transforms.contains(&camera.to_transform()),
        "旋转视图应发出一条等于相机矩阵的 SetTransform：{transforms:?}"
    );

    let destination = commands
        .iter()
        .find_map(|command| match command {
            DrawCommand::DrawImage {
                texture,
                destination,
                ..
            } if *texture == DOCUMENT_TEXTURE => Some(*destination),
            _ => None,
        })
        .expect("文档图像");
    assert_eq!(
        destination,
        igui_core::Rect::from_min_size(Vec2::ZERO, Size::new(128.0, 128.0)),
        "旋转时文档图像应画在局部坐标里，由变换负责摆位"
    );
}

/// 0° 时保持旧路径：轴对齐矩形 + 设备像素对齐，不发变换。
#[test]
fn an_upright_view_keeps_the_axis_aligned_path() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let transforms = paint_commands(&view)
        .into_iter()
        .filter(|command| matches!(command, DrawCommand::SetTransform(t) if !t.is_identity()))
        .count();
    assert_eq!(transforms, 0, "正着看不该有多余变换");
}

/// 旋转后像素网格跟着转：每根线的两端都在文档空间的同一条整数坐标上。
#[test]
fn the_pixel_grid_follows_the_rotation() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.options.pixel_mode.set(true);
    {
        let mut state = view.state.borrow_mut();
        state.canvas.zoom = 12.0;
        state.canvas.offset = Vec2::new(120.0, 120.0);
    }
    // 只取画布区域里的线：面板分隔条 / 边框也是 `Line`。
    let lines = |view: &EditorView| {
        let area = view.canvas_area_rect().expect("canvas area");
        paint_commands(view)
            .into_iter()
            .filter_map(|command| match command {
                DrawCommand::Line { from, to, .. } if area.contains((from + to) * 0.5) => {
                    Some((from, to))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };

    let upright = lines(&view);
    assert!(!upright.is_empty(), "放大 + 像素模式应画网格");

    view.state.borrow_mut().canvas.angle = 30f32.to_radians();
    let camera = view.canvas_camera();
    let rotated = lines(&view);
    assert!(!rotated.is_empty(), "旋转后网格不该消失");
    assert!(
        rotated.len() < 600,
        "旋转后网格线数应有界：{}",
        rotated.len()
    );
    for (from, to) in rotated {
        let a = camera.screen_to_document(from);
        let b = camera.screen_to_document(to);
        // 竖直族：x 相同且是整数；水平族：y 相同且是整数。
        let vertical = (a.x - b.x).abs() < 1e-2;
        let horizontal = (a.y - b.y).abs() < 1e-2;
        assert!(
            vertical || horizontal,
            "网格线既不竖直也不水平：{a:?} {b:?}"
        );
        if vertical {
            assert!((a.x - a.x.round()).abs() < 1e-2, "x = {}", a.x);
        } else {
            assert!((a.y - a.y.round()).abs() < 1e-2, "y = {}", a.y);
        }
    }
}

/// 旋转后点击仍然落在正确的文档像素上（命中测试和绘制用同一个相机）。
#[test]
fn painting_through_a_rotated_camera_hits_the_expected_pixel() {
    let mut view = super::brush_view();
    view.state.borrow_mut().canvas.angle = 30f32.to_radians();
    let point = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));
    super::click_at(&mut view, point);

    view.mark_texture_dirty();
    let composite = view.take_texture_upload().expect("脏文档应产出合成结果");
    assert_eq!(composite.get_pixel(64, 64), Color::BLACK);
    assert_eq!(composite.get_pixel(65, 64), Color::WHITE);
}

/// `T` / `Shift+T` 每次转 15°，`0` 归零（和 Photoshop 的 Reset View 一致）。
#[test]
fn the_rotation_shortcuts_step_and_reset_the_view() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    for (key, expected) in [('t', 15.0f32), ('t', 30.0), ('T', 15.0)] {
        view.event(&InputEvent::KeyDown {
            key: Key::Character(key),
        });
        let angle = view.canvas_camera().angle.to_degrees();
        assert!((angle - expected).abs() < 1e-3, "{key} -> {angle}°");
    }
    view.event(&InputEvent::KeyDown {
        key: Key::Character('0'),
    });
    assert_eq!(view.canvas_camera().angle, 0.0, "0 应把角度归零");
}

// -- 触控板导航（设备区分 / 捏合 / 旋转手势） -----------------------------

/// 触控板滚动 = 平移：内容跟手，缩放不变；带刻度的滚轮仍然缩放。
#[test]
fn a_trackpad_scroll_pans_while_a_wheel_still_zooms() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let point = view.canvas_area_rect().expect("canvas area").center();
    let before = view.canvas_camera();

    // igui 约定 `y > 0` 是「向下滚」；触控板向上滑 = 内容向上移，偏移减小。
    assert!(view
        .event(&InputEvent::Wheel {
            position: point,
            delta: Vec2::new(0.0, 40.0),
            phase: igui_core::TouchPhase::Moved,
            device: igui_core::ScrollDevice::Trackpad,
        })
        .is_handled());
    let panned = view.canvas_camera();
    assert_eq!(panned.zoom, before.zoom, "触控板滚动不该缩放");
    assert!(
        (panned.offset.y - (before.offset.y - 40.0)).abs() < 1e-3,
        "内容应跟着手指向上移 40px：{} -> {}",
        before.offset.y,
        panned.offset.y
    );
    assert!((panned.offset.x - before.offset.x).abs() < 1e-3);

    // 带刻度的滚轮：还是缩放（向上滚放大）。
    assert!(view
        .event(&InputEvent::Wheel {
            position: point,
            delta: Vec2::new(0.0, -48.0),
            phase: igui_core::TouchPhase::Moved,
            device: igui_core::ScrollDevice::Wheel,
        })
        .is_handled());
    assert!(view.canvas_camera().zoom > panned.zoom, "滚轮应该仍然缩放");
}

/// `Shift` + 触控板滚动：只取主方向，换轴成水平平移。
#[test]
fn shift_scroll_swaps_the_axis() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let point = view.canvas_area_rect().expect("canvas area").center();
    view.set_modifiers(igui_core::Modifiers::SHIFT);
    let before = view.canvas_camera();

    view.event(&InputEvent::Wheel {
        position: point,
        delta: Vec2::new(30.0, 40.0),
        phase: igui_core::TouchPhase::Moved,
        device: igui_core::ScrollDevice::Trackpad,
    });
    let after = view.canvas_camera();
    assert!((after.offset.x - (before.offset.x - 40.0)).abs() < 1e-3);
    assert!((after.offset.y - before.offset.y).abs() < 1e-3);
}

/// 画布外的滚动不碰相机（面板里的列表照常滚）。
#[test]
fn a_scroll_outside_the_canvas_leaves_the_camera_alone() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();
    // 工具栏那一列。
    let outside = Vec2::new(10.0, 400.0);
    view.event(&InputEvent::Wheel {
        position: outside,
        delta: Vec2::new(0.0, 40.0),
        phase: igui_core::TouchPhase::Moved,
        device: igui_core::ScrollDevice::Trackpad,
    });
    assert_eq!(view.canvas_camera(), camera);
}

/// 捏合以指针为锚缩放；单个事件的因子夹在 `[0.5, 2.0]`。
#[test]
fn a_pinch_zooms_around_the_pointer_and_clamps_one_event() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let anchor = view.canvas_area_rect().expect("canvas area").center();
    let before = view.canvas_camera();
    let document = before.screen_to_document(anchor);

    assert!(view
        .event(&InputEvent::Pinch {
            position: anchor,
            factor: 1.25,
            phase: igui_core::TouchPhase::Moved,
        })
        .is_handled());
    let zoomed = view.canvas_camera();
    assert!((zoomed.zoom - before.zoom * 1.25).abs() < 1e-3);
    assert!((zoomed.screen_to_document(anchor) - document).length() < 1e-3);

    // 一个离谱的事件只能走 2 倍，不会一步顶到 64×。
    view.event(&InputEvent::Pinch {
        position: anchor,
        factor: 1e9,
        phase: igui_core::TouchPhase::Moved,
    });
    assert!((view.canvas_camera().zoom - zoomed.zoom * 2.0).abs() < 1e-3);
}

/// 指针在画布外捏合：锚到画布中心，而不是屏幕外的某一点。
#[test]
fn a_pinch_outside_the_canvas_anchors_on_the_canvas_center() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let center = view.canvas_area_rect().expect("canvas area").center();
    let before = view.canvas_camera();
    let document = before.screen_to_document(center);

    view.event(&InputEvent::Pinch {
        position: Vec2::new(10.0, 400.0),
        factor: 1.5,
        phase: igui_core::TouchPhase::Moved,
    });
    let after = view.canvas_camera();
    assert!(after.zoom > before.zoom);
    assert!((after.screen_to_document(center) - document).length() < 1e-3);
}

/// 旋转手势按 15° 吸附；键盘 `T` 是确定步进（不吸附）。
#[test]
fn the_rotation_gesture_snaps_but_the_key_steps_exactly() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());

    // 14.5° 落在 15° 的吸附半径里 -> 贴到 15°。
    view.event(&InputEvent::Rotate {
        position: Vec2::ZERO,
        delta: 14.5,
        phase: igui_core::TouchPhase::Moved,
    });
    assert!((view.canvas_camera().angle.to_degrees() - 15.0).abs() < 1e-3);

    // 再转 5° -> 20°，离 15 和 30 都远，不吸附。
    view.event(&InputEvent::Rotate {
        position: Vec2::ZERO,
        delta: 5.0,
        phase: igui_core::TouchPhase::Moved,
    });
    assert!((view.canvas_camera().angle.to_degrees() - 20.0).abs() < 1e-3);

    // 转到 89.5°：吸到直角。
    view.event(&InputEvent::Rotate {
        position: Vec2::ZERO,
        delta: 69.5,
        phase: igui_core::TouchPhase::Moved,
    });
    assert!((view.canvas_camera().angle.to_degrees() - 90.0).abs() < 1e-3);

    // 键盘：+15°，停在 105°。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('t'),
    });
    assert!((view.canvas_camera().angle.to_degrees() - 105.0).abs() < 1e-3);
}
