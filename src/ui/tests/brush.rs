//! 画笔 / 橡皮：选项栏参数、像素模式、对称、光标圈、预设、落笔与擦除。

use super::*;

/// 点一下选项栏的 `−` / `+` 并 `update`。
fn click_adjust(view: &mut EditorView, adjust: BrushAdjust) {
    let center = view.brush_adjust_center(adjust).expect("adjust button");
    view.event(&InputEvent::PointerDown {
        position: center,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: center,
        button: PointerButton::Left,
    });
    view.update();
}

fn click_toggle(view: &mut EditorView, toggle: BrushToggle) {
    let center = view.brush_toggle_center(toggle).expect("toggle button");
    view.event(&InputEvent::PointerDown {
        position: center,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: center,
        button: PointerButton::Left,
    });
    view.update();
}

/// 指针停在 `position`（画布区域内的逻辑坐标）后画一帧。
fn frame_with_pointer(view: &mut EditorView, position: Vec2) -> Vec<DrawCommand> {
    view.event(&InputEvent::PointerMove { position });
    view.layout(viewport());
    paint_commands(view)
}

#[test]
fn the_options_bar_adjusts_the_brush_size() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // Select the brush so its config is shown in the options bar.
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    view.layout(viewport());
    let before = view.brush_size();
    let center = view
        .brush_adjust_center(BrushAdjust::SizeUp)
        .expect("size + button");
    view.event(&InputEvent::PointerDown {
        position: center,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: center,
        button: PointerButton::Left,
    });
    view.update();
    assert_eq!(view.brush_size(), before + 1.0);
}

#[test]
fn the_pixel_mode_toggles_flip_the_brush() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    view.layout(viewport());

    // 默认：像素模式开、方形开。
    assert!(view.pixel_mode());
    assert!(view.square_mode());
    assert!(view.brush_hard());
    assert_eq!(view.brush_shape(), BrushShape::Square);

    for toggle in [BrushToggle::Hard, BrushToggle::Square] {
        click_toggle(&mut view, toggle);
    }
    assert!(!view.pixel_mode());
    assert!(!view.square_mode());
    // 开关同步进了画笔。
    assert!(!view.brush_hard());
    assert_eq!(view.brush_shape(), BrushShape::Round);

    for toggle in [BrushToggle::Hard, BrushToggle::Square] {
        click_toggle(&mut view, toggle);
    }
    assert!(view.pixel_mode() && view.square_mode());
}

#[test]
fn the_symmetry_button_cycles_and_the_brush_follows() {
    let mut view = brush_view();
    assert_eq!(view.symmetry_mode(), crate::tools::Symmetry::Off);
    let center = view.symmetry_button_center().expect("symmetry button");
    for expected in [
        crate::tools::Symmetry::Vertical,
        crate::tools::Symmetry::Horizontal,
        crate::tools::Symmetry::Both,
        crate::tools::Symmetry::Off,
    ] {
        click_at(&mut view, center);
        assert_eq!(view.symmetry_mode(), expected);
        // 选项栏是显示源，`update` 每帧同步进画笔。
        assert_eq!(label_text(&view, view.options.symmetry), expected.label());
    }
}

#[test]
fn a_symmetric_stroke_paints_both_sides_through_the_view() {
    let mut view = brush_view();
    let center = view.symmetry_button_center().expect("symmetry button");
    click_at(&mut view, center);
    assert_eq!(view.symmetry_mode(), crate::tools::Symmetry::Vertical);

    // 左半边画一下（默认 32? 不 —— 默认 128×128 文档）。
    let point = view
        .canvas_camera()
        .document_to_screen(Vec2::new(20.0, 64.0));
    click_at(&mut view, point);
    let (left, right) = {
        let state = view.state.borrow();
        let pixels = &state.document.active_layer().unwrap().pixels;
        (pixels.get_pixel(20, 64), pixels.get_pixel(107, 64))
    };
    assert_eq!(left, Color::BLACK, "原位置应落笔");
    assert_eq!(right, Color::BLACK, "x' = 128 - 1 - 20 镜像位置也应落笔");
}

#[test]
fn the_brush_cursor_ring_follows_the_pointer_and_the_brush_size() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let pointer = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));

    // 默认 1px 像素笔：指针处出现一个方形轮廓（最小半径，保证看得见）。
    let ring = |commands: &[DrawCommand]| {
        commands.iter().find_map(|command| match command {
            DrawCommand::StrokeRect { rect, .. } if rect.center() == pointer => Some(rect.size),
            _ => None,
        })
    };
    let small = ring(&frame_with_pointer(&mut view, pointer)).expect("指针在画布上应有光标圈");

    // 笔刷变大 -> 圈跟着变大。
    view.brush.size = 20.0;
    let large = ring(&frame_with_pointer(&mut view, pointer)).expect("光标圈还在");
    assert!(large.width > small.width, "{large:?} 应比 {small:?} 大");

    // 指针离开画布（例如移到菜单栏）就不再画。
    let off_canvas = Vec2::new(pointer.x, 4.0);
    assert!(
        ring(&frame_with_pointer(&mut view, off_canvas)).is_none(),
        "指针不在画布上时不该有光标圈"
    );
}

#[test]
fn the_round_brush_draws_a_circle_cursor() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 关掉「方形」= 圆头笔。
    view.options.square_mode.set(false);
    view.update();
    view.brush.size = 20.0;
    let pointer = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));
    let zoom = view.canvas_camera().zoom;

    let commands = frame_with_pointer(&mut view, pointer);
    let radius = commands.iter().find_map(|command| match command {
        DrawCommand::StrokeCircle { center, radius, .. } if *center == pointer => Some(*radius),
        _ => None,
    });
    let radius = radius.expect("圆头笔应画圆形光标");
    assert!(
        (radius - 20.0 * zoom * 0.5).abs() < 0.01,
        "半径 {radius} 应等于直径 {} 的一半",
        20.0 * zoom
    );
}

/// 间距按钮：纹理笔靠它调连续性，两头都夹住。
#[test]
fn the_options_bar_spacing_buttons_tighten_and_cap_the_dabs() {
    let mut view = brush_view();
    assert!((view.brush_spacing() - 0.25).abs() < 1e-6, "默认间距 25%");
    for _ in 0..20 {
        click_adjust(&mut view, BrushAdjust::SpacingDown);
    }
    assert!((view.brush_spacing() - 0.02).abs() < 1e-6, "− 到底 = 2%");
    for _ in 0..60 {
        click_adjust(&mut view, BrushAdjust::SpacingUp);
    }
    assert!((view.brush_spacing() - 1.0).abs() < 1e-6, "+ 到顶 = 100%");
}

#[test]
fn the_options_bar_smooth_buttons_reach_zero_and_cap_at_one() {
    let mut view = brush_view();
    // 默认平滑 100%（二次中点平滑）。
    assert_eq!(view.brush_smooth(), 1.0);
    for _ in 0..12 {
        click_adjust(&mut view, BrushAdjust::SmoothDown);
    }
    assert_eq!(view.brush_smooth(), 0.0, "− 到底就是折线，笔到手到");
    for _ in 0..12 {
        click_adjust(&mut view, BrushAdjust::SmoothUp);
    }
    assert_eq!(view.brush_smooth(), 1.0, "+ 到顶不会超过 100%");
}

#[test]
fn the_options_bar_hardness_buttons_step_the_hardness() {
    let mut view = brush_view();
    let before = view.brush_hardness();
    click_adjust(&mut view, BrushAdjust::HardnessDown);
    assert!((view.brush_hardness() - (before - 0.05)).abs() < 1e-4);
    // 两头都夹住：不会变成 0（那样笔就没了）也不会超过 100%。
    for _ in 0..40 {
        click_adjust(&mut view, BrushAdjust::HardnessUp);
    }
    assert_eq!(view.brush_hardness(), 1.0);
    for _ in 0..40 {
        click_adjust(&mut view, BrushAdjust::HardnessDown);
    }
    assert_eq!(view.brush_hardness(), 0.05);
}

#[test]
fn picking_a_preset_sets_the_brush_and_the_options_bar_toggles() {
    let mut view = brush_view();
    // 柔边圆笔 = 24px / 100% / 硬度 35% / 圆头 / 关掉像素模式。
    let preset = &crate::tools::brush::preset::PRESETS[3];
    view.intents.push(Intent::BrushPreset(3));
    view.update();
    assert_eq!(view.brush_size(), preset.size);
    assert!((view.brush_hardness() - preset.hardness).abs() < 1e-4);
    assert!((view.brush_opacity() - preset.opacity).abs() < 1e-4);
    assert_eq!(view.brush_shape(), BrushShape::Round);
    // 选项栏的开关是显示源，预设必须回写它，否则下一帧就被覆盖回去。
    assert!(!view.pixel_mode() && !view.square_mode());
    assert_eq!(label_text(&view, view.options.preset_label), preset.name);
}

#[test]
fn the_preset_label_falls_back_to_custom() {
    let mut view = brush_view();
    click_adjust(&mut view, BrushAdjust::HardnessDown);
    assert_eq!(label_text(&view, view.options.preset_label), "自定义");
}

#[test]
fn a_left_drag_with_the_brush_paints_the_active_layer() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
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
    view.event(&InputEvent::PointerMove {
        position: point + Vec2::new(40.0, 0.0),
    });
    view.event(&InputEvent::PointerUp {
        position: point + Vec2::new(40.0, 0.0),
        button: PointerButton::Left,
    });
    view.update();

    let painted = view
        .state
        .borrow()
        .document
        .active_layer()
        .unwrap()
        .pixels
        .get_pixel(64, 64);
    assert_eq!(painted, Color::BLACK);
    assert!(view.take_texture_upload().is_some(), "画笔改动要重传纹理");
}

#[test]
fn the_eraser_clears_instead_of_painting() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 走真实路径：先按 E 选橡皮，再点工具栏的橡皮组按钮。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('e'),
    });
    view.update();
    let eraser = view.tool_center(ActiveTool::Eraser).expect("brush group");
    view.event(&InputEvent::PointerDown {
        position: eraser,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: eraser,
        button: PointerButton::Left,
    });
    view.update();
    assert_eq!(view.active_tool(), ActiveTool::Eraser);

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

    let alpha = view
        .state
        .borrow()
        .document
        .active_layer()
        .unwrap()
        .pixels
        .get_pixel(64, 64)
        .a;
    assert_eq!(alpha, 0, "橡皮把背景擦透明");
}

#[test]
fn erasing_removes_a_brush_stroke() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    paint_a_stroke(&mut view);
    let point = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));
    assert_eq!(
        view.state
            .borrow()
            .document
            .active_layer()
            .unwrap()
            .pixels
            .get_pixel(64, 64),
        Color::BLACK,
        "画笔应在 (64,64) 留下黑色"
    );

    // 先按 E 选橡皮，再点工具栏的橡皮组按钮。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('e'),
    });
    view.update();
    let eraser = view.tool_center(ActiveTool::Eraser).expect("brush group");
    view.event(&InputEvent::PointerDown {
        position: eraser,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: eraser,
        button: PointerButton::Left,
    });
    view.update();
    view.event(&InputEvent::PointerDown {
        position: point,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: point,
        button: PointerButton::Left,
    });

    let alpha = view
        .state
        .borrow()
        .document
        .active_layer()
        .unwrap()
        .pixels
        .get_pixel(64, 64)
        .a;
    assert_eq!(alpha, 0, "橡皮应把这一笔擦掉（alpha=0）");
}
