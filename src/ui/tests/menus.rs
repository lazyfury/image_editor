//! 菜单栏：标题开 / 切 / 关、Esc、编辑菜单的剪切复制粘贴。

use super::*;

fn menu_index(name: &str) -> usize {
    crate::ui::menu::MENUS
        .iter()
        .position(|candidate| *candidate == name)
        .expect("menu exists")
}

/// 走 `EditorView::event` 的一次点击（覆盖层参与路由，与真实鼠标一致）。
fn click_event(view: &mut EditorView, position: Vec2) {
    view.event(&InputEvent::PointerDown {
        position,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position,
        button: PointerButton::Left,
    });
}

/// 一帧里某段文字的落点（点菜单项用）。
fn text_center(view: &EditorView, needle: &str) -> Option<Vec2> {
    paint_commands(view)
        .into_iter()
        .find_map(|command| match command {
            DrawCommand::DrawText {
                text,
                position,
                font_size,
                ..
            } if text.contains(needle) => Some(position + Vec2::new(8.0, -font_size * 0.5)),
            _ => None,
        })
}

/// 棋盘格开着时「视图 → 显示棋盘格」带勾选（自绘下拉画 `✓ ` 前缀）。
#[test]
fn the_view_menu_marks_the_checkerboard_when_it_is_on() {
    let view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    let spec = crate::ui::menu::menu_spec(6, view.menu_state());
    let entry = spec
        .iter()
        .find(|entry| entry.action() == Some(crate::ui::menu::MenuAction::ToggleCheckerboard))
        .expect("视图菜单里有棋盘格开关");
    assert!(entry.label().starts_with('✓'), "开着的开关应带勾选");
}

#[test]
fn clicking_a_menu_title_opens_its_dropdown() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let help = menu_index("帮助");
    let center = view.menu_center(help).expect("help menu button");
    click_event(&mut view, center);
    view.update();
    assert!(view.menu_open(), "点菜单标题应打开下拉");
}

#[test]
fn clicking_another_menu_title_switches_in_one_click() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let file = menu_index("文件");
    let edit = menu_index("编辑");
    let center = view.menu_center(file).expect("file menu button");
    click_event(&mut view, center);
    view.update();
    assert_eq!(view.open_menu_index(), Some(file));
    // 一次点击就切到「编辑」，而不是先关再点。
    let center = view.menu_center(edit).expect("edit menu button");
    click_event(&mut view, center);
    view.update();
    assert_eq!(view.open_menu_index(), Some(edit));
    assert!(view.menu_open());
}

#[test]
fn clicking_the_open_menu_title_closes_it() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let help = menu_index("帮助");
    let center = view.menu_center(help).expect("help menu button");
    click_event(&mut view, center);
    view.update();
    assert!(view.menu_open());
    click_event(&mut view, center);
    view.update();
    assert!(!view.menu_open(), "点同一标题应关闭菜单");
}

#[test]
fn escape_closes_an_open_menu() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let help = menu_index("帮助");
    let center = view.menu_center(help).expect("help menu button");
    click_event(&mut view, center);
    view.update();
    assert!(view.menu_open());
    view.event(&InputEvent::KeyDown { key: Key::Escape });
    assert!(!view.menu_open(), "Esc 应关闭菜单");
}

/// 「编辑」菜单的剪切 / 复制 / 粘贴接到真实动作上（不是占位）。
#[test]
fn the_edit_menu_cuts_copies_and_pastes_the_selection() {
    let mut view = brush_view();
    // 画一块黑，框选它。
    let camera = view.canvas_camera();
    let paint_rect = |view: &mut EditorView| {
        let from = camera.document_to_screen(Vec2::new(20.0, 20.0));
        let to = camera.document_to_screen(Vec2::new(60.0, 60.0));
        view.event(&InputEvent::KeyDown {
            key: Key::Character('r'),
        });
        view.update();
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
    };
    paint_rect(&mut view);
    view.state.borrow_mut().selection = Some(std::rc::Rc::new(crate::canvas::Selection::rect(
        crate::document::PixelRegion::new(20, 20, 40, 40),
        (128, 128),
    )));

    // 「编辑」菜单里点「复制」，剪贴板应有内容。
    let pick = |view: &mut EditorView, label: &str| {
        let title = view.menu_center(1).expect("编辑菜单标题");
        click_at(view, title);
        view.layout(viewport());
        let item = text_center(view, label).expect("菜单项");
        click_at(view, item);
    };
    pick(&mut view, "复制");
    assert!(view.has_clipboard(), "复制后剪贴板应有内容");

    // 剪切 -> 选区变透明；粘贴 -> 复原。
    let before = {
        let state = view.state.borrow();
        state
            .document
            .active_layer()
            .unwrap()
            .pixels
            .get_pixel(30, 30)
    };
    assert_eq!(before, Color::BLACK);
    pick(&mut view, "剪切");
    let cleared = {
        let state = view.state.borrow();
        state
            .document
            .active_layer()
            .unwrap()
            .pixels
            .get_pixel(30, 30)
    };
    assert_eq!(cleared.a, 0, "剪切后选区内容被清掉");
    pick(&mut view, "粘贴");
    let pasted = {
        let state = view.state.borrow();
        state
            .document
            .active_layer()
            .unwrap()
            .pixels
            .get_pixel(30, 30)
    };
    assert_eq!(pasted, Color::BLACK, "粘贴回原位");
}
