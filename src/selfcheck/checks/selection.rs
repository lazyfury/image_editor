//! 选区：合并规则、选区菜单、魔棒。

use crate::app::state::ActiveTool;
use crate::selfcheck::*;
use crate::ui::EditorView;
use igui_core::{InputEvent, Key, PointerButton, Vec2};

/// 拖一个矩形选区（带修饰键），走真实事件。
pub(crate) fn drag_selection(
    view: &mut EditorView,
    from: Vec2,
    to: Vec2,
    modifiers: igui_core::Modifiers,
) {
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

/// 选区加选 / 减选：Shift 框一块并进旧选区，Alt 再框掉它。
pub(crate) fn check_selection_merge(failures: &mut Vec<String>) {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('m'),
    });
    view.update();
    view.layout(viewport());
    let camera = view.canvas_camera();
    let a = camera.document_to_screen(Vec2::new(20.0, 20.0));
    let b = camera.document_to_screen(Vec2::new(60.0, 60.0));
    let c = camera.document_to_screen(Vec2::new(80.0, 20.0));
    let d = camera.document_to_screen(Vec2::new(120.0, 60.0));
    let area = |view: &EditorView| view.selection().map(|s| s.area()).unwrap_or(0);

    drag_selection(&mut view, a, b, igui_core::Modifiers::NONE);
    let first = area(&view);
    if first == 0 {
        failures.push("框选没有产生选区".to_string());
        return;
    }

    // Shift：把右边那块并进来，面积至少翻倍。
    drag_selection(
        &mut view,
        c,
        d,
        igui_core::Modifiers {
            shift: true,
            ..igui_core::Modifiers::NONE
        },
    );
    let added = area(&view);
    if added < first * 2 {
        failures.push(format!("Shift 加选没生效：{first} -> {added}"));
    }

    // Alt：再把第一块减掉，面积应该变小。
    drag_selection(
        &mut view,
        a,
        b,
        igui_core::Modifiers {
            alt: true,
            ..igui_core::Modifiers::NONE
        },
    );
    let remaining = area(&view);
    if remaining >= added {
        failures.push(format!("Alt 减选没生效：{added} -> {remaining}"));
    }
}

/// 「选择」菜单是第 4 个标题（文件 / 编辑 / 图像 / 图层 / 选择 …）。
const SELECT_MENU: usize = 4;

/// 打开「选择」菜单并按文字点里面的某一项。
pub(crate) fn pick_select_menu(
    view: &mut EditorView,
    label: &str,
    failures: &mut Vec<String>,
) -> bool {
    pick_menu(view, SELECT_MENU, "选择", label, failures)
}

/// 点开第 `index` 个菜单，再点里面 `label` 这一项。
pub(crate) fn pick_menu(
    view: &mut EditorView,
    index: usize,
    title: &str,
    label: &str,
    failures: &mut Vec<String>,
) -> bool {
    let Some(title_center) = view.menu_center(index) else {
        failures.push(format!("找不到「{title}」菜单标题"));
        return false;
    };
    click_at(view, title_center);
    view.update();
    view.layout(viewport());
    let frame = paint_frame(view);
    match text_center(&frame, label) {
        Some(center) => {
            click_at(view, center);
            view.update();
            true
        }
        None => {
            failures.push(format!("「{title}」菜单里找不到「{label}」"));
            false
        }
    }
}

/// 选择菜单的「全选 / 反选」：走真实的下拉点击，选区和状态栏都要跟着变。
pub(crate) fn check_selection_menu(failures: &mut Vec<String>) {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());

    if pick_select_menu(&mut view, "全选", failures) {
        match view.selection() {
            Some(selection) if selection.area() == 128 * 128 => {}
            Some(selection) => failures.push(format!(
                "全选后选区面积应为 {}，实为 {}",
                128 * 128,
                selection.area()
            )),
            None => failures.push("全选没有产生选区".to_string()),
        }
    }

    if pick_select_menu(&mut view, "反选", failures) && view.selection().is_some() {
        failures.push("全选再反选应取消选择".to_string());
    }

    // 羽化：先全选，再点「羽化」，画布边缘的覆盖率应淡到一半以下。
    if pick_select_menu(&mut view, "全选", failures)
        && pick_select_menu(&mut view, "羽化", failures)
    {
        match view.selection() {
            Some(selection) if selection.coverage(0, 64) < 0.5 => {
                if selection.coverage(64, 64) < 0.99 {
                    failures.push("羽化不该把选区中心也变淡".to_string());
                }
            }
            Some(selection) => {
                failures.push(format!("羽化没有让边缘变淡：{}", selection.coverage(0, 64)))
            }
            None => failures.push("羽化把选区弄没了".to_string()),
        }
    }
}

/// 魔棒：`W` 切换；点白底把整张文档选成选区（不落笔）。
pub(crate) fn check_magic_wand(failures: &mut Vec<String>) {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('w'),
    });
    view.update();
    if view.active_tool() != ActiveTool::MagicWand {
        failures.push("W 应切到魔棒".to_string());
    }
    let point = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));
    click_at(&mut view, point);
    view.update();
    match view.selection() {
        Some(selection) if selection.area() == 128 * 128 => {}
        Some(selection) => failures.push(format!(
            "魔棒选区面积应为 {}，实为 {}",
            128 * 128,
            selection.area()
        )),
        None => failures.push("魔棒点击应产生选区".to_string()),
    }
}
