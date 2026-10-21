//! macOS 系统菜单栏：`NSApplication.mainMenu` + `NSMenu` / `NSMenuItem`。
//!
//! 手写 AppKit FFI（和 `io/dialog/macos.rs` 同一套风格：`objc2*` 已在依赖树里，
//! 不为一个菜单引 `muda` / `tray-icon`）。四条平台规则：
//!
//! 1. **主线程**：`NSApp` 只能在主线程碰 —— 拿 `MainThreadMarker` 当证明，拿不到
//!    就返回 `None`（继续用自绘菜单），绝不在错误线程上崩。
//! 2. **`setTarget` 是弱引用**：`NSMenuItem` 不持有 target，所以 [`NativeMenu`]
//!    自己留一份 `Retained<MenuTarget>`。
//! 3. **`setAutoenablesItems(false)`**：不然 AppKit 沿响应链自己算启用态，我们的
//!    `setEnabled` 不起作用。
//! 4. **回调没有 `&mut`**：菜单项的动作发生在 AppKit 的事件分发里，只能把
//!    `MenuAction` 塞进静态队列，等宿主每帧 [`NativeMenu::take_actions`] 取走。
//!
//! 快捷键这一版**不设** key equivalent（除了 App 菜单的隐藏 / 退出）：继续走
//! `app/application/shortcuts.rs`，否则 `Cmd+A` 会被菜单抢走，重命名输入框里的
//! 全选就失效了（见 `todo.md` §1.aa）。

use std::cell::RefCell;
use std::sync::{Mutex, OnceLock};

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::runtime::NSObjectProtocol;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSMenu, NSMenuItem,
};
use objc2_foundation::NSString;

use crate::app::native_menu::Wake;
use crate::ui::menu::{menu_spec, MenuAction, MenuEntry, MenuState, MENUS};

/// 系统菜单栏里的那一套菜单（App 菜单 + [`MENUS`] 的八个下拉）。
pub struct NativeMenu {
    /// 菜单项的 target 是弱引用；留着它，回调目标才不会被释放。
    _target: Retained<MenuTarget>,
    /// 每个下拉里建出来的项，顺序与 `menu_spec` 一一对应（含分隔线）。
    items: Vec<Vec<Retained<NSMenuItem>>>,
    /// 上一次同步过的状态；没变就不碰 AppKit。
    state: MenuState,
}

impl NativeMenu {
    /// 装到系统菜单栏。拿不到主线程（或在无头环境里没有 `NSApp`）返回 `None`。
    pub fn install(state: MenuState, wake: Option<Wake>) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let app = NSApplication::sharedApplication(mtm);
        let target = MenuTarget::new(mtm);

        let main = NSMenu::new(mtm);
        main.setAutoenablesItems(false);
        // 接管主菜单后 winit 默认那套（退出 / 隐藏）就没了，自己补回来。
        main.addItem(&application_item(mtm, &app));

        // `tag` -> 动作：`NSMenuItem` 只存得下一个整数，表放在这里。
        let mut table = Vec::new();
        let mut items = Vec::new();
        for (index, title) in MENUS.iter().enumerate() {
            let menu = NSMenu::new(mtm);
            menu.setTitle(&NSString::from_str(title));
            menu.setAutoenablesItems(false);
            let mut row = Vec::new();
            for entry in menu_spec(index, state) {
                let item = match entry {
                    MenuEntry::Separator => NSMenuItem::separatorItem(mtm),
                    MenuEntry::Item { label, checked, .. } => {
                        let item = NSMenuItem::new(mtm);
                        item.setTitle(&NSString::from_str(label));
                        item.setEnabled(entry.enabled());
                        set_checked(&item, checked);
                        if let Some(action) = entry.action() {
                            // `tag` 只是 registry 的下标，动作表在闭包里查。
                            item.setTag(table.len() as isize);
                            table.push(action);
                            // SAFETY: target 活在本结构里，选择器就是下面那个方法。
                            unsafe {
                                item.setTarget(Some(&target));
                                item.setAction(Some(sel!(menuAction:)));
                            }
                        }
                        item
                    }
                };
                menu.addItem(&item);
                row.push(item);
            }
            let parent = NSMenuItem::new(mtm);
            parent.setTitle(&NSString::from_str(title));
            parent.setSubmenu(Some(&menu));
            main.addItem(&parent);
            items.push(row);
        }

        *registry().lock().ok()? = table;
        // AppKit 的回调里没有别的上下文，叫醒事件循环的闭包放线程局部。
        WAKE.with(|slot| *slot.borrow_mut() = wake);
        app.setMainMenu(Some(&main));
        tracing::info!(
            target: "image_editor",
            menus = items.len(),
            items = items.iter().map(Vec::len).sum::<usize>(),
            "native_menu_installed"
        );
        Some(Self {
            _target: target,
            items,
            state,
        })
    }

    /// 可点 / 勾选跟着文档状态走（[`MenuState`] 没变就直接返回）。
    pub fn sync(&mut self, state: MenuState) {
        if state == self.state {
            return;
        }
        self.state = state;
        for (index, row) in self.items.iter().enumerate() {
            for (entry, item) in menu_spec(index, state).iter().zip(row) {
                let MenuEntry::Item { checked, .. } = entry else {
                    continue;
                };
                item.setEnabled(entry.enabled());
                set_checked(item, *checked);
            }
        }
    }

    /// 取走用户在系统菜单里点的动作（宿主每帧调用）。
    pub fn take_actions(&mut self) -> Vec<MenuAction> {
        match pending().lock() {
            Ok(mut queue) => std::mem::take(&mut *queue),
            Err(_) => Vec::new(),
        }
    }
}

/// 勾选态（`✓`）只能用 `setState`，`MenuItem` 那边是靠标题前缀画的。
fn set_checked(item: &NSMenuItem, checked: bool) {
    item.setState(if checked {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
}

/// App 菜单（隐藏 / 退出）：接管主菜单后这两项得自己补。
fn application_item(mtm: MainThreadMarker, app: &NSApplication) -> Retained<NSMenuItem> {
    let menu = NSMenu::new(mtm);
    menu.setTitle(&NSString::from_str("image_editor"));
    menu.setAutoenablesItems(false);
    for (label, key, selector) in [
        ("隐藏 image_editor", "h", sel!(hide:)),
        ("退出 image_editor", "q", sel!(terminate:)),
    ] {
        let item = NSMenuItem::new(mtm);
        item.setTitle(&NSString::from_str(label));
        item.setKeyEquivalent(&NSString::from_str(key));
        // SAFETY: 选择器来自 `NSApplication`（AppKit 的标准动作），target 是 NSApp。
        unsafe {
            item.setTarget(Some(app));
            item.setAction(Some(selector));
        }
        menu.addItem(&item);
    }
    let parent = NSMenuItem::new(mtm);
    parent.setTitle(&NSString::from_str("image_editor"));
    parent.setSubmenu(Some(&menu));
    parent
}

thread_local! {
    /// 菜单动作入队后叫醒事件循环；[`NativeMenu::install`] 装进来。
    static WAKE: RefCell<Option<Wake>> = const { RefCell::new(None) };
}

/// `tag` -> 动作的表（`NSMenuItem` 的 tag 只是一个整数）。
fn registry() -> &'static Mutex<Vec<MenuAction>> {
    static REGISTRY: OnceLock<Mutex<Vec<MenuAction>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

/// 叫醒事件循环（`ControlFlow::Wait` 下不给事件就不会有下一帧）。
///
/// 先复制出闭包再调：不要在持有 `RefCell` 借用时跑宿主的代码。
fn wake_event_loop() {
    let wake = WAKE.with(|slot| slot.borrow().clone());
    if let Some(wake) = wake {
        wake();
    }
}

/// 系统菜单里点下来的动作，等宿主取走（回调里没有 `&mut AppLogic`）。
fn pending() -> &'static Mutex<Vec<MenuAction>> {
    static PENDING: OnceLock<Mutex<Vec<MenuAction>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(Vec::new()))
}

define_class!(
    // SAFETY: NSObject 没有子类化要求，这个类也不实现 `Drop`。
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[name = "ImageEditorMenuTarget"]
    struct MenuTarget;

    // SAFETY: NSObjectProtocol 没有安全要求。
    unsafe impl NSObjectProtocol for MenuTarget {}

    impl MenuTarget {
        /// 所有菜单项共用的动作：`tag` 是 [`registry`] 里的下标。
        // SAFETY: 签名与 `NSMenuItem` 的 target/action 约定一致。
        #[unsafe(method(menuAction:))]
        fn menu_action(&self, sender: &NSMenuItem) {
            let index = usize::try_from(sender.tag()).ok();
            let action = index.and_then(|index| {
                registry()
                    .lock()
                    .ok()
                    .and_then(|table| table.get(index).copied())
            });
            if let Some(action) = action {
                if let Ok(mut queue) = pending().lock() {
                    queue.push(action);
                }
                // 动作已经进队：叫醒事件循环，宿主下一帧就会取走并执行。
                wake_event_loop();
            }
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSObject 的 `init` 签名正确（见 objc2 的 define_class 示例）。
        unsafe { msg_send![super(this), init] }
    }
}
