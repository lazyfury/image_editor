//! 图层面板：树形列表（缩进 / 折叠 / 蒙版）+ 作用在「当前节点」的按钮。
//!
//! 行数据来自 [`Document::rows`]（前序 DFS -> 从上到下的显示顺序，折叠的组不带
//! 子节点），面板持有一份缓存，`refresh_layers()` 里重建一次，`List` 的行源只按
//! 下标读。右键菜单走 `List::on_context` + [`EditorView::open_layer_context_menu`]。
//!
//! 顶部的眼睛 / 不透明度是列表的列；**操作按钮作用于“当前图层”**（先在列表里
//! 选中一行）。这是刻意的：`List` 的行是文本单元，没有逐格点击，把动作放到
//! 选中对象上既符合编辑器习惯，也不用给列表加第二套交互。
//!
//! 所有按钮回调只写共享的 [`AppState`] 或往意图队列里塞 [`Intent::LayerAction`]，
//! 由 [`EditorView::update`](crate::ui::EditorView) 统一执行并重合成。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use igui_components::{Button, Column, Component, Divider, Flex, List, ListColumn, Text};
use igui_core::{Edges, NodeId, Vec2};
use igui_scene::SceneTree;
use igui_theme::{space, TextSize, Theme, Tone};
use igui_ui::MouseFilter;

use crate::app::state::AppState;
use crate::document::{AddLayerCommand, Document, Layer, LayerMetaCommand, LayerRow, PixelBuffer};

use crate::ui::card::Card;

use super::{Intent, Intents, LayerAction};

/// 图层行高（逻辑像素）。列表的池大小按它算。
pub const LAYER_ROW_HEIGHT: f32 = 28.0;
/// 每级缩进的逻辑像素。
const INDENT: f32 = 12.0;

/// 图层列表：第 0 行是**最上面的节点**（跟 Photoshop 一致），数据来自行缓存。
#[allow(clippy::too_many_arguments)]
pub fn layer_list(
    theme: &'static dyn Theme,
    state: Rc<RefCell<AppState>>,
    rows: Rc<RefCell<Vec<LayerRow>>>,
    count: Rc<Cell<usize>>,
    selected: Rc<Cell<Option<usize>>>,
    intents: Intents,
) -> List {
    let source = {
        let rows = rows.clone();
        move |index: usize| {
            let rows = rows.borrow();
            let Some(row) = rows.get(index) else {
                return Vec::new();
            };
            let eye = if row.visible { "◉" } else { "○" };
            let mut name = String::new();
            if row.is_group {
                name.push_str(if row.expanded { "▾ " } else { "▸ " });
            }
            if row.has_mask {
                name.push_str(if row.mask_enabled { "◧ " } else { "◨ " });
            }
            name.push_str(&row.name);
            vec![
                eye.to_string(),
                name,
                format!("{:.0}%", row.opacity * 100.0),
            ]
        }
    };

    let indent = {
        let rows = rows.clone();
        move |index: usize| {
            rows.borrow()
                .get(index)
                .map_or(0.0, |row| row.depth as f32 * INDENT)
        }
    };

    let activate = {
        let state = state.clone();
        let rows = rows.clone();
        move |index: usize| {
            let Some(id) = rows.borrow().get(index).map(|row| row.id) else {
                return;
            };
            state.borrow_mut().document.select_layer(id);
        }
    };

    let context = {
        let rows = rows.clone();
        move |index: usize, position: Vec2| {
            if let Some(id) = rows.borrow().get(index).map(|row| row.id) {
                intents.push(Intent::LayerMenu {
                    target: id,
                    position,
                });
            }
        }
    };

    List::new(theme, LAYER_ROW_HEIGHT, source)
        .spacer(indent)
        .columns(vec![
            ListColumn::fixed(22.0),
            ListColumn::flexible(),
            ListColumn::fixed(44.0).tone(Tone::Muted),
        ])
        .count(count)
        .selected(selected)
        .on_activate(activate)
        .on_context(context)
        .grow(1.0)
}

/// 完整图层面板：标题 + 列表 + 操作按钮。`list` 由 [`EditorView`] 构建，
/// 因为它需要 `ListState` 句柄。
///
/// [`EditorView`]: crate::ui::EditorView
pub fn layer_panel(
    theme: &'static dyn Theme,
    state: Rc<RefCell<AppState>>,
    list: List,
    intents: Intents,
) -> impl Component {
    Card::new(theme)
        .gap(space::SM)
        .grow(1.0)
        .child(Text::subheading("图层", theme))
        .child(Divider::horizontal(theme))
        .child(list)
        .child(Divider::horizontal(theme))
        .child(action_buttons(theme, state, intents))
}

/// 两排按钮，全部作用于“当前节点”。树操作走 [`Intent::LayerAction`]（可撤销）。
fn action_buttons(
    theme: &'static dyn Theme,
    state: Rc<RefCell<AppState>>,
    intents: Intents,
) -> impl Component {
    let add = {
        let state = state.clone();
        button(theme, "+ 图层", move |_tree, _id| {
            let mut state = state.borrow_mut();
            let count = state.document.layers.len();
            let before_active = state.document.active_layer;
            let (width, height) = (state.document.width, state.document.height);
            let (parent, index) = state.document.insertion_slot();
            let layer = Layer::new(
                format!("图层 {}", count + 1),
                PixelBuffer::new(width, height),
            );
            state.execute(Box::new(AddLayerCommand::new(
                layer,
                parent,
                index,
                before_active,
            )));
        })
    };
    let add_group = {
        let intents = intents.clone();
        action_button(theme, "+ 组", state.clone(), intents, LayerAction::NewGroup)
    };
    let delete = action_button(
        theme,
        "− 删除",
        state.clone(),
        intents.clone(),
        LayerAction::Delete,
    );
    let rename = {
        let intents = intents.clone();
        button(theme, "重命名", move |_tree, id| {
            intents.push(Intent::RenameLayer(id))
        })
    };
    let toggle = {
        let state = state.clone();
        button(theme, "显示/隐藏", move |_tree, _id| {
            let mut state = state.borrow_mut();
            let Some(id) = state.document.active_layer else {
                return;
            };
            let visible = state
                .document
                .layer(id)
                .map(|layer| layer.visible)
                .unwrap_or(true);
            meta_edit(&mut state, "显示/隐藏", |document| {
                document.set_layer_visible(id, !visible);
            });
        })
    };
    let less = {
        let state = state.clone();
        button(theme, "−", move |_tree, _id| {
            let mut state = state.borrow_mut();
            let Some(id) = state.document.active_layer else {
                return;
            };
            let opacity = state
                .document
                .layer(id)
                .map(|layer| layer.opacity)
                .unwrap_or(1.0);
            meta_edit(&mut state, "不透明度", |document| {
                document.set_layer_opacity(id, opacity - 0.1);
            });
        })
    };
    let more = {
        let state = state.clone();
        button(theme, "+", move |_tree, _id| {
            let mut state = state.borrow_mut();
            let Some(id) = state.document.active_layer else {
                return;
            };
            let opacity = state
                .document
                .layer(id)
                .map(|layer| layer.opacity)
                .unwrap_or(1.0);
            meta_edit(&mut state, "不透明度", |document| {
                document.set_layer_opacity(id, opacity + 0.1);
            });
        })
    };
    let up = action_button(
        theme,
        "↑",
        state.clone(),
        intents.clone(),
        LayerAction::MoveUp,
    );
    let down = action_button(theme, "↓", state.clone(), intents, LayerAction::MoveDown);

    Column::new()
        .gap(space::XXS)
        .child(
            Flex::row()
                .gap(space::XXS)
                .padding(Edges::ZERO)
                .mouse_filter(MouseFilter::Ignore)
                .child(add)
                .child(add_group)
                .child(delete)
                .child(rename),
        )
        .child(
            Flex::row()
                .gap(space::XXS)
                .padding(Edges::ZERO)
                .mouse_filter(MouseFilter::Ignore)
                .child(toggle)
                .child(less)
                .child(more)
                .child(up)
                .child(down),
        )
}

/// 作用在当前节点上的按钮：点一下塞一个 [`Intent::LayerAction`]。
fn action_button(
    theme: &'static dyn Theme,
    label: &'static str,
    state: Rc<RefCell<AppState>>,
    intents: Intents,
    action: LayerAction,
) -> Button {
    button(theme, label, move |_tree, _id| {
        let target = state.borrow().document.active_layer;
        if let Some(target) = target {
            intents.push(Intent::LayerAction { target, action });
        }
    })
}

/// 做一次“只改图层元数据”的编辑，并压入一步可撤销的命令。
fn meta_edit(state: &mut AppState, label: &'static str, mutate: impl FnOnce(&mut Document)) {
    let before = LayerMetaCommand::capture(&state.document);
    mutate(&mut state.document);
    let after = LayerMetaCommand::capture(&state.document);
    state.execute(Box::new(LayerMetaCommand::new(before, after, label)));
}

fn button(
    theme: &'static dyn Theme,
    label: &'static str,
    action: impl FnMut(&mut SceneTree, NodeId) + 'static,
) -> Button {
    Button::secondary(label, theme)
        .font_size(TextSize::Small.px())
        .on_click(action)
}
