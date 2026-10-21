//! 窗口骨架：标签条 / 菜单栏 / 选项栏的顺序，状态栏文本不抖。

use super::*;

#[test]
fn the_status_bar_survives_a_relayout_without_extra_text_churn() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    // 首次布局会做适配，状态栏文字随之变化；先让它稳定下来。
    view.layout(viewport());
    view.update();
    view.layout(viewport());
    let before = igui_ui::layout_count(&view.tree);
    view.update();
    view.layout(viewport());
    assert_eq!(
        igui_ui::layout_count(&view.tree),
        before,
        "稳定后的一帧不该触发重排"
    );
}

/// 标签条只占顶部一条，不盖住编辑器。
#[test]
fn the_tab_strip_stays_at_the_top() {
    let mut tabs = WorkspaceTabs::new(crate::theme::editor_theme(false));
    tabs.set_tabs(
        &[WorkspaceTab {
            name: "A".into(),
            dirty: false,
        }],
        0,
    );
    tabs.layout(viewport());
    let center = tabs.tab_center(0).expect("标签");
    assert!(center.y < 40.0, "标签条应贴顶，实际 y = {}", center.y);
}

/// 标签条下方的点击应透传给编辑器，工具能切换。
#[test]
fn a_click_on_the_editor_toolbar_reaches_it_through_the_tab_strip() {
    let theme = crate::theme::editor_theme(false);
    let mut editor = EditorView::new(theme, AppState::default());
    editor.set_tab_band(TAB_STRIP_HEIGHT);
    let mut tabs = WorkspaceTabs::new(theme);
    tabs.set_tabs(
        &[WorkspaceTab {
            name: "A".into(),
            dirty: false,
        }],
        0,
    );
    editor.layout(viewport());
    // 菜单栏在最上：标签条钉在菜单栏下面（宿主每帧同步，这里手动做一次）。
    tabs.set_top(editor.menu_bar_rect().expect("菜单栏").bottom());
    tabs.layout(viewport());
    let center = editor
        .tool_center(ActiveTool::RectangleSelect)
        .expect("选区组按钮");
    for event in [
        InputEvent::PointerDown {
            position: center,
            button: PointerButton::Left,
        },
        InputEvent::PointerUp {
            position: center,
            button: PointerButton::Left,
        },
    ] {
        if !tabs.event(&event).is_handled() {
            editor.event(&event);
        }
    }
    editor.update();
    assert_eq!(
        editor.active_tool(),
        ActiveTool::RectangleSelect,
        "标签条下方的点击应到编辑器"
    );
}

/// 菜单栏在最上，工作区标签条在它下面：菜单底 ≤ 标签条顶 ≤ 选项栏顶。
#[test]
fn the_menu_bar_sits_above_the_tab_band() {
    let theme = crate::theme::editor_theme(false);
    let mut view = EditorView::new(theme, AppState::default());
    view.set_tab_band(TAB_STRIP_HEIGHT);
    view.layout(viewport());

    let menu = view.menu_bar_rect().expect("菜单栏");
    let options = view.options_bar_rect().expect("选项栏");
    assert!(menu.top() <= 0.5, "菜单栏应贴顶，实际 {:?}", menu.top());
    assert!(
        menu.bottom() <= options.top(),
        "菜单栏 ({}) 不该和选项栏 ({}) 重叠",
        menu.bottom(),
        options.top()
    );
    assert!(
        (options.top() - menu.bottom() - TAB_STRIP_HEIGHT).abs() < 0.5,
        "菜单栏和选项栏之间应正好留出标签条：{} -> {}",
        menu.bottom(),
        options.top()
    );

    // 宿主把标签条钉在菜单栏下面，正好落在预留带里。
    let mut tabs = WorkspaceTabs::new(theme);
    tabs.set_tabs(
        &[WorkspaceTab {
            name: "A".into(),
            dirty: false,
        }],
        0,
    );
    tabs.set_top(menu.bottom());
    tabs.layout(viewport());
    let bar = tabs.bar_rect().expect("标签条");
    assert!(
        (bar.top() - menu.bottom()).abs() < 0.5,
        "标签条贴菜单栏下沿"
    );
    assert!(bar.bottom() <= options.top() + 0.5, "标签条不该压到选项栏");
}

/// 没有标签条时（`set_tab_band(0)`）菜单下面直接就是选项栏。
#[test]
fn without_a_tab_band_the_options_bar_follows_the_menu() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.set_tab_band(0.0);
    view.layout(viewport());
    let menu = view.menu_bar_rect().expect("菜单栏");
    let options = view.options_bar_rect().expect("选项栏");
    assert!(
        (options.top() - menu.bottom()).abs() < 0.5,
        "标签带 0 高时应紧接着菜单栏：{} -> {}",
        menu.bottom(),
        options.top()
    );
}
