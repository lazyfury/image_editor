//! 原生菜单栏：把编辑器的菜单放进 **macOS 的系统菜单栏**（`NSApp.mainMenu`）。
//!
//! 默认开（`Options::native_menu`，命令行 `--ui-menu` 关掉切回自绘）；其它平台与
//! 无头自检没有实现，[`NativeMenu::install`] 返回 `None`，宿主继续画
//! `crate::ui::menu` 的那一行。
//!
//! **一张表两处用**：菜单内容来自 [`crate::ui::menu::menu_spec`]（标题 / 快捷键 /
//! 禁用 / 勾选 / 动作），所以「原生」和「自绘」只有画法不同，不会漂。
//!
//! 宿主每帧做两件事：
//!
//! - `sync(editor.menu_state())`：可点 / 勾选跟着文档状态走（签名没变就不碰
//!   AppKit）；
//! - `take_actions()`：用户在系统菜单里点的动作取回来，转发给当前工作区
//!   （[`EditorView::apply_menu_action`](crate::ui::EditorView)）。没有工作区时
//!   整张菜单是灰的（[`crate::ui::menu::MenuState::none`]），点了也没人接。
//!
//! **入队后必须叫醒事件循环**（[`Wake`]）：`igui_winit` 跑的是
//! `ControlFlow::Wait`，只有 winit 事件才 `request_redraw`；系统菜单的动作发生在
//! AppKit 的 run loop 里，不给它一个事件，界面要等下一次鼠标移动才刷新 ——
//! 看起来就是「点了菜单没反应，动一下才变」。宿主在装的时候把一个
//! `window.request_redraw()` 的闭包交进来。
//!
//! 这是「UI 全部由 igui 绘制」的一个明确例外：系统菜单栏不在窗口里，画不了。

pub use platform::NativeMenu;

/// 菜单动作入队后叫醒事件循环（见上）；宿主装菜单时交进来，平台层自己调。
pub type Wake = std::rc::Rc<dyn Fn()>;

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;

#[cfg(not(target_os = "macos"))]
#[path = "other.rs"]
mod platform;
