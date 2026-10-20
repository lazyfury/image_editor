//! 菜单栏下拉 + 工具栏长按浮层 + 渐变编辑器的开关与动作分发。
//!
//! `update` 轮询这里的请求（[`EditorView::open_menu`] / `poll_long_press`），
//! `event` 命中标题时调 [`EditorView::menu_index_at`]。

use igui_components::{Component, Menu, MenuItem, Placement};
use igui_core::{NodeId, Vec2};

use crate::tools::brush::preset;

use super::*;

impl EditorView {
    /// 命中位置落在哪个菜单标题上（含其子节点）；用于一次点击切换菜单。
    pub(super) fn menu_index_at(&self, position: Vec2) -> Option<usize> {
        let hit = igui_ui::hit_test(&self.tree, position)?;
        let mut current = Some(hit);
        while let Some(node) = current {
            if let Some(index) = self.menu_nodes.iter().position(|id| *id == node) {
                return Some(index);
            }
            current = self.tree.parent(node);
        }
        None
    }

    /// 打开第 `index` 个菜单的下拉，锚在对应标题按钮下。
    pub(super) fn open_menu(&mut self, index: usize) {
        let Some(&anchor) = self.menu_nodes.get(index) else {
            return;
        };
        self.overlays.close_all();
        self.open_tool_group.set(None);
        let theme = self.theme;
        let action = self.menu_action.clone();
        let (can_undo, can_redo, has_selection) = {
            let state = self.state.borrow();
            (
                state.can_undo(),
                state.can_redo(),
                state.selection.is_some(),
            )
        };
        let can_paste = self.has_clipboard();
        self.overlays.menu(anchor, move |tree, root| {
            menu::menu_content(
                tree,
                root,
                theme,
                index,
                action.clone(),
                can_undo,
                can_redo,
                has_selection,
                can_paste,
            );
        });
        self.open_menu.set(Some(index));
    }

    /// 打开选项栏的笔刷预设下拉（锚在「预设」触发按钮下）。
    ///
    /// 当前参数命中的预设前面打「✓」；选中只写请求格，`update` 应用并关闭下拉
    /// （和菜单栏一个套路）。
    pub(super) fn open_brush_presets(&mut self, anchor: NodeId) {
        self.overlays.close_all();
        self.open_menu.set(None);
        self.open_tool_group.set(None);
        let theme = self.theme;
        let request = self.brush_preset_request.clone();
        let current = preset::matching(&self.brush);
        self.overlays.menu(anchor, move |tree, root| {
            let mut menu = Menu::new(theme);
            for (index, item) in preset::PRESETS.iter().enumerate() {
                let request = request.clone();
                let checked = if current == Some(index) { "✓" } else { " " };
                menu = menu.item(
                    MenuItem::new(format!("{checked} {}", item.name), theme)
                        .shortcut(format!("{:.0}px · {:.0}%", item.size, item.opacity * 100.0))
                        .on_click(move |_tree, _id| request.set(Some(index))),
                );
            }
            menu.build(tree, root);
        });
    }

    /// 长按到阈值就展开对应组的工具浮层。
    pub(super) fn poll_long_press(&mut self) {
        let group = {
            let state = self.long_press.borrow();
            match &*state {
                toolbar::LongPress::Pending {
                    group,
                    start,
                    moved,
                    ..
                } if !*moved && start.elapsed() >= self.long_press_threshold => Some(*group),
                _ => None,
            }
        };
        if let Some(group) = group {
            *self.long_press.borrow_mut() = toolbar::LongPress::Fired;
            self.open_tool_flyout(group);
        }
    }

    /// 展开某个组的工具浮层（向右锚在组按钮上）。
    pub(super) fn open_tool_flyout(&mut self, group: usize) {
        let Some(&anchor) = self.tool_group_nodes.get(group) else {
            return;
        };
        self.overlays.close_all();
        self.open_menu.set(None);
        let theme = self.theme;
        let state = self.state.clone();
        let icons = self.icons.clone();
        let request = self.tool_request.clone();
        self.overlays
            .popover(anchor, Placement::Right, move |tree, root| {
                toolbar::tool_flyout_content(
                    tree,
                    root,
                    theme,
                    group,
                    state.clone(),
                    icons.clone(),
                    request.clone(),
                );
            });
        self.open_tool_group.set(Some(group));
    }

    /// 打开多节点渐变编辑器（popover 锚在选项栏的预览条下）。
    pub(super) fn open_gradient_editor(&mut self) {
        self.overlays.close_all();
        self.open_menu.set(None);
        self.open_tool_group.set(None);
        let theme = self.theme;
        let state = self.state.clone();
        let gradient = self.gradient.clone();
        let customized = self.gradient_customized.clone();
        let selected = self.gradient_selected.clone();
        let refs = gradient_editor::GradientEditorRefs::default();
        let anchor = self.gradient_preview_node;
        self.overlays
            .popover(anchor, Placement::Below, move |tree, root| {
                gradient_editor::gradient_editor_content(
                    tree,
                    root,
                    theme,
                    state.clone(),
                    gradient.clone(),
                    customized.clone(),
                    selected.clone(),
                    &refs,
                );
            });
        self.gradient_editor_open.set(true);
    }

    /// 当前工具属于哪个组，就把那个组的「脸」更新成它；标签跟着改。
    pub(super) fn sync_tool_faces(&mut self) {
        let active = self.state.borrow().active_tool;
        for (index, group) in TOOL_GROUPS.iter().enumerate() {
            if group.contains(active) {
                if let Some(face) = self.tool_faces.get(index) {
                    face.set(active);
                }
            }
            if let (Some(face), Some(&label)) =
                (self.tool_faces.get(index), self.tool_face_labels.get(index))
            {
                set_text(&mut self.tree, label, face.get().short_label());
            }
        }
    }

    /// 执行一个菜单动作（[`EditorView::update`] 在关闭菜单后调用）。
    pub(super) fn apply_menu_action(&mut self, action: menu::MenuAction) {
        match action {
            menu::MenuAction::Undo => {
                self.undo();
            }
            menu::MenuAction::Redo => {
                self.redo();
            }
            menu::MenuAction::Import => self.file_request.set(Some(FileRequest::ImportPng)),
            menu::MenuAction::Export => self.file_request.set(Some(FileRequest::ExportPng)),
            menu::MenuAction::ZoomIn => self.apply_canvas_action(CanvasAction::ZoomIn),
            menu::MenuAction::ZoomOut => self.apply_canvas_action(CanvasAction::ZoomOut),
            menu::MenuAction::ZoomReset => self.apply_canvas_action(CanvasAction::Reset),
            menu::MenuAction::ZoomFit => self.apply_canvas_action(CanvasAction::Fit),
            menu::MenuAction::SelectAll => {
                self.select_all();
            }
            menu::MenuAction::InvertSelection => {
                self.invert_selection();
            }
            menu::MenuAction::FeatherSelection => {
                self.feather_selection();
            }
            menu::MenuAction::Cut => {
                self.cut_selection();
            }
            menu::MenuAction::Copy => {
                self.copy_selection();
            }
            menu::MenuAction::Paste => {
                self.paste_clipboard();
            }
            menu::MenuAction::ClearSelection => {
                self.clear_selection();
            }
            menu::MenuAction::CropLayerToDocument => self.crop_layer_to_document(),
            menu::MenuAction::ToggleCheckerboard => {
                self.show_checkerboard = !self.show_checkerboard;
                self.texture_dirty = true;
                *self.message.borrow_mut() = Some(if self.show_checkerboard {
                    "已显示棋盘格".to_string()
                } else {
                    "已隐藏棋盘格".to_string()
                });
            }
            menu::MenuAction::About => {
                *self.message.borrow_mut() = Some(menu::about_text());
            }
            menu::MenuAction::Placeholder(note) => {
                *self.message.borrow_mut() = Some(note.to_string());
            }
        }
    }

    /// 把当前图层裁到文档大小（丢弃画布外像素，回收缓冲区）。
    pub(super) fn crop_layer_to_document(&mut self) {
        let mut state = self.state.borrow_mut();
        let Some(layer) = state.document.active_layer() else {
            *self.message.borrow_mut() = Some("没有可裁剪的图层".to_string());
            return;
        };
        if layer.position == crate::document::Point::ZERO
            && layer.pixels.width == state.document.width
            && layer.pixels.height == state.document.height
        {
            *self.message.borrow_mut() = Some("图层已经在文档范围内".to_string());
            return;
        }
        let id = layer.id;
        let before = layer.pixels.clone();
        let position = layer.position;
        state.execute(Box::new(CropLayerCommand::new(id, before, position)));
        self.texture_dirty = true;
        *self.message.borrow_mut() = Some("已裁到文档大小".to_string());
    }
}
