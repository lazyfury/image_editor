//! 界面骨架：菜单栏 / 选项栏 / 调色盘 / 可拖动面板的顺序与交互。

use crate::document::Color;
use crate::selfcheck::*;
use crate::ui::EditorView;
use igui_core::{InputEvent, Key, Vec2};

pub(crate) fn check_chrome_order(failures: &mut Vec<String>) {
    let theme = crate::theme::editor_theme(false);
    let mut view = EditorView::new(theme, AppState::default());
    view.set_tab_band(crate::ui::TAB_STRIP_HEIGHT);
    view.layout(viewport());

    let (Some(menu), Some(options)) = (view.menu_bar_rect(), view.options_bar_rect()) else {
        failures.push("取不到菜单栏 / 选项栏的矩形".to_string());
        return;
    };
    if menu.top() > 0.5 {
        failures.push(format!("菜单栏应贴顶，实际 top = {}", menu.top()));
    }
    let band = options.top() - menu.bottom();
    if (band - crate::ui::TAB_STRIP_HEIGHT).abs() > 0.5 {
        failures.push(format!(
            "菜单栏和选项栏之间应留出标签条 {:.0}px，实际 {band:.1}px",
            crate::ui::TAB_STRIP_HEIGHT
        ));
    }

    // 宿主把标签条钉在菜单栏下面，正好落在那条带子里。
    let mut tabs = crate::ui::WorkspaceTabs::new(theme);
    tabs.set_tabs(
        &[crate::ui::WorkspaceTab {
            name: "A".into(),
            dirty: false,
        }],
        0,
    );
    tabs.set_top(menu.bottom());
    tabs.layout(viewport());
    match tabs.bar_rect() {
        Some(bar) => {
            if (bar.top() - menu.bottom()).abs() > 0.5 {
                failures.push(format!(
                    "标签条应贴菜单栏下沿：菜单 {} / 标签 {}",
                    menu.bottom(),
                    bar.top()
                ));
            }
            if bar.bottom() > options.top() + 0.5 {
                failures.push(format!(
                    "标签条压到了选项栏：标签 {} / 选项栏 {}",
                    bar.bottom(),
                    options.top()
                ));
            }
        }
        None => failures.push("取不到标签条的矩形".to_string()),
    }
}

/// 系统菜单栏接管（macOS 默认）时窗口里没有菜单栏：标签条钉到 `y = 0`，
/// 选项栏接在标签带下面。无头自检画不了系统菜单栏，只检查折叠后的骨架。
pub(crate) fn check_native_chrome_order(failures: &mut Vec<String>) {
    let theme = crate::theme::editor_theme(false);
    let mut view = EditorView::new(theme, AppState::default());
    view.set_tab_band(crate::ui::TAB_STRIP_HEIGHT);
    view.set_menu_bar_visible(false);
    view.layout(viewport());

    if let Some(menu) = view.menu_bar_rect() {
        failures.push(format!("折叠后菜单栏仍报矩形 {:?}", menu));
    }
    let Some(options) = view.options_bar_rect() else {
        failures.push("原生模式下取不到选项栏的矩形".to_string());
        return;
    };
    let band = options.top();
    if (band - crate::ui::TAB_STRIP_HEIGHT).abs() > 0.5 {
        failures.push(format!(
            "原生模式下选项栏应接在标签带 {:.0}px 下面，实际 top = {band:.1}",
            crate::ui::TAB_STRIP_HEIGHT
        ));
    }

    // 宿主按 `menu_bar_rect()` 落空把标签条钉到 0。
    let mut tabs = crate::ui::WorkspaceTabs::new(theme);
    tabs.set_tabs(
        &[crate::ui::WorkspaceTab {
            name: "A".into(),
            dirty: false,
        }],
        0,
    );
    tabs.set_top(0.0);
    tabs.layout(viewport());
    match tabs.bar_rect() {
        Some(bar) => {
            if bar.top().abs() > 0.5 {
                failures.push(format!("原生模式下标签条应贴顶，实际 top = {}", bar.top()));
            }
            if bar.bottom() > options.top() + 0.5 {
                failures.push(format!(
                    "标签条压到了选项栏：标签 {} / 选项栏 {}",
                    bar.bottom(),
                    options.top()
                ));
            }
        }
        None => failures.push("取不到标签条的矩形".to_string()),
    }
}

/// `视图` 菜单：点标题开、菜单项齐全、点「适配窗口」执行并关闭。
pub(crate) fn view_menu(view: &mut EditorView, failures: &mut Vec<String>) {
    // Phase 9：菜单栏 -> 下拉菜单 -> 菜单项动作。
    let view_index = crate::ui::menu::MENUS
        .iter()
        .position(|name| *name == "视图")
        .expect("视图 menu exists");
    match view.menu_center(view_index) {
        Some(center) => {
            click_at(view, center);
            view.update();
            view.layout(viewport());
            let menu_frame = paint_frame(view);
            if !view.menu_open() {
                failures.push("点「视图」后菜单没有打开".to_string());
            }
            if view.open_menu_index() != Some(view_index) {
                failures.push("打开的菜单下标不对".to_string());
            }
            for needle in ["放大", "缩小", "100%", "适配窗口"] {
                if !contains(&menu_frame, needle) {
                    failures.push(format!("视图菜单缺少菜单项：`{needle}`"));
                }
            }
            // 点「适配窗口」：应执行缩放并关闭菜单。
            match text_center(&menu_frame, "适配窗口") {
                Some(item_center) => {
                    click_at(view, item_center);
                    view.update();
                }
                None => failures.push("找不到「适配窗口」菜单项的位置".to_string()),
            }
            if view.menu_open() {
                failures.push("点菜单项后菜单没有关闭".to_string());
            }
        }
        None => failures.push("找不到「视图」菜单标题".to_string()),
    }
}

/// 选项栏：工具名、`大小 +`、像素模式的默认与开关。
pub(crate) fn options_bar(view: &mut EditorView, failures: &mut Vec<String>) {
    // 工具选项栏：切到画笔，点「大小 +」，笔刷应变大；工具名也在帧里。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    view.layout(viewport());
    let options_frame = paint_frame(view);
    if !contains(&options_frame, "画笔工具") {
        failures.push("选项栏缺少当前工具名".to_string());
    }
    let size_before = view.brush_size();
    match view.brush_adjust_center(crate::ui::BrushAdjust::SizeUp) {
        Some(center) => {
            click_at(view, center);
            view.update();
        }
        None => failures.push("找不到选项栏的「大小 +」按钮".to_string()),
    }
    if view.brush_size() <= size_before {
        failures.push(format!(
            "选项栏「{}」没生效",
            crate::ui::BrushAdjust::SizeUp.label()
        ));
    }

    // 像素模式：默认开；选项栏有开关。绘制验证单独用一个新视图，避免污染
    // 主视图的历史（后面还会断言“只有最初那一笔”）。
    if !view.pixel_mode() {
        failures.push("像素模式默认应为开".to_string());
    }
    for needle in ["像素", "方形"] {
        if !contains(&options_frame, needle) {
            failures.push(format!("选项栏缺少「{needle}」开关"));
        }
    }
}

/// 调色盘：色块齐全，点色块与取色器都会改前景色。
pub(crate) fn palette(view: &mut EditorView, failures: &mut Vec<String>) {
    // 调色盘：色块齐全，点一个预设色块应把前景色改掉。
    if view.palette_swatch_count() < 16 {
        failures.push("调色盘色块不足".to_string());
    }
    match view.palette_swatch_center(4) {
        Some(center) => {
            click_at(view, center);
            view.update();
            if view.foreground() != Color::RED {
                failures.push("点调色盘没有把前景色设成红".to_string());
            }
        }
        None => failures.push("找不到调色盘色块".to_string()),
    }

    // 取色器：点方块中心应改前景色。
    match view.palette_picker_center() {
        Some(center) => {
            click_at(view, center);
            view.update();
            let fg = view.foreground();
            if fg == Color::RED || fg == Color::BLACK {
                failures.push("取色器没有改前景色".to_string());
            }
        }
        None => failures.push("找不到取色器".to_string()),
    }
}

/// 右栏 / 调色盘 / 标签页面板的分隔条都能拖动改变尺寸。
pub(crate) fn resizable_panels(view: &mut EditorView, failures: &mut Vec<String>) {
    // 可拖动右栏：向左拖分隔条，右栏应变宽。
    let sidebar_before = view.sidebar_width();
    match view.sidebar_handle_center() {
        Some(start) => {
            let end = start - Vec2::new(40.0, 0.0);
            view.event(&InputEvent::PointerDown {
                position: start,
                button: PointerButton::Left,
            });
            view.event(&InputEvent::PointerMove { position: end });
            view.event(&InputEvent::PointerUp {
                position: end,
                button: PointerButton::Left,
            });
            view.layout(viewport());
        }
        None => failures.push("找不到右栏分隔条".to_string()),
    }
    if view.sidebar_width() <= sidebar_before {
        failures.push("拖分隔条后右栏没变宽".to_string());
    }

    // 左侧调色盘面板也能拖动调宽。
    let palette_before = view.palette_width();
    match view.palette_handle_center() {
        Some(start) => {
            let end = start + Vec2::new(24.0, 0.0);
            view.event(&InputEvent::PointerDown {
                position: start,
                button: PointerButton::Left,
            });
            view.event(&InputEvent::PointerMove { position: end });
            view.event(&InputEvent::PointerUp {
                position: end,
                button: PointerButton::Left,
            });
            view.layout(viewport());
        }
        None => failures.push("找不到调色盘分隔条".to_string()),
    }
    if view.palette_width() <= palette_before {
        failures.push("拖调色盘分隔条没有变宽".to_string());
    }

    // 右侧栏标签页：「文件 / 历史 / 属性」共用一个面板。点标签切换内容，
    // 面板与「图层」之间的分隔条可以拖动调高。
    let tabs_tab = crate::ui::SidebarTab::History;
    if view.active_tab() != crate::ui::SidebarTab::File {
        failures.push("标签页默认应停在「文件」".to_string());
    }
    let tabs_before = view.tabs_height();
    match view.tabs_handle_center() {
        Some(start) => {
            let end = start + Vec2::new(0.0, 24.0);
            view.event(&InputEvent::PointerDown {
                position: start,
                button: PointerButton::Left,
            });
            view.event(&InputEvent::PointerMove { position: end });
            view.event(&InputEvent::PointerUp {
                position: end,
                button: PointerButton::Left,
            });
            view.layout(viewport());
        }
        None => failures.push("找不到标签页分隔条".to_string()),
    }
    if view.tabs_height() <= tabs_before {
        failures.push("拖标签页分隔条没有改变高度".to_string());
    }
    match view.tab_center(tabs_tab) {
        Some(center) => {
            click_at(view, center);
            view.update();
            view.layout(viewport());
        }
        None => failures.push("找不到「历史」标签".to_string()),
    }
    if view.active_tab() != tabs_tab
        || !view.tab_content_visible(tabs_tab)
        || view.tab_content_visible(crate::ui::SidebarTab::File)
        || view.tab_content_visible(crate::ui::SidebarTab::Properties)
    {
        failures.push("点「历史」标签没有切换面板".to_string());
    }
    // 回到「文件」标签再录帧：下面的语义检查要看到文件面板的按钮文字。
    if let Some(center) = view.tab_center(crate::ui::SidebarTab::File) {
        click_at(view, center);
        view.update();
        view.layout(viewport());
    }
}

/// 选项栏高度：切遍所有工具都一样（否则画布跟着上下跳）。
///
/// 单测里有同款断言；这里在自检里再跑一遍，因为布局常量（`OPTIONS_BAR_HEIGHT`）
/// 是「界面对面」的契约，改主题 / 换控件时最容易悄悄破。
pub(crate) fn check_options_bar_height(failures: &mut Vec<String>) {
    let theme = crate::theme::editor_theme(false);
    let mut view = EditorView::new(theme, AppState::default());
    view.set_tab_band(crate::ui::TAB_STRIP_HEIGHT);
    view.layout(viewport());
    let expected = crate::ui::OPTIONS_BAR_HEIGHT;
    let mut bottoms = Vec::new();
    for (key, _tool) in crate::ui::TOOL_KEYS {
        let key = key.to_ascii_lowercase();
        view.event(&InputEvent::KeyDown {
            key: Key::Character(key),
        });
        view.update();
        view.layout(viewport());
        let Some(rect) = view.options_bar_rect() else {
            failures.push("取不到选项栏的矩形".to_string());
            return;
        };
        let height = rect.bottom() - rect.top();
        if (height - expected).abs() > 0.01 {
            failures.push(format!(
                "工具键 {key} 的选项栏高度 {height}，应为 {expected}"
            ));
        }
        bottoms.push(rect.bottom());
    }
    if bottoms
        .windows(2)
        .any(|pair| (pair[0] - pair[1]).abs() > 0.01)
    {
        failures.push(format!("切工具时选项栏下沿在动：{bottoms:?}"));
    }
}

/// 模拟窗口：选项栏的「全部属性…」留下打开意图；窗口能排版 / 绘制 / Esc 关闭；
/// 快捷键表覆盖每个工具键。
pub(crate) fn check_dialogs(failures: &mut Vec<String>) {
    let theme = crate::theme::editor_theme(false);
    let mut view = EditorView::new(theme, AppState::default());
    view.layout(viewport());
    let Some(center) = view.settings_button_center() else {
        failures.push("取不到「全部属性…」按钮".to_string());
        return;
    };
    click_at(&mut view, center);
    view.update();
    match view.take_dialog_request() {
        Some(crate::ui::DialogKind::Settings) => {}
        other => failures.push(format!("「全部属性…」应请求属性窗口，实际 {other:?}")),
    }

    let mut window = crate::ui::DialogWindow::new(
        crate::ui::DialogKind::Settings,
        theme,
        crate::ui::SettingsWindowInput {
            cells: view.option_cells(),
            intents: view.intents(),
        },
        crate::ui::AboutSnapshot::default(),
    );
    window.sync(&view.settings_snapshot());
    window.layout(viewport());
    let frame = paint_window(&window);
    if frame.commands().is_empty() {
        failures.push("属性窗口没有画出任何命令".to_string());
    }
    window.event(&InputEvent::KeyDown { key: Key::Escape });
    if !window.take_close() {
        failures.push("属性窗口按 Esc 没有关闭".to_string());
    }

    let rows = crate::ui::shortcut_rows();
    for (key, tool) in crate::ui::TOOL_KEYS {
        if !rows
            .iter()
            .any(|(row_key, label)| row_key == &key.to_string() && label == tool.label())
        {
            failures.push(format!("快捷键表缺少工具键 {key}（{}）", tool.label()));
        }
    }
}
