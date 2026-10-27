//! 宿主侧光标同步：把当前指针位置 / 工具 / 控件映射成 winit 光标，并设给窗口。
//!
//! 映射规则在 [`crate::app::cursor`]（纯函数、可单测）；这里只负责按帧算
//! “当前该显示什么”并在变化时调用 `window.set_cursor`。没有窗口（headless
//! 自检）时什么都不做。

use winit::window::CursorIcon;

use crate::app::cursor;

use super::{ImageEditorLogic, Screen};

impl ImageEditorLogic {
    /// 每帧调用一次：光标变了才 `set_cursor`。
    pub(super) fn sync_cursor(&mut self) {
        // 先确认窗口已经发布：没有窗口时不要写缓存，否则窗口就绪后
        // 同一个 desired 会被缓存挡掉，光标永远不生效。
        let Some(window) = self
            .window
            .as_ref()
            .and_then(|window| window.borrow().clone())
        else {
            return;
        };
        let desired = self.desired_cursor();
        if self.last_cursor == Some(desired) {
            return;
        }
        self.last_cursor = Some(desired);
        window.set_cursor(desired);
    }

    /// 当前该显示的系统光标。
    ///
    /// 优先级：模态 / 确认框 / 模拟窗口（它们有自己的树）> 主页 > 编辑器。
    /// 编辑器里：菜单打开 → 默认；平移中 → Grabbing（拖出画布也算）；标签条 /
    /// 画布 / 各面板分别用各自的 hover 结果。
    pub(super) fn desired_cursor(&self) -> CursorIcon {
        if let Some(modal) = self.modal.as_ref() {
            return cursor::igui(modal.hovered_cursor());
        }
        if let Some(confirm) = self.confirm.as_ref() {
            return cursor::igui(confirm.hovered_cursor());
        }
        if let Some(popup) = self.popup.as_ref() {
            return cursor::igui(popup.hovered_cursor());
        }

        match &self.screen {
            Screen::Home(home) => cursor::igui(home.hovered_cursor()),
            Screen::Editor => {
                let Some(workspace) = self.workspaces.active() else {
                    return CursorIcon::Default;
                };
                let editor = &workspace.editor;
                if editor.menu_open() {
                    return CursorIcon::Default;
                }
                // 平移中即使指针被拖出画布，也保持“抓紧”的手。
                if editor.panning() {
                    return CursorIcon::Grabbing;
                }
                if let Some(position) = self.pointer {
                    if self
                        .tabs
                        .bar_rect()
                        .is_some_and(|bar| bar.contains(position))
                    {
                        return cursor::igui(self.tabs.hovered_cursor());
                    }
                }
                if editor.pointer_over_canvas() {
                    return cursor::canvas(
                        editor.active_tool(),
                        false,
                        editor.space_held(),
                        self.modifiers.alt,
                    );
                }
                cursor::igui(editor.hovered_cursor())
            }
        }
    }
}
