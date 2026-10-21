//! 菜单栏：一排菜单标题 + 真实的下拉菜单。
//!
//! **一张表两处用**：下拉里有什么由 [`menu_spec`] 给（标题 / 快捷键 / 禁用 /
//! 勾选 / 动作），自绘下拉（[`menu_content`]）和 macOS 原生菜单栏
//! （`crate::app::native_menu`）都从它生成 —— 加一条菜单只改这张表，两边不会漂。
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
    /// 打开「属性」窗口（完整的工具属性）。
    Settings,
    /// 打开「快捷键」窗口。
    Shortcuts,
    About,
    /// 尚未实现的菜单项：只把提示写进状态栏。
    Placeholder(&'static str),
}

/// 下拉里的一条，和「怎么画」无关：自绘下拉与原生菜单栏都读它。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEntry {
    /// 一条可点 / 置灰的项。
    Item {
        label: &'static str,
        /// 自绘下拉显示的快捷键提示。原生菜单栏这一版**不设** key equivalent
        /// （见 `todo.md` §1.aa：快捷键继续走 `shortcuts.rs`，`Cmd+A` 不能被
        /// 菜单抢走，否则重命名输入框里全选失效）。
        shortcut: Option<&'static str>,
        action: MenuAction,
        enabled: bool,
        /// 前面打 `✓`（原生菜单栏用 `setState`）。
        checked: bool,
    },
    /// 分隔线。
    Separator,
}

impl MenuEntry {
    /// 这一条要执行的动作（分隔线没有）。
    pub fn action(&self) -> Option<MenuAction> {
        match self {
            Self::Item { action, .. } => Some(*action),
            Self::Separator => None,
        }
    }

    /// 自绘下拉里显示的文字：勾选时前面加 `✓ `（`MenuItem` 没有勾选态）。
    pub fn label(&self) -> String {
        match self {
            Self::Item {
                label,
                checked: true,
                ..
            } => format!("✓ {label}"),
            Self::Item { label, .. } => (*label).to_string(),
            Self::Separator => String::new(),
        }
    }

    /// 是否可点（分隔线恒为不可点）。
    pub fn enabled(&self) -> bool {
        matches!(self, Self::Item { enabled: true, .. })
    }
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

/// 菜单项要用的状态位（决定禁用 / 勾选）。
///
/// 自绘下拉在编辑器里调用时 `has_document` 恒为 `true`；原生菜单栏在主页
/// （没有工作区）也显示，这时整张菜单置灰 —— 否则点了也没人接。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuState {
    /// 有没有打开的文档（宿主没有工作区时为 `false`，整张菜单置灰）。
    pub has_document: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub has_selection: bool,
    pub can_paste: bool,
    /// 「视图 → 显示棋盘格」的勾选。
    pub checkerboard: bool,
}

impl MenuState {
    /// 没有文档：全部置灰（原生菜单栏在主页用）。
    pub const fn none() -> Self {
        Self {
            has_document: false,
            can_undo: false,
            can_redo: false,
            has_selection: false,
            can_paste: false,
            checkerboard: false,
        }
    }
}

/// 菜单项总数与其中的占位项数（「关于」窗口与能力梳理用）。
///
/// 状态取「有文档、什么都能点」，所以只看表里有什么，不看当前能不能点。
pub fn item_counts() -> (usize, usize) {
    let state = MenuState {
        has_document: true,
        can_undo: true,
        can_redo: true,
        has_selection: true,
        can_paste: true,
        checkerboard: false,
    };
    let mut items = 0;
    let mut placeholders = 0;
    for index in 0..MENUS.len() {
        for entry in menu_spec(index, state) {
            if let MenuEntry::Item { action, .. } = entry {
                items += 1;
                if matches!(action, MenuAction::Placeholder(_)) {
                    placeholders += 1;
                }
            }
        }
    }
    (items, placeholders)
}

/// 第 `index` 个菜单的内容（和画法无关）。
///
/// 每个菜单一个函数（[`file_menu`] … [`help_menu`]），这里只做下标 -> 函数的分发。
/// `has_document == false` 时每一条都置灰（见 [`MenuState`]）。
pub fn menu_spec(index: usize, state: MenuState) -> Vec<MenuEntry> {
    match index {
        0 => file_menu(state),
        1 => edit_menu(state),
        2 => image_menu(state),
        3 => layer_menu(state),
        4 => select_menu(state),
        5 => filter_menu(state),
        6 => view_menu(state),
        _ => help_menu(state),
    }
}

/// 构建第 `index` 个菜单的内容，挂到 `root`（覆盖层给的定位节点）下。
pub fn menu_content(
    tree: &mut SceneTree,
    root: igui_core::NodeId,
    theme: &'static dyn Theme,
    index: usize,
    intents: Intents,
    state: MenuState,
) {
    build_menu(theme, &intents, &menu_spec(index, state)).build(tree, root);
}

/// 把一张表变成 igui 的下拉（每个 `Item` 点一下塞 [`Intent::MenuAction`]）。
fn build_menu(theme: &'static dyn Theme, intents: &Intents, entries: &[MenuEntry]) -> Menu {
    let mut menu = Menu::new(theme);
    for entry in entries {
        menu = match entry {
            MenuEntry::Separator => menu.separator(),
            MenuEntry::Item {
                shortcut,
                action,
                enabled,
                ..
            } => {
                let mut item = MenuItem::new(entry.label(), theme);
                if let Some(shortcut) = shortcut {
                    item = item.shortcut(*shortcut);
                }
                let intents = intents.clone();
                let action = *action;
                menu.item(
                    item.disabled(!enabled)
                        .on_click(move |_tree, _id| intents.push(Intent::MenuAction(action))),
                )
            }
        };
    }
    menu
}

/// 一条菜单项；`enabled` 会再与「有没有文档」相与（见 [`MenuState`]）。
fn item(
    state: MenuState,
    label: &'static str,
    shortcut: Option<&'static str>,
    action: MenuAction,
    enabled: bool,
    checked: bool,
) -> MenuEntry {
    MenuEntry::Item {
        label,
        shortcut,
        action,
        enabled: state.has_document && enabled,
        checked,
    }
}

fn separator() -> MenuEntry {
    MenuEntry::Separator
}

/// 还没实现的菜单项：点了把提示写进状态栏。
fn placeholder(state: MenuState, label: &'static str, note: &'static str) -> MenuEntry {
    item(
        state,
        label,
        None,
        MenuAction::Placeholder(note),
        true,
        false,
    )
}

/// 「文件」菜单。
fn file_menu(state: MenuState) -> Vec<MenuEntry> {
    vec![
        placeholder(state, "新建…", "新建：后续 Phase"),
        placeholder(state, "打开…", "打开：Phase 10 文件浏览器"),
        separator(),
        item(state, "导入 PNG…", None, MenuAction::Import, true, false),
        item(state, "导出 PNG…", None, MenuAction::Export, true, false),
    ]
}

/// 「编辑」菜单。
fn edit_menu(state: MenuState) -> Vec<MenuEntry> {
    vec![
        item(
            state,
            "撤销",
            Some("Ctrl+Z"),
            MenuAction::Undo,
            state.can_undo,
            false,
        ),
        item(
            state,
            "重做",
            Some("Shift+Ctrl+Z"),
            MenuAction::Redo,
            state.can_redo,
            false,
        ),
        separator(),
        item(
            state,
            "剪切",
            Some("Ctrl+X"),
            MenuAction::Cut,
            state.has_selection,
            false,
        ),
        item(
            state,
            "复制",
            Some("Ctrl+C"),
            MenuAction::Copy,
            state.has_selection,
            false,
        ),
        item(
            state,
            "粘贴",
            Some("Ctrl+V"),
            MenuAction::Paste,
            state.can_paste,
            false,
        ),
        item(
            state,
            "粘贴为新图层",
            Some("Shift+Ctrl+V"),
            MenuAction::PasteAsLayer,
            state.can_paste,
            false,
        ),
        separator(),
        // 完整的工具属性：选项栏只放得下紧凑子集，塞不下的在这里（见 `ui/dialogs`）。
        item(state, "首选项…", None, MenuAction::Settings, true, false),
    ]
}

/// 「图像」菜单。
fn image_menu(state: MenuState) -> Vec<MenuEntry> {
    vec![
        placeholder(state, "图像大小…", "图像大小：后续 Phase"),
        placeholder(state, "裁剪", "裁剪：后续 Phase"),
        separator(),
        placeholder(state, "水平翻转", "水平翻转：后续 Phase"),
        placeholder(state, "垂直翻转", "垂直翻转：后续 Phase"),
        placeholder(state, "旋转 90°", "旋转：后续 Phase"),
    ]
}

/// 「图层」菜单。
fn layer_menu(state: MenuState) -> Vec<MenuEntry> {
    vec![
        placeholder(state, "新建图层", "新建图层：用右侧图层面板"),
        placeholder(state, "复制图层", "复制图层：后续 Phase"),
        separator(),
        placeholder(state, "上移一层", "上移一层：用右侧图层面板"),
        placeholder(state, "下移一层", "下移一层：用右侧图层面板"),
        separator(),
        item(
            state,
            "裁到文档",
            None,
            MenuAction::CropLayerToDocument,
            true,
            false,
        ),
    ]
}

/// 「选择」菜单。
fn select_menu(state: MenuState) -> Vec<MenuEntry> {
    vec![
        item(
            state,
            "全选",
            Some("Ctrl+A"),
            MenuAction::SelectAll,
            true,
            false,
        ),
        item(
            state,
            "反选",
            Some("Shift+Ctrl+I"),
            MenuAction::InvertSelection,
            true,
            false,
        ),
        item(
            state,
            "羽化",
            None,
            MenuAction::FeatherSelection,
            state.has_selection,
            false,
        ),
        item(
            state,
            "取消选择",
            Some("Esc"),
            MenuAction::ClearSelection,
            state.has_selection,
            false,
        ),
    ]
}

/// 「滤镜」菜单。
fn filter_menu(state: MenuState) -> Vec<MenuEntry> {
    vec![
        placeholder(state, "灰度", "灰度：后续 Phase"),
        placeholder(state, "反相", "反相：后续 Phase"),
        placeholder(state, "高斯模糊", "模糊：后续 Phase"),
    ]
}

/// 「视图」菜单。
fn view_menu(state: MenuState) -> Vec<MenuEntry> {
    vec![
        item(state, "放大", Some("+"), MenuAction::ZoomIn, true, false),
        item(state, "缩小", Some("-"), MenuAction::ZoomOut, true, false),
        item(state, "100%", Some("0"), MenuAction::ZoomReset, true, false),
        item(
            state,
            "适配窗口",
            Some("F"),
            MenuAction::ZoomFit,
            true,
            false,
        ),
        separator(),
        item(
            state,
            "显示棋盘格",
            None,
            MenuAction::ToggleCheckerboard,
            true,
            state.checkerboard,
        ),
    ]
}

/// 「帮助」菜单。
fn help_menu(state: MenuState) -> Vec<MenuEntry> {
    vec![
        item(
            state,
            "关于 image_editor",
            None,
            MenuAction::About,
            true,
            false,
        ),
        separator(),
        item(state, "快捷键…", None, MenuAction::Shortcuts, true, false),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document_state() -> MenuState {
        MenuState {
            has_document: true,
            can_undo: true,
            can_redo: false,
            has_selection: false,
            can_paste: false,
            checkerboard: false,
        }
    }

    fn entries_with(menus: &[usize], state: MenuState) -> Vec<MenuEntry> {
        menus
            .iter()
            .flat_map(|index| menu_spec(*index, state))
            .collect()
    }

    fn find(menus: &[usize], state: MenuState, action: MenuAction) -> MenuEntry {
        entries_with(menus, state)
            .into_iter()
            .find(|entry| entry.action() == Some(action))
            .expect("菜单里有这一项")
    }

    #[test]
    fn the_menu_bar_has_eight_distinct_titles() {
        assert_eq!(MENUS.len(), 8);
        let mut titles = MENUS.to_vec();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(titles.len(), MENUS.len());
    }

    #[test]
    fn every_menu_has_a_spec() {
        for index in 0..MENUS.len() {
            assert!(
                !menu_spec(index, document_state()).is_empty(),
                "第 {index} 个菜单是空的"
            );
        }
    }

    /// 撤销 / 重做 / 剪切 / 羽化的可点性跟着文档状态走（自绘与原生共用这张表）。
    #[test]
    fn history_and_selection_actions_follow_the_menu_state() {
        let empty = MenuState {
            can_undo: false,
            ..document_state()
        };
        assert!(!find(&[1], empty, MenuAction::Undo).enabled());
        assert!(find(&[1], document_state(), MenuAction::Undo).enabled());
        // 没有选区时「剪切」「羽化」「取消选择」都置灰。
        assert!(!find(&[1, 4], document_state(), MenuAction::Cut).enabled());
        assert!(!find(&[1, 4], document_state(), MenuAction::FeatherSelection).enabled());
        // 撤销 / 重做是两个独立状态位：撤销不可用不影响重做。
        assert!(!find(&[1], empty, MenuAction::Redo).enabled());
        let redo = MenuState {
            can_redo: true,
            ..empty
        };
        assert!(find(&[1], redo, MenuAction::Redo).enabled());
    }

    /// 主页（没有文档）时整张菜单置灰：原生菜单栏那时候也显示。
    #[test]
    fn every_entry_is_greyed_out_without_a_document() {
        for index in 0..MENUS.len() {
            for entry in menu_spec(index, MenuState::none()) {
                if entry == MenuEntry::Separator {
                    continue;
                }
                assert!(!entry.enabled(), "第 {index} 个菜单仍有可点项：{entry:?}");
            }
        }
    }

    /// 棋盘格开关带勾选：自绘下拉显示 `✓ `，原生菜单栏用 `setState`。
    #[test]
    fn the_checkerboard_entry_is_checked_while_the_background_is_on() {
        let on = MenuState {
            checkerboard: true,
            ..document_state()
        };
        let entry = find(&[6], on, MenuAction::ToggleCheckerboard);
        assert_eq!(
            entry,
            MenuEntry::Item {
                label: "显示棋盘格",
                shortcut: None,
                action: MenuAction::ToggleCheckerboard,
                enabled: true,
                checked: true,
            }
        );
        assert_eq!(entry.label(), "✓ 显示棋盘格");
        assert!(
            !find(&[6], document_state(), MenuAction::ToggleCheckerboard)
                .label()
                .contains('✓')
        );
    }
}
