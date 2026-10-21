//! 模拟窗口：属性（完整属性）/ 关于 / 快捷键。

use super::*;

/// 按一下选项栏末尾的「全部属性…」：只留下一个「打开设置窗口」的意图。
#[test]
fn the_options_bar_asks_for_the_full_properties_window() {
    let mut view = brush_view();
    let center = view.settings_button_center().expect("全部属性… 按钮");
    view.event(&InputEvent::PointerDown {
        position: center,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: center,
        button: PointerButton::Left,
    });
    view.update();
    assert_eq!(
        view.take_dialog_request(),
        Some(crate::ui::DialogKind::Settings),
        "选项栏放不下的属性要能打开完整属性窗口"
    );
}

/// 属性窗口显示的是编辑器当前的值（不是打开那一刻的快照）。
#[test]
fn the_properties_window_shows_the_live_brush_size() {
    let theme = crate::theme::editor_theme(false);
    let mut view = brush_view();
    view.event(&InputEvent::KeyDown {
        key: Key::Character(']'),
    });
    view.update();
    let snapshot = view.settings_snapshot();

    let mut window = crate::ui::DialogWindow::new(
        crate::ui::DialogKind::Settings,
        theme,
        crate::ui::SettingsWindowInput {
            cells: view.option_cells(),
            intents: view.intents(),
        },
        crate::ui::AboutSnapshot::default(),
    );
    window.sync(&snapshot);
    window.layout(viewport());
    let texts = dialog_texts(&window);
    assert!(
        texts.contains(&format!("{:.0} px", snapshot.size)),
        "属性窗口应显示当前笔刷大小 {:?}，实际 {texts:?}",
        snapshot.size
    );
}

/// 三个对话框都要能把「关闭」喊出来（Esc / 关闭按钮 / 点外面都走这个标记）。
#[test]
fn every_dialog_can_be_closed() {
    let theme = crate::theme::editor_theme(false);
    let view = brush_view();
    for kind in [
        crate::ui::DialogKind::Settings,
        crate::ui::DialogKind::About,
        crate::ui::DialogKind::Shortcuts,
    ] {
        let mut window = crate::ui::DialogWindow::new(
            kind,
            theme,
            crate::ui::SettingsWindowInput {
                cells: view.option_cells(),
                intents: view.intents(),
            },
            view_about_snapshot(),
        );
        window.layout(viewport());
        assert!(!window.take_close(), "{kind:?} 一开始不该是关闭状态");
        window.event(&InputEvent::KeyDown { key: Key::Escape });
        assert!(window.take_close(), "{kind:?} 按下 Esc 应请求关闭");
        assert!(!window.take_close(), "{kind:?} 关闭请求只算一次");
    }
}

/// 关于窗口的数字是现取的：菜单项 / 占位项来自菜单表，工具来自工具表。
#[test]
fn the_about_window_counts_match_the_menu_and_tool_tables() {
    let snapshot = view_about_snapshot();
    let (menu_items, placeholders) = crate::ui::menu::item_counts();
    let tools: usize = crate::app::tool_groups::TOOL_GROUPS
        .iter()
        .map(|group| group.tools.len())
        .sum();
    assert_eq!(snapshot.menu_items, menu_items);
    assert_eq!(snapshot.placeholders, placeholders);
    assert_eq!(snapshot.tools, tools);
    let lines = snapshot.lines();
    assert!(lines.iter().any(|(label, _)| label == "版本"));
    assert!(lines
        .iter()
        .any(|(_, value)| value.contains(&menu_items.to_string())));
}

/// 快捷键表覆盖每个工具键与菜单表里的每个快捷键。
#[test]
fn the_shortcut_table_covers_tools_and_menus() {
    let rows = crate::ui::dialogs::shortcut_rows();
    for (key, tool) in crate::ui::TOOL_KEYS {
        assert!(
            rows.iter()
                .any(|(row_key, label)| row_key == &key.to_string() && label == tool.label()),
            "快捷键表里缺少工具键 {key}（{}）",
            tool.label()
        );
    }
    let state = crate::ui::menu::MenuState {
        has_document: true,
        can_undo: true,
        can_redo: true,
        has_selection: true,
        can_paste: true,
        checkerboard: false,
    };
    let mut menu_shortcuts = 0;
    for index in 0..crate::ui::menu::MENUS.len() {
        for entry in crate::ui::menu::menu_spec(index, state) {
            if let crate::ui::menu::MenuEntry::Item {
                shortcut: Some(shortcut),
                ..
            } = entry
            {
                menu_shortcuts += 1;
                assert!(
                    rows.iter().any(|(row_key, _)| row_key == shortcut),
                    "快捷键表里缺少菜单快捷键 {shortcut}"
                );
            }
        }
    }
    assert!(menu_shortcuts > 0, "菜单表应该带一些快捷键");
}

/// 一帧里的所有文字（对话框的树自带绘制入口）。
fn dialog_texts(window: &crate::ui::DialogWindow) -> Vec<String> {
    let mut ctx = PaintContext::new();
    window.paint(&mut ctx);
    ctx.into_draw_list()
        .into_commands()
        .into_iter()
        .filter_map(|command| match command {
            DrawCommand::DrawText { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

fn view_about_snapshot() -> crate::ui::AboutSnapshot {
    let view = brush_view();
    let (menu_items, placeholders) = crate::ui::menu::item_counts();
    crate::ui::AboutSnapshot {
        version: env!("CARGO_PKG_VERSION").to_string(),
        igui: "v0.3.0".to_string(),
        font: "随包".to_string(),
        document: Some(view.document_size()),
        layers: view.layer_count(),
        tools: crate::app::tool_groups::TOOL_GROUPS
            .iter()
            .map(|group| group.tools.len())
            .sum(),
        menu_items,
        placeholders,
    }
}
