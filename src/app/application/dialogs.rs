//! 模拟窗口（属性 / 关于 / 快捷键）的宿主侧接线。
//!
//! 打开请求走编辑器的意图队列：菜单项（`MenuAction::Settings` / `About` /
//! `Shortcuts`）和选项栏末尾的「全部属性…」都只塞一个
//! [`Intent::Dialog`](crate::ui::Intent)，宿主每帧在 `update` 末尾取走并建窗口
//! （见 [`ImageEditorLogic::open_popup_if_requested`]）。
//!
//! 同一时刻只开一个：有窗口时输入全交给它（[`ImageEditorLogic::event`] 里在模态
//! 之前转发），Esc / 关闭按钮 / 页脚「关闭」/ 点窗口外都关掉它。窗口开着时系统
//! 菜单栏整张置灰（`native_menu_state`），和「新建文档」模态一个规矩。

use super::*;
use crate::app::tool_groups::TOOL_GROUPS;
use crate::ui::{AboutSnapshot, DialogWindow, SettingsSnapshot, SettingsWindowInput};

/// 「关于」里显示的 igui release tag。`Cargo.toml` 里 pin 的就是这个；升级 tag
/// 时这里跟着改（`release.md` 的流程）。
const IGUI_TAG: &str = "v0.3.0";

impl ImageEditorLogic {
    /// 打开编辑器请求的模拟窗口（每帧一次，取到就建）。
    pub(super) fn open_popup_if_requested(&mut self) {
        if self.popup.is_some() {
            return;
        }
        let Some(workspace) = self.workspaces.active() else {
            return;
        };
        let Some(kind) = workspace.editor.take_dialog_request() else {
            return;
        };
        let theme = self.theme;
        let settings = SettingsWindowInput {
            cells: workspace.editor.option_cells(),
            intents: workspace.editor.intents(),
        };
        let about = self.about_snapshot();
        let mut dialog = DialogWindow::new(kind, theme, settings, about);
        if let Some(measurer) = self.measurer.clone() {
            dialog.set_text_measurer(measurer);
        }
        tracing::info!(
            target: "image_editor",
            kind = ?kind,
            "dialog_opened"
        );
        self.popup = Some(dialog);
    }

    /// 「关于」窗口的内容（打开时取一次：文档尺寸 / 图层数都是那一刻的）。
    fn about_snapshot(&self) -> AboutSnapshot {
        let (document, layers) = match self.workspaces.active() {
            Some(workspace) => (
                Some(workspace.editor.document_size()),
                workspace.editor.layer_count(),
            ),
            None => (None, 0),
        };
        let (menu_items, placeholders) = crate::ui::menu::item_counts();
        AboutSnapshot {
            version: env!("CARGO_PKG_VERSION").to_string(),
            igui: IGUI_TAG.to_string(),
            font: if crate::fonts::bundled_face().is_some() {
                "京華老宋体（随包）".to_string()
            } else {
                "系统字体".to_string()
            },
            document,
            layers,
            tools: TOOL_GROUPS.iter().map(|group| group.tools.len()).sum(),
            menu_items,
            placeholders,
        }
    }

    /// 转发一个事件给模拟窗口；它说要关就关（Esc / 关闭按钮 / 点窗口外）。
    pub(super) fn popup_event(&mut self, event: &InputEvent) -> bool {
        let Some(dialog) = self.popup.as_mut() else {
            return false;
        };
        dialog.event(event);
        if dialog.take_close() {
            self.popup = None;
        }
        true
    }

    /// 每帧把属性值同步给窗口（只有属性窗口有内容要刷）。
    pub(super) fn sync_popup(&mut self) {
        let Some(dialog) = self.popup.as_mut() else {
            return;
        };
        let snapshot: SettingsSnapshot = match self.workspaces.active() {
            Some(workspace) => workspace.editor.settings_snapshot(),
            None => SettingsSnapshot::default(),
        };
        dialog.sync(&snapshot);
    }

    /// 工作区关了 / 回主页时窗口跟着关：它显示的是那个工作区的属性。
    pub(super) fn close_popup(&mut self) {
        self.popup = None;
    }
}
