//! 菜单栏：一排菜单标题 + 真实的下拉菜单。
//!
//! 标题点击只往意图队列里塞一个 [`Intent::OpenMenu`]，真正的下拉由
//! [`EditorView`](crate::ui::EditorView) 在 `update` 里用
//! [`Overlays::menu`](igui_components::Overlays::menu) 打开 —— 回调拿不到
//! `&mut self`，所以走意图队列。菜单项再塞 [`Intent::MenuAction`]，由 `update`
//! 统一执行（这也是关闭菜单的时机）。
//!
//! 已实现的动作接真实命令（撤销 / 重做 / 导入导出 / 缩放 / 取消选区 / 关于），
//! 其余保留带标注的占位项，遵守仓库的 placeholder policy。

use igui_components::{Button, Component, Flex, Menu, MenuItem, NodeRef};
use igui_core::Edges;
use igui_scene::SceneTree;
use igui_theme::{space, SurfaceLevel, TextSize, Theme};
use igui_ui::MouseFilter;

use super::{Intent, Intents};

/// 菜单栏标题，从左到右。
pub const MENUS: [&str; 8] = [
    "文件", "编辑", "图像", "图层", "选择", "滤镜", "视图", "帮助",
];

/// 下拉菜单里一个可执行的动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    Undo,
    Redo,
    Import,
    Export,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ZoomFit,
    /// 全选（整个文档）。
    SelectAll,
    /// 反选（没有选区时等于全选）。
    InvertSelection,
    /// 羽化当前选区（半径固定 `FEATHER_RADIUS`）。
    FeatherSelection,
    /// 剪切选区内容。
    Cut,
    /// 复制选区内容。
    Copy,
    /// 粘贴剪贴板。
    Paste,
    /// 粘贴为新图层（不动当前图层）。
    PasteAsLayer,
    ClearSelection,
    /// 把当前图层裁到文档大小（丢掉画布外像素）。
    CropLayerToDocument,
    /// 切换透明棋盘格背景。
    ToggleCheckerboard,
    About,
    /// 尚未实现的菜单项：只把提示写进状态栏。
    Placeholder(&'static str),
}

/// 一条带占位提示的菜单栏。点标题会往 `intents` 里塞 [`Intent::OpenMenu`]。
pub fn menu_bar(
    theme: &'static dyn Theme,
    intents: Intents,
    refs: &mut Vec<NodeRef>,
    bar_slot: &NodeRef,
) -> impl Component {
    let mut bar = Flex::row()
        .gap(space::XXS)
        .padding(Edges::symmetric(space::SM, space::XXS))
        .background(theme.surface(SurfaceLevel::Surface))
        .mouse_filter(MouseFilter::Ignore);
    for (index, name) in MENUS.iter().enumerate() {
        let slot = NodeRef::new();
        refs.push(slot.clone());
        let intents = intents.clone();
        bar = bar.child(
            Button::ghost(*name, theme)
                .font_size(TextSize::Small.px())
                .on_click(move |_tree, _id| intents.push(Intent::OpenMenu(index)))
                .ref_(&slot),
        );
    }
    bar.ref_(bar_slot)
}

/// 菜单项要用的四个状态位（决定禁用 / 可点）。
#[derive(Debug, Clone, Copy)]
pub struct MenuState {
    pub can_undo: bool,
    pub can_redo: bool,
    pub has_selection: bool,
    pub can_paste: bool,
}

/// 构建第 `index` 个菜单的内容，挂到 `root`（覆盖层给的定位节点）下。
///
/// 每个菜单一个函数（[`file_menu`] … [`help_menu`]），这里只做下标 -> 函数的分发。
pub fn menu_content(
    tree: &mut SceneTree,
    root: igui_core::NodeId,
    theme: &'static dyn Theme,
    index: usize,
    intents: Intents,
    state: MenuState,
) {
    let menu = match index {
        0 => file_menu(theme, &intents),
        1 => edit_menu(theme, &intents, state),
        2 => image_menu(theme, &intents),
        3 => layer_menu(theme, &intents),
        4 => select_menu(theme, &intents, state),
        5 => filter_menu(theme, &intents),
        6 => view_menu(theme, &intents),
        _ => help_menu(theme, &intents),
    };
    menu.build(tree, root);
}

/// 「文件」菜单。
fn file_menu(theme: &'static dyn Theme, intents: &Intents) -> Menu {
    Menu::new(theme)
        .item(placeholder("新建…", theme, intents, "新建：后续 Phase"))
        .item(placeholder(
            "打开…",
            theme,
            intents,
            "打开：Phase 10 文件浏览器",
        ))
        .separator()
        .item(menu_item(
            "导入 PNG…",
            None,
            theme,
            intents,
            MenuAction::Import,
        ))
        .item(menu_item(
            "导出 PNG…",
            None,
            theme,
            intents,
            MenuAction::Export,
        ))
}

/// 「编辑」菜单。
fn edit_menu(theme: &'static dyn Theme, intents: &Intents, state: MenuState) -> Menu {
    Menu::new(theme)
        .item(
            menu_item("撤销", Some("Ctrl+Z"), theme, intents, MenuAction::Undo)
                .disabled(!state.can_undo),
        )
        .item(
            menu_item(
                "重做",
                Some("Shift+Ctrl+Z"),
                theme,
                intents,
                MenuAction::Redo,
            )
            .disabled(!state.can_redo),
        )
        .separator()
        .item(
            menu_item("剪切", Some("Ctrl+X"), theme, intents, MenuAction::Cut)
                .disabled(!state.has_selection),
        )
        .item(
            menu_item("复制", Some("Ctrl+C"), theme, intents, MenuAction::Copy)
                .disabled(!state.has_selection),
        )
        .item(
            menu_item("粘贴", Some("Ctrl+V"), theme, intents, MenuAction::Paste)
                .disabled(!state.can_paste),
        )
        .item(
            menu_item(
                "粘贴为新图层",
                Some("Shift+Ctrl+V"),
                theme,
                intents,
                MenuAction::PasteAsLayer,
            )
            .disabled(!state.can_paste),
        )
}

/// 「图像」菜单。
fn image_menu(theme: &'static dyn Theme, intents: &Intents) -> Menu {
    Menu::new(theme)
        .item(placeholder(
            "图像大小…",
            theme,
            intents,
            "图像大小：后续 Phase",
        ))
        .item(placeholder("裁剪", theme, intents, "裁剪：后续 Phase"))
        .separator()
        .item(placeholder(
            "水平翻转",
            theme,
            intents,
            "水平翻转：后续 Phase",
        ))
        .item(placeholder(
            "垂直翻转",
            theme,
            intents,
            "垂直翻转：后续 Phase",
        ))
        .item(placeholder("旋转 90°", theme, intents, "旋转：后续 Phase"))
}

/// 「图层」菜单。
fn layer_menu(theme: &'static dyn Theme, intents: &Intents) -> Menu {
    Menu::new(theme)
        .item(placeholder(
            "新建图层",
            theme,
            intents,
            "新建图层：用右侧图层面板",
        ))
        .item(placeholder(
            "复制图层",
            theme,
            intents,
            "复制图层：后续 Phase",
        ))
        .separator()
        .item(placeholder(
            "上移一层",
            theme,
            intents,
            "上移一层：用右侧图层面板",
        ))
        .item(placeholder(
            "下移一层",
            theme,
            intents,
            "下移一层：用右侧图层面板",
        ))
        .separator()
        .item(menu_item(
            "裁到文档",
            None,
            theme,
            intents,
            MenuAction::CropLayerToDocument,
        ))
}

/// 「选择」菜单。
fn select_menu(theme: &'static dyn Theme, intents: &Intents, state: MenuState) -> Menu {
    Menu::new(theme)
        .item(menu_item(
            "全选",
            Some("Ctrl+A"),
            theme,
            intents,
            MenuAction::SelectAll,
        ))
        .item(menu_item(
            "反选",
            Some("Shift+Ctrl+I"),
            theme,
            intents,
            MenuAction::InvertSelection,
        ))
        .item(
            menu_item("羽化", None, theme, intents, MenuAction::FeatherSelection)
                .disabled(!state.has_selection),
        )
        .item(
            menu_item(
                "取消选择",
                Some("Esc"),
                theme,
                intents,
                MenuAction::ClearSelection,
            )
            .disabled(!state.has_selection),
        )
}

/// 「滤镜」菜单。
fn filter_menu(theme: &'static dyn Theme, intents: &Intents) -> Menu {
    Menu::new(theme)
        .item(placeholder("灰度", theme, intents, "灰度：后续 Phase"))
        .item(placeholder("反相", theme, intents, "反相：后续 Phase"))
        .item(placeholder("高斯模糊", theme, intents, "模糊：后续 Phase"))
}

/// 「视图」菜单。
fn view_menu(theme: &'static dyn Theme, intents: &Intents) -> Menu {
    Menu::new(theme)
        .item(menu_item(
            "放大",
            Some("+"),
            theme,
            intents,
            MenuAction::ZoomIn,
        ))
        .item(menu_item(
            "缩小",
            Some("-"),
            theme,
            intents,
            MenuAction::ZoomOut,
        ))
        .item(menu_item(
            "100%",
            Some("0"),
            theme,
            intents,
            MenuAction::ZoomReset,
        ))
        .item(menu_item(
            "适配窗口",
            Some("F"),
            theme,
            intents,
            MenuAction::ZoomFit,
        ))
        .separator()
        .item(menu_item(
            "显示棋盘格",
            None,
            theme,
            intents,
            MenuAction::ToggleCheckerboard,
        ))
}

/// 「帮助」菜单。
fn help_menu(theme: &'static dyn Theme, intents: &Intents) -> Menu {
    Menu::new(theme)
        .item(menu_item(
            "关于 image_editor",
            None,
            theme,
            intents,
            MenuAction::About,
        ))
        .separator()
        .item(placeholder(
            "快捷键…",
            theme,
            intents,
            "快捷键：见窗口标题栏 / README",
        ))
}

fn menu_item(
    label: &str,
    shortcut: Option<&str>,
    theme: &'static dyn Theme,
    intents: &Intents,
    value: MenuAction,
) -> MenuItem {
    let mut item = MenuItem::new(label, theme);
    if let Some(shortcut) = shortcut {
        item = item.shortcut(shortcut);
    }
    let intents = intents.clone();
    item.on_click(move |_tree, _id| intents.push(Intent::MenuAction(value)))
}

fn placeholder(
    label: &str,
    theme: &'static dyn Theme,
    intents: &Intents,
    note: &'static str,
) -> MenuItem {
    menu_item(label, None, theme, intents, MenuAction::Placeholder(note))
}

/// 关于对话框 / 状态栏用的版本串。
pub fn about_text() -> String {
    format!("image_editor · 用 igui UI 栈绘制 · {} 个菜单", MENUS.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_bar_has_eight_distinct_titles() {
        assert_eq!(MENUS.len(), 8);
        let mut titles = MENUS.to_vec();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(titles.len(), MENUS.len());
    }

    #[test]
    fn about_text_mentions_the_menu_count() {
        assert!(about_text().contains("8"));
    }
}
