//! 非 macOS：没有系统菜单栏（Windows / Linux 的菜单在窗口里，归自绘那一套）。
//!
//! [`NativeMenu::install`] 恒返回 `None`，宿主于是保留自绘菜单 —— 这个类型只为
//! 让宿主的调用点与平台无关而存在。

use crate::app::native_menu::Wake;
use crate::ui::menu::{MenuAction, MenuState};

/// 系统菜单栏（这个平台没有）。
pub struct NativeMenu;

impl NativeMenu {
    /// 这个平台没有系统菜单栏：`None` = 继续用自绘菜单。
    pub fn install(_state: MenuState, _wake: Option<Wake>) -> Option<Self> {
        None
    }

    /// 同步可点 / 勾选状态（这个平台什么都不做）。
    pub fn sync(&mut self, _state: MenuState) {}

    /// 取走用户在系统菜单里点的动作（这个平台没有菜单，永远空）。
    pub fn take_actions(&mut self) -> Vec<MenuAction> {
        Vec::new()
    }
}
