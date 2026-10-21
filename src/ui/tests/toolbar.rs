//! 工具栏 / 快捷键 / 图标：点组按钮、长按浮层、图标解析。

use super::*;

#[test]
fn keyboard_shortcut_selects_a_tool() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    assert!(view
        .event(&InputEvent::KeyDown {
            key: Key::Character('b'),
        })
        .is_handled());
    view.update();
    assert_eq!(view.active_tool(), ActiveTool::Brush);
    assert_eq!(label_text(&view, view.tool_label), "画笔工具");
}

#[test]
fn clicking_a_group_button_activates_its_face() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 默认脸是矩形选择：点选区组按钮激活它。
    let center = view
        .tool_center(ActiveTool::RectangleSelect)
        .expect("selection group");
    click(&mut view, center);
    view.update();
    assert_eq!(view.active_tool(), ActiveTool::RectangleSelect);

    // 按 L 选套索，脸跟着变；再点同一个组按钮就激活套索。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('l'),
    });
    view.update();
    assert_eq!(view.tool_face(3), Some(ActiveTool::LassoSelect));
    click(&mut view, center);
    view.update();
    assert_eq!(view.active_tool(), ActiveTool::LassoSelect);
    assert_eq!(label_text(&view, view.tool_label), "自由选区");
}

#[test]
fn the_active_tool_button_paints_the_selection_color() {
    let theme = crate::theme::editor_theme(false);
    let mut view = EditorView::new(theme, AppState::default());
    view.layout(viewport());
    let selection = theme.palette().selection;
    assert!(
        paint_commands(&view).iter().any(|command| matches!(
            command,
            DrawCommand::FillRoundedRect { paint, .. } if paint.color == selection
        )),
        "当前工具的按钮应画出 selection 高亮"
    );
}

#[test]
fn the_icon_pack_covers_the_toolbar_icons() {
    let view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    let needed = ActiveTool::ALL.len() + HistoryAction::ALL.len();
    assert!(view.icon_count() >= needed, "图标包应覆盖工具栏图标");
}

/// 每个工具的图标名都能在 vendor 的图标包里解析（包括自制的 `gradient`）。
#[test]
fn every_tool_icon_name_resolves() {
    let icons = crate::icons::IconSet::load();
    for tool in ActiveTool::ALL {
        assert!(
            icons.document(tool.icon()).is_some(),
            "缺少图标 `{}`",
            tool.icon()
        );
    }
}

#[test]
fn icons_draw_into_the_painted_frame() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 图标用圆头 / 圆角，会发出 FillCircle；普通 UI 不画圆。
    assert!(
        paint_commands(&view)
            .iter()
            .any(|command| matches!(command, DrawCommand::FillCircle { .. })),
        "图标应发出 FillCircle"
    );
}

#[test]
fn the_toolbar_packs_its_buttons_tightly() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let first = view.tool_group_center(0).expect("first group");
    let last = view
        .tool_group_center(TOOL_GROUPS.len() - 1)
        .expect("last group");
    let pitch = (last.y - first.y) / (TOOL_GROUPS.len() - 1) as f32;
    // `Flex` 默认带 16px 内边距；忘记清零会让每一项高 32px、间距翻倍。
    assert!(pitch < 60.0, "工具栏每一项的间距应紧凑，实际 {pitch:.1}px");
}

/// 快捷键选中组内工具后，组的「脸」跟着更新（组按钮图标 / 标签同步）。
#[test]
fn a_shortcut_updates_the_group_face() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    assert_eq!(view.tool_face(3), Some(ActiveTool::RectangleSelect));
    assert_eq!(view.tool_face(6), Some(ActiveTool::RectangleFill));
    // L 把选区组的脸换成套索；R 把填充形状组的脸换成矩形填充。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('l'),
    });
    view.update();
    assert_eq!(view.tool_face(3), Some(ActiveTool::LassoSelect));
    view.event(&InputEvent::KeyDown {
        key: Key::Character('r'),
    });
    view.update();
    assert_eq!(view.tool_face(6), Some(ActiveTool::RectangleFill));
    assert_eq!(view.tool_face(1), Some(ActiveTool::Brush));
}

/// 长按组按钮展开工具浮层；短按不展开。
#[test]
fn a_long_press_opens_the_tool_flyout() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.set_long_press_ms(0);
    view.layout(viewport());
    let center = view.tool_center(ActiveTool::Brush).expect("brush group");

    view.event(&InputEvent::PointerDown {
        position: center,
        button: PointerButton::Left,
    });
    view.update();
    assert_eq!(view.tool_flyout_open(), Some(1), "长按应展开画笔组浮层");
    assert!(!view.needs_frame(), "展开后不再需要持续出帧");

    // 松手不算短按，不会切工具。
    view.event(&InputEvent::PointerUp {
        position: center,
        button: PointerButton::Left,
    });
    view.update();
    assert_eq!(view.active_tool(), ActiveTool::Brush);
}

/// 没到阈值的短按只激活脸，不展开浮层。
#[test]
fn a_short_click_does_not_open_the_tool_flyout() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.set_long_press_ms(10_000);
    view.layout(viewport());
    let center = view.tool_center(ActiveTool::Brush).expect("brush group");
    view.event(&InputEvent::PointerDown {
        position: center,
        button: PointerButton::Left,
    });
    view.update();
    assert_eq!(view.tool_flyout_open(), None);
    assert!(view.needs_frame(), "等阈值时应持续出帧");
    view.event(&InputEvent::PointerUp {
        position: center,
        button: PointerButton::Left,
    });
    view.update();
    assert_eq!(view.tool_flyout_open(), None);
    assert_eq!(view.active_tool(), ActiveTool::Brush);
}
