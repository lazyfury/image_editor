//! 图层上下文菜单与「图层动作」：树形分组 / 蒙版 / 排序 / 复制 / 删除。
//!
//! 菜单只负责把 [`Intent::LayerAction`] 塞进队列；真正的改动在这里统一执行，
//! 并且**全部走命令**（可撤销）。面板按钮和菜单栏「图层」菜单也复用同一批动作。

use std::collections::HashMap;

use igui_components::{Component, Menu, MenuItem};
use igui_core::Vec2;
use igui_theme::Theme;

use crate::document::{
    AddLayerCommand, Color, Document, GroupCommand, Layer, LayerId, LayerMask, LayerMetaCommand,
    MaskCommand, MoveLayerCommand, MoveTarget, PixelBuffer, RemoveLayerCommand, UngroupCommand,
};

use super::*;

/// 右键菜单 / 菜单栏 / 面板按钮共用的图层动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerAction {
    /// 新建一个栅格图层（放在当前节点上方）。
    NewLayer,
    Rename,
    Duplicate,
    Delete,
    /// 新建一个空组。
    NewGroup,
    /// 把目标节点裹进一个新组。
    Group,
    Ungroup,
    /// 从当前选区创建蒙版。
    AddMask,
    DeleteMask,
    /// 启用 / 停用蒙版。
    ToggleMask,
    /// 显示 / 隐藏节点（组会连带子树）。
    ToggleVisible,
    /// 展开 / 折叠组。
    ToggleExpand,
    InvertMask,
    /// 在「编辑图层像素」和「编辑蒙版」之间切换。
    ToggleEditTarget,
    MoveUp,
    MoveDown,
    Crop,
}

impl EditorView {
    /// 打开某个图层节点的右键菜单。
    pub(super) fn open_layer_context_menu(&mut self, target: LayerId, position: Vec2) {
        let (flags, theme, intents) = {
            let state = self.state.borrow();
            let document = &state.document;
            let Some(layer) = document.layer(target) else {
                return;
            };
            let siblings = document.children(layer.parent);
            let index = siblings.iter().position(|id| *id == target);
            let flags = MenuFlags {
                is_group: layer.is_group(),
                has_mask: layer.mask.is_some(),
                mask_enabled: layer.mask.as_ref().is_some_and(|mask| mask.enabled),
                can_move_up: index.is_some_and(|index| index + 1 < siblings.len()),
                can_move_down: index.is_some_and(|index| index > 0),
                can_ungroup: layer.is_group() && !document.children(Some(target)).is_empty(),
                can_crop: !layer.is_group(),
                has_selection: state.selection.as_ref().is_some_and(|s| !s.is_empty()),
                editing_mask: state.document.active_mask,
            };
            (flags, self.theme, self.intents.clone())
        };

        self.overlays.close_all();
        self.overlays.menu_at(position, move |tree, root| {
            build_layer_menu(theme, &intents, target, flags).build(tree, root);
        });
    }

    /// 执行一个图层动作（面板按钮 / 菜单 / 快捷键都走这里）。
    pub(super) fn apply_layer_action(&mut self, target: LayerId, action: LayerAction) {
        let mut state = self.state.borrow_mut();
        let Some(layer) = state.document.layer(target) else {
            return;
        };
        let is_group = layer.is_group();
        match action {
            LayerAction::NewLayer => {
                let before_active = state.document.active_layer;
                let (width, height) = (state.document.width, state.document.height);
                let (parent, index) = state.document.insertion_slot();
                let name = format!("图层 {}", state.document.layers.len() + 1);
                let layer = Layer::new(name, PixelBuffer::new(width, height));
                state.execute(Box::new(AddLayerCommand::new(
                    layer,
                    parent,
                    index,
                    before_active,
                )));
            }
            LayerAction::Rename => {
                drop(state);
                if let Some(anchor) = self.rename_anchor() {
                    self.open_rename(anchor);
                }
            }
            LayerAction::Duplicate => {
                let nodes = duplicate_subtree(&state.document, target);
                let Some(nodes) = nodes else {
                    return;
                };
                let before_active = state.document.active_layer;
                let (parent, index) = {
                    let layer = state.document.layer(target).expect("目标还在");
                    let siblings = state.document.children(layer.parent);
                    let position = siblings
                        .iter()
                        .position(|id| *id == target)
                        .unwrap_or_default();
                    (layer.parent, position + 1)
                };
                state.execute(Box::new(AddLayerCommand::nodes(
                    nodes,
                    parent,
                    index,
                    before_active,
                    "复制图层",
                )));
            }
            LayerAction::Delete => {
                let before_active = state.document.active_layer;
                state.execute(Box::new(
                    RemoveLayerCommand::new(target, before_active).labeled(if is_group {
                        "删除组"
                    } else {
                        "删除图层"
                    }),
                ));
            }
            LayerAction::NewGroup => {
                let before_active = state.document.active_layer;
                let (parent, index) = {
                    let layer = state.document.layer(target).expect("目标还在");
                    let siblings = state.document.children(layer.parent);
                    let position = siblings
                        .iter()
                        .position(|id| *id == target)
                        .map_or(siblings.len(), |index| index + 1);
                    (layer.parent, position)
                };
                let group = Layer::group("组");
                state.execute(Box::new(AddLayerCommand::new(
                    group,
                    parent,
                    index,
                    before_active,
                )));
            }
            LayerAction::Group => {
                if is_group {
                    return;
                }
                let name = state
                    .document
                    .layer(target)
                    .map(|layer| format!("{} 组", layer.name))
                    .unwrap_or_else(|| "组".to_string());
                let command = GroupCommand::new(&state.document, target, name);
                state.execute(Box::new(command));
            }
            LayerAction::Ungroup => {
                let Some(command) = UngroupCommand::new(&state.document, target) else {
                    return;
                };
                state.execute(Box::new(command));
            }
            LayerAction::AddMask => {
                let selection = state.selection.clone();
                let Some(selection) = selection else {
                    *self.message.borrow_mut() = Some("先建一个选区，再添加蒙版".to_string());
                    return;
                };
                if selection.is_empty() {
                    *self.message.borrow_mut() = Some("选区是空的".to_string());
                    return;
                }
                let before = state.document.layer(target).and_then(|l| l.mask.clone());
                let Some(mask) = mask_from_selection(&state.document, target, &selection) else {
                    return;
                };
                state.execute(Box::new(MaskCommand::new(
                    target,
                    before,
                    Some(mask),
                    "添加蒙版",
                )));
            }
            LayerAction::DeleteMask => {
                let Some(before) = state.document.layer(target).and_then(|l| l.mask.clone()) else {
                    return;
                };
                state.execute(Box::new(MaskCommand::new(
                    target,
                    Some(before),
                    None,
                    "删除蒙版",
                )));
            }
            LayerAction::ToggleMask => {
                let Some(before) = state.document.layer(target).and_then(|l| l.mask.clone()) else {
                    return;
                };
                let mut after = before.clone();
                after.enabled = !after.enabled;
                let label = if before.enabled {
                    "停用蒙版"
                } else {
                    "启用蒙版"
                };
                state.execute(Box::new(MaskCommand::new(
                    target,
                    Some(before),
                    Some(after),
                    label,
                )));
            }
            LayerAction::InvertMask => {
                let Some(before) = state.document.layer(target).and_then(|l| l.mask.clone()) else {
                    return;
                };
                let mut after = before.clone();
                for pixel in after.pixels.data.chunks_exact_mut(4) {
                    let value = 255 - pixel[0];
                    pixel[0] = value;
                    pixel[1] = value;
                    pixel[2] = value;
                    pixel[3] = 255;
                }
                state.execute(Box::new(MaskCommand::new(
                    target,
                    Some(before),
                    Some(after),
                    "反相蒙版",
                )));
            }
            LayerAction::ToggleEditTarget => {
                let editable = state
                    .document
                    .layer(target)
                    .is_some_and(|layer| !layer.is_group() && layer.mask.is_some());
                if editable {
                    let active = !state.document.active_mask;
                    state.document.select_layer(target);
                    state.document.set_active_mask(active);
                    drop(state);
                    self.refresh_layers();
                }
            }
            LayerAction::ToggleVisible => {
                let visible = state
                    .document
                    .layer(target)
                    .is_some_and(|layer| layer.visible);
                let before = LayerMetaCommand::capture(&state.document);
                state.document.set_layer_visible(target, !visible);
                let after = LayerMetaCommand::capture(&state.document);
                state.execute(Box::new(LayerMetaCommand::new(before, after, "显示/隐藏")));
            }
            LayerAction::ToggleExpand => {
                let expanded = state
                    .document
                    .layer(target)
                    .is_some_and(|layer| layer.expanded);
                let changed = state.document.set_expanded(target, !expanded);
                drop(state);
                if changed {
                    self.refresh_layers();
                }
            }
            LayerAction::MoveUp | LayerAction::MoveDown => {
                let delta = if action == LayerAction::MoveUp { 1 } else { -1 };
                let Some(layer) = state.document.layer(target) else {
                    return;
                };
                let siblings = state.document.children(layer.parent);
                let Some(index) = siblings.iter().position(|id| *id == target) else {
                    return;
                };
                let target_index = index as i64 + delta as i64;
                if target_index < 0 || target_index as usize >= siblings.len() {
                    return;
                }
                let command = MoveLayerCommand::new(
                    &state.document,
                    target,
                    MoveTarget::Position(layer.parent, target_index as usize),
                );
                state.execute(Box::new(command));
            }
            LayerAction::Crop => {
                drop(state);
                self.crop_layer_to_document();
            }
        }
    }
}

/// 右键菜单里哪些项目可点。
#[derive(Clone, Copy)]
struct MenuFlags {
    is_group: bool,
    has_mask: bool,
    mask_enabled: bool,
    can_move_up: bool,
    can_move_down: bool,
    can_ungroup: bool,
    can_crop: bool,
    has_selection: bool,
    editing_mask: bool,
}

/// 把右键菜单搭出来（与菜单栏「图层」是同一批动作）。
fn build_layer_menu(
    theme: &'static dyn Theme,
    intents: &Intents,
    target: LayerId,
    flags: MenuFlags,
) -> Menu {
    let mut menu = Menu::new(theme);
    menu = layer_entry(
        menu,
        theme,
        intents,
        target,
        "重命名",
        Some("F2"),
        LayerAction::Rename,
        true,
    );
    menu = layer_entry(
        menu,
        theme,
        intents,
        target,
        "复制图层",
        Some("Cmd+J"),
        LayerAction::Duplicate,
        true,
    );
    menu = layer_entry(
        menu,
        theme,
        intents,
        target,
        "删除",
        Some("Cmd+⌫"),
        LayerAction::Delete,
        true,
    );
    menu = menu.separator();
    menu = layer_entry(
        menu,
        theme,
        intents,
        target,
        "编组",
        Some("Cmd+G"),
        LayerAction::Group,
        !flags.is_group,
    );
    menu = layer_entry(
        menu,
        theme,
        intents,
        target,
        "取消编组",
        Some("Shift+Cmd+G"),
        LayerAction::Ungroup,
        flags.can_ungroup,
    );
    menu = layer_entry(
        menu,
        theme,
        intents,
        target,
        "新建空组",
        None,
        LayerAction::NewGroup,
        true,
    );
    menu = menu.separator();
    menu = layer_entry(
        menu,
        theme,
        intents,
        target,
        "添加蒙版（选区）",
        None,
        LayerAction::AddMask,
        flags.has_selection && !flags.has_mask,
    );
    menu = layer_entry(
        menu,
        theme,
        intents,
        target,
        if flags.mask_enabled {
            "停用蒙版"
        } else {
            "启用蒙版"
        },
        None,
        LayerAction::ToggleMask,
        flags.has_mask,
    );
    menu = layer_entry(
        menu,
        theme,
        intents,
        target,
        "反相蒙版",
        None,
        LayerAction::InvertMask,
        flags.has_mask,
    );
    menu = layer_entry(
        menu,
        theme,
        intents,
        target,
        "删除蒙版",
        None,
        LayerAction::DeleteMask,
        flags.has_mask,
    );
    menu = layer_entry(
        menu,
        theme,
        intents,
        target,
        if flags.editing_mask {
            "编辑图层像素"
        } else {
            "编辑蒙版"
        },
        None,
        LayerAction::ToggleEditTarget,
        flags.has_mask && !flags.is_group,
    );
    menu = menu.separator();
    menu = layer_entry(
        menu,
        theme,
        intents,
        target,
        "上移一层",
        None,
        LayerAction::MoveUp,
        flags.can_move_up,
    );
    menu = layer_entry(
        menu,
        theme,
        intents,
        target,
        "下移一层",
        None,
        LayerAction::MoveDown,
        flags.can_move_down,
    );
    layer_entry(
        menu,
        theme,
        intents,
        target,
        "裁到文档",
        None,
        LayerAction::Crop,
        flags.can_crop,
    )
}

#[allow(clippy::too_many_arguments)]
fn layer_entry(
    menu: Menu,
    theme: &'static dyn Theme,
    intents: &Intents,
    target: LayerId,
    label: &str,
    shortcut: Option<&str>,
    action: LayerAction,
    enabled: bool,
) -> Menu {
    let intents = intents.clone();
    let mut item = MenuItem::new(label, theme);
    if let Some(shortcut) = shortcut {
        item = item.shortcut(shortcut);
    }
    menu.item(item.disabled(!enabled).on_click(move |_tree, _id| {
        intents.push(Intent::LayerAction { target, action });
    }))
}

/// 复制一棵子树（新 id，parent 映射到副本），返回按前序 DFS 排好的节点。
fn duplicate_subtree(document: &Document, id: LayerId) -> Option<Vec<Layer>> {
    let range = document.subtree_range(id)?;
    let mut ids: HashMap<LayerId, LayerId> = HashMap::new();
    let mut copies = Vec::with_capacity(range.len());
    for layer in &document.layers[range] {
        let mut copy = layer.clone();
        copy.id = LayerId::next();
        ids.insert(layer.id, copy.id);
        copies.push(copy);
    }
    for copy in &mut copies {
        if let Some(parent) = copy.parent {
            copy.parent = ids.get(&parent).copied().or(Some(parent));
        }
    }
    Some(copies)
}

/// 用选区（文档坐标）给节点造一张蒙版。
fn mask_from_selection(
    document: &Document,
    id: LayerId,
    selection: &crate::canvas::Selection,
) -> Option<LayerMask> {
    let layer = document.layer(id)?;
    let (width, height, offset) = if layer.is_group() {
        (document.width, document.height, (0, 0))
    } else {
        (
            layer.pixels.width,
            layer.pixels.height,
            (layer.position.x, layer.position.y),
        )
    };
    let mut pixels = PixelBuffer::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let doc_x = x as i64 + offset.0 as i64;
            let doc_y = y as i64 + offset.1 as i64;
            let coverage = if doc_x < 0 || doc_y < 0 {
                0.0
            } else {
                selection.coverage(doc_x as u32, doc_y as u32)
            };
            let value = (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
            pixels.set_pixel(x, y, Color::new(value, value, value, 255));
        }
    }
    Some(LayerMask {
        pixels,
        enabled: true,
    })
}

/// 便捷：不带菜单的 `LayerAction`（面板按钮 / 快捷键用）。
impl EditorView {
    /// 对当前节点执行一个图层动作（菜单栏 / 宿主快捷键用）。
    pub fn layer_action(&mut self, action: LayerAction) {
        let Some(target) = self.state.borrow().document.active_layer else {
            return;
        };
        self.apply_layer_action(target, action);
    }
}
