//! 菜单栏的宿主侧接线：装 macOS 系统菜单栏（[`crate::app::native_menu`]）、
//! 每帧同步可点 / 勾选、把点下来的动作转给当前工作区，并折叠窗口里那一行。
//!
//! 两条路只有「谁点」不同：
//!
//! - 自绘下拉：菜单项塞 [`Intent::MenuAction`](crate::ui::Intent)，由
//!   `EditorView::update` 执行；
//! - 系统菜单栏：菜单项把动作塞进静态队列（AppKit 的回调里没有 `&mut`），
//!   宿主在 [`ImageEditorLogic::update`] 开头取走再转发。
//!
//! 两者执行的是同一个 [`EditorView::apply_menu_action`](crate::ui::EditorView)，
//! 菜单内容也来自同一张表（[`crate::ui::menu::menu_spec`]）。

use super::*;
use crate::ui::menu::{MenuAction, MenuState};

impl ImageEditorLogic {
    /// 装系统菜单栏（只在实际开窗的运行时里；无头自检直接建视图，不走这里）。
    ///
    /// 拿不到主线程 / 这个平台没有时 [`NativeMenu::install`] 返回 `None`，窗口里
    /// 继续画自绘菜单，行为完全不变。
    pub(super) fn install_native_menu(&mut self, wake: Option<crate::app::native_menu::Wake>) {
        if !self.native_menu_wanted {
            return;
        }
        // 起始状态按「没有文档」装：翻到编辑器后第一帧 `sync_native_menu` 会改。
        self.native_menu = NativeMenu::install(MenuState::none(), wake);
    }

    /// 系统菜单栏此刻该是什么状态。
    ///
    /// 模态窗口（新建文档 / 未保存确认）和模拟窗口（属性 / 关于 / 快捷键）开着时
    /// 整张置灰：自绘下拉会被窗口吞掉输入，系统菜单栏如果还能点，就等于绕过窗口
    /// 去改下面的文档。
    pub(super) fn native_menu_state(&self) -> MenuState {
        if self.modal.is_some() || self.confirm.is_some() || self.popup.is_some() {
            return MenuState::none();
        }
        match self.screen {
            Screen::Editor => self
                .workspaces
                .active()
                .map_or_else(MenuState::none, |workspace| workspace.editor.menu_state()),
            Screen::Home(_) => MenuState::none(),
        }
    }

    /// 宿主每帧：可点 / 勾选跟着当前工作区走；主页（没有工作区）整张置灰。
    pub(super) fn sync_native_menu(&mut self) {
        let state = self.native_menu_state();
        if let Some(menu) = self.native_menu.as_mut() {
            menu.sync(state);
        }
    }

    /// 取走系统菜单里点下来的动作，按到达顺序转发给当前工作区。
    pub(super) fn handle_native_menu_actions(&mut self) {
        let Some(menu) = self.native_menu.as_mut() else {
            return;
        };
        for action in menu.take_actions() {
            self.apply_menu_action(action);
        }
    }

    /// 把菜单动作交给当前工作区执行；没有工作区（主页）时菜单本来就是灰的，
    /// 这里丢掉即可。
    pub(super) fn apply_menu_action(&mut self, action: MenuAction) {
        let Some(workspace) = self.workspaces.active_mut() else {
            return;
        };
        workspace.editor.apply_menu_action(action);
        // 动作可能改了图层 / 文档 / 标题，下一帧把标题写回原生窗口。
        self.title_dirty = true;
    }
}
