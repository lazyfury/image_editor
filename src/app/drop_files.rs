//! 拖放文件：`igui_winit` 不转发 `winit` 的拖放事件，宿主自己看。
//!
//! `WinitPlugin` 把原生事件原样交给 `PlatformObserver`；这里 downcast 回
//! `winit::event::WindowEvent`，把 `DroppedFile` 的路径塞进一个共享队列。
//! [`ImageEditorLogic`](crate::app::application) 每帧取走，当成一个新图层导入
//! （没有打开的工作区时，先按图片尺寸开一个新文档）。
//!
//! 拖放需要在同一个 `winit` 版本上 downcast，所以 `Cargo.toml` 直接依赖
//! `winit`（和 `igui_winit` 用的版本一致）。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use igui_app::{AppBuilder, PlatformEvent, PlatformObserver, Plugin};
use igui_core::InputEvent;

/// 拖放进来的文件路径队列（宿主每帧取走）。
pub struct DroppedFiles(pub Rc<RefCell<Vec<PathBuf>>>);

/// 注册 [`DroppedFiles`] 服务并观察 `winit` 的拖放事件。
pub struct DropFilesPlugin;

impl Plugin for DropFilesPlugin {
    fn name(&self) -> &'static str {
        "image-editor-drop-files"
    }

    fn build(&self, app: &mut AppBuilder) {
        let shared = Rc::new(RefCell::new(Vec::new()));
        app.insert_service(DroppedFiles(shared.clone()));
        app.add_platform_observer(DropObserver { shared });
    }
}

struct DropObserver {
    shared: Rc<RefCell<Vec<PathBuf>>>,
}

impl PlatformObserver for DropObserver {
    fn on_platform(&mut self, event: PlatformEvent<'_>, _out: &mut Vec<InputEvent>) {
        if let Some(winit::event::WindowEvent::DroppedFile(path)) =
            event.downcast_ref::<winit::event::WindowEvent>()
        {
            self.shared.borrow_mut().push(path.clone());
        }
    }
}
