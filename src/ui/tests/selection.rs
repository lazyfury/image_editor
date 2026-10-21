//! 选区：Shift 加选 / Alt 减选、羽化、框选约束画笔、魔棒、马赛克描边、套索。

use super::*;

/// 拖一个矩形选区（屏幕坐标），带修饰键状态。
fn drag_selection(view: &mut EditorView, from: Vec2, to: Vec2, modifiers: igui_core::Modifiers) {
    view.set_modifiers(modifiers);
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
    view.set_modifiers(igui_core::Modifiers::NONE);
}

#[test]
fn shift_adds_and_alt_subtracts_from_the_selection() {
    let mut view = brush_view();
    view.event(&InputEvent::KeyDown {
        key: Key::Character('m'),
    });
    view.update();
    let camera = view.canvas_camera();
    let a = camera.document_to_screen(Vec2::new(20.0, 20.0));
    let b = camera.document_to_screen(Vec2::new(60.0, 60.0));

    // 先框一块：40×40。
    drag_selection(&mut view, a, b, igui_core::Modifiers::NONE);
    assert_eq!(
        view.selection().map(|selection| selection.area()),
        Some(40 * 40)
    );

    // Shift 框右侧同样大的一块：40×40 + 40×40（不重叠）。
    let c = camera.document_to_screen(Vec2::new(80.0, 20.0));
    let d = camera.document_to_screen(Vec2::new(120.0, 60.0));
    drag_selection(
        &mut view,
        c,
        d,
        igui_core::Modifiers {
            shift: true,
            ..igui_core::Modifiers::NONE
        },
    );
    assert_eq!(
        view.selection().map(|selection| selection.area()),
        Some(2 * 40 * 40),
        "Shift 应该是加选"
    );

    // Alt 框掉左边那块：只剩右边 1600。
    drag_selection(
        &mut view,
        a,
        b,
        igui_core::Modifiers {
            alt: true,
            ..igui_core::Modifiers::NONE
        },
    );
    assert_eq!(
        view.selection().map(|selection| selection.area()),
        Some(40 * 40),
        "Alt 应该是减选"
    );

    // 减空了就没有选区。
    drag_selection(
        &mut view,
        c,
        d,
        igui_core::Modifiers {
            alt: true,
            ..igui_core::Modifiers::NONE
        },
    );
    assert!(view.selection().is_none());
}

/// 「羽化」把选区边缘变成覆盖率渐变，填充按覆盖率混合（边缘是中间色）。
#[test]
fn feathering_softens_the_selection_and_the_fill_follows_it() {
    let mut view = brush_view();
    // 框一块 40×40，羽化后边缘变淡、中心仍是满覆盖。
    view.select_all();
    let radius = 8.0;
    assert!(view.feather_selection());
    let selection = view.selection().expect("羽化后选区还在");
    assert_eq!(selection.coverage(64, 64), 1.0, "中心仍满覆盖");
    assert!(
        selection.coverage(0, 64) < 0.5,
        "画布边缘应被羽化淡出：{}",
        selection.coverage(0, 64)
    );
    assert!(selection.area() < 128 * 128, "面积按覆盖率折算");

    // 用矩形填充盖住几乎整张画布（起点在画布区域内）：羽化边缘的像素是中间色，
    // 不是纯前景色。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('r'),
    });
    view.update();
    let camera = view.canvas_camera();
    let from = camera.document_to_screen(Vec2::new(1.0, 1.0));
    let to = camera.document_to_screen(Vec2::new(127.0, 127.0));
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
    let snapshot = view.document_snapshot();
    let pixels = &snapshot.active_layer().unwrap().pixels;
    let edge = pixels.get_pixel(3, 64);
    let center = pixels.get_pixel(64, 64);
    assert_eq!(center, Color::BLACK, "中间应被填满");
    assert!(
        edge.r > 40 && edge.r < 240,
        "羽化边缘应是中间色而不是硬边：{edge:?}"
    );
    let _ = radius;
}

#[test]
fn a_selection_confines_the_brush_and_escape_clears_it() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();

    // 框一块 54..74 × 54..74。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('m'),
    });
    view.update();
    let start = camera.document_to_screen(Vec2::new(54.0, 54.0));
    let end = camera.document_to_screen(Vec2::new(74.0, 74.0));
    view.event(&InputEvent::PointerDown {
        position: start,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove { position: end });
    view.event(&InputEvent::PointerUp {
        position: end,
        button: PointerButton::Left,
    });
    let selection = view.selection().expect("应产生选区");
    assert!(selection.contains(64, 64));
    assert!(!selection.contains(100, 100));

    // 画笔只在选区里落笔。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    let inside = camera.document_to_screen(Vec2::new(64.5, 64.5));
    let outside = camera.document_to_screen(Vec2::new(100.0, 100.0));
    for point in [inside, outside] {
        view.event(&InputEvent::PointerDown {
            position: point,
            button: PointerButton::Left,
        });
        view.event(&InputEvent::PointerUp {
            position: point,
            button: PointerButton::Left,
        });
    }
    let (inside_pixel, outside_pixel) = {
        let state = view.state.borrow();
        let layer = state.document.active_layer().unwrap();
        (
            layer.pixels.get_pixel(64, 64),
            layer.pixels.get_pixel(100, 100),
        )
    };
    assert_eq!(inside_pixel, Color::BLACK, "选区内落笔");
    assert_eq!(outside_pixel, Color::WHITE, "选区外不落笔");

    // Esc 清空选区。
    assert!(view
        .event(&InputEvent::KeyDown { key: Key::Escape })
        .is_handled());
    assert!(view.selection().is_none());
}

/// 魔棒：点一下把容差连通区域变成选区（默认白底 -> 整张文档）。
#[test]
fn the_magic_wand_turns_a_click_into_a_selection() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();

    view.event(&InputEvent::KeyDown {
        key: Key::Character('w'),
    });
    view.update();
    assert_eq!(view.active_tool(), ActiveTool::MagicWand, "W 切到魔棒");

    let point = camera.document_to_screen(Vec2::new(64.5, 64.5));
    view.event(&InputEvent::PointerDown {
        position: point,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: point,
        button: PointerButton::Left,
    });

    let selection = view.selection().expect("魔棒应产生选区");
    assert!(selection.contains(0, 0), "白底连通 -> 左上角也选中");
    assert!(selection.contains(127, 127), "右下角也选中");
    assert_eq!(selection.area(), 128 * 128);
}

/// 选区边框是黑白相间的线段（马赛克 / marching ants），不是半透明单色线。
#[test]
fn the_selection_border_is_marching_ants() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();

    view.event(&InputEvent::KeyDown {
        key: Key::Character('m'),
    });
    view.update();
    let start = camera.document_to_screen(Vec2::new(54.0, 54.0));
    let end = camera.document_to_screen(Vec2::new(74.0, 74.0));
    view.event(&InputEvent::PointerDown {
        position: start,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove { position: end });
    view.event(&InputEvent::PointerUp {
        position: end,
        button: PointerButton::Left,
    });
    assert!(view.selection().is_some());

    let commands = paint_commands(&view);
    let has_line = |color: igui_core::Color| {
        commands.iter().any(
            |command| matches!(command, DrawCommand::Line { paint, .. } if paint.color == color),
        )
    };
    assert!(has_line(igui_core::Color::BLACK), "选区边框应有黑段");
    assert!(has_line(igui_core::Color::WHITE), "选区边框应有白段");
}

/// 套索：沿自由折线围出的区域被选中，区域外不选。
#[test]
fn a_lasso_selects_only_the_enclosed_region() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();
    view.event(&InputEvent::KeyDown {
        key: Key::Character('l'),
    });
    view.update();

    let path = [
        Vec2::new(20.0, 20.0),
        Vec2::new(60.0, 20.0),
        Vec2::new(60.0, 60.0),
        Vec2::new(20.0, 60.0),
        Vec2::new(20.0, 20.0),
    ];
    let screen = |point: Vec2| camera.document_to_screen(point);
    view.event(&InputEvent::PointerDown {
        position: screen(path[0]),
        button: PointerButton::Left,
    });
    for point in &path[1..] {
        view.event(&InputEvent::PointerMove {
            position: screen(*point),
        });
    }
    view.event(&InputEvent::PointerUp {
        position: screen(path[4]),
        button: PointerButton::Left,
    });

    let selection = view.selection().expect("套索应产生选区");
    assert!(selection.contains(40, 40), "围出的区域应被选中");
    assert!(!selection.contains(80, 80), "区域外不该选中");
}

/// 套索拖动中会实时画折线预览（黑白马赛克线段）。
#[test]
fn the_lasso_preview_is_drawn_while_dragging() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();
    view.event(&InputEvent::KeyDown {
        key: Key::Character('l'),
    });
    view.update();

    let screen = |point: Vec2| camera.document_to_screen(point);
    view.event(&InputEvent::PointerDown {
        position: screen(Vec2::new(20.0, 20.0)),
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove {
        position: screen(Vec2::new(60.0, 20.0)),
    });
    view.event(&InputEvent::PointerMove {
        position: screen(Vec2::new(60.0, 60.0)),
    });

    let commands = paint_commands(&view);
    let has_line = |color: igui_core::Color| {
        commands.iter().any(
            |command| matches!(command, DrawCommand::Line { paint, .. } if paint.color == color),
        )
    };
    assert!(has_line(igui_core::Color::BLACK), "套索预览应有黑段");
    assert!(has_line(igui_core::Color::WHITE), "套索预览应有白段");
}
