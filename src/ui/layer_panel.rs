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
use igui_core::{Edges, NodeId, Rect, Size, Vec2};
use igui_render::{Paint, PaintContext};
use igui_scene::SceneTree;
use igui_theme::{space, TextSize, Theme, Tone};
use igui_ui::MouseFilter;

use crate::app::state::AppState;
use crate::document::{AddLayerCommand, Document, Layer, LayerMetaCommand, LayerRow, PixelBuffer};
use crate::icons::IconSet;

use crate::ui::card::Card;

use super::{Intent, Intents, LayerAction};

/// 图层行高（逻辑像素）。列表的池大小按它算。
pub const LAYER_ROW_HEIGHT: f32 = 28.0;
/// 每级缩进的逻辑像素。
const INDENT: f32 = 10.0;
/// 行内图标尺寸（眼睛 / 文件夹 / 蒙版）。
const ICON: f32 = 14.0;

/// 图层列表：第 0 行是**最上面的节点**（跟 Photoshop 一致），数据来自行缓存。
#[allow(clippy::too_many_arguments)]
pub fn layer_list(
    theme: &'static dyn Theme,
    state: Rc<RefCell<AppState>>,
    rows: Rc<RefCell<Vec<LayerRow>>>,
    textures: Rc<RefCell<crate::ui::layer_thumbs::LayerTextures>>,
    icons: Rc<IconSet>,
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
            vec![row.name.clone(), format!("{:.0}%", row.opacity * 100.0)]
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

    // 眼睛：可见 `eye`、隐藏 `eye-off`（Lucide，矢量描边，不用纹理）。
    let eye = {
        let rows = rows.clone();
        let icons = icons.clone();
        move |ctx: &mut PaintContext, rect: Rect, index: usize| {
            let rows = rows.borrow();
            let Some(row) = rows.get(index) else {
                return;
            };
            let name = if row.visible { "eye" } else { "eye-off" };
            let color = if row.visible {
                theme.palette().foreground
            } else {
                theme.palette().muted
            };
            if let Some(document) = icons.document(name) {
                document.draw(
                    ctx,
                    Rect::from_center_size(rect.center(), Size::splat(ICON)),
                    color,
                );
            }
        }
    };

    // 图层缩略图。组永远没有像素预览，这一格改画文件夹图标
    // （`folder-open` 展开 / `folder` 折叠）—— 展开箭头仍然占它原来的位置。
    let thumbnail = {
        let rows = rows.clone();
        let textures = textures.clone();
        let icons = icons.clone();
        move |ctx: &mut PaintContext, rect: Rect, index: usize| {
            let rows = rows.borrow();
            let Some(row) = rows.get(index) else {
                return;
            };
            let color = theme.palette().muted;
            if row.is_group {
                let name = if row.expanded {
                    "folder-open"
                } else {
                    "folder"
                };
                if let Some(document) = icons.document(name) {
                    document.draw(
                        ctx,
                        Rect::from_center_size(rect.center(), Size::splat(ICON)),
                        color,
                    );
                }
                return;
            }
            let square = Rect::from_center_size(rect.center(), Size::splat(20.0));
            match textures.borrow().get(&row.id).copied() {
                Some(texture) => ctx.draw_image(texture, square, None, Paint::default()),
                None => ctx.stroke_rect(square, 1.0, Paint::new(color)),
            }
        }
    };

    // 展开箭头（原来小箭头的位置不变）：组才画，展开向下、折叠向右。
    let triangle = {
        let rows = rows.clone();
        move |ctx: &mut PaintContext, rect: Rect, index: usize| {
            let rows = rows.borrow();
            let Some(row) = rows.get(index) else {
                return;
            };
            if !row.is_group {
                return;
            }
            let center = rect.center();
            let paint = Paint::new(theme.palette().muted);
            let (a, b, c) = if row.expanded {
                (
                    center + Vec2::new(-3.5, -1.5),
                    center + Vec2::new(0.0, 2.0),
                    center + Vec2::new(3.5, -1.5),
                )
            } else {
                (
                    center + Vec2::new(-1.5, -3.5),
                    center + Vec2::new(2.0, 0.0),
                    center + Vec2::new(-1.5, 3.5),
                )
            };
            ctx.draw_line(a, b, 1.0, paint);
            ctx.draw_line(b, c, 1.0, paint);
        }
    };

    // 蒙版缩略图：小方块，启用时左半填色（黑/白覆盖率的样子）。
    let mask_icon = {
        let rows = rows.clone();
        move |ctx: &mut PaintContext, rect: Rect, index: usize| {
            let rows = rows.borrow();
            let Some(row) = rows.get(index) else {
                return;
            };
            if !row.has_mask {
                return;
            }
            let square = Rect::from_center_size(rect.center(), Size::splat(10.0));
            let paint = Paint::new(theme.palette().muted);
            ctx.stroke_rect(square, 1.0, paint);
            if row.mask_enabled {
                ctx.fill_rect(
                    Rect::from_min_max(square.min(), Vec2::new(square.center().x, square.max().y)),
                    paint,
                );
            }
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

    let cell = {
        let rows = rows.clone();
        let state = state.clone();
        let selected = selected.clone();
        let intents = intents.clone();
        move |index: usize, cell: usize| {
            let Some(row) = rows.borrow().get(index).cloned() else {
                return;
            };
            match cell {
                // 0 缩进 / 1 眼睛 / 2 展开箭头 / 3 缩略图（组是文件夹）/ 4 蒙版；
                // 其余列（名字 / 不透明度）当选中。
                1 => intents.push(Intent::LayerAction {
                    target: row.id,
                    action: LayerAction::ToggleVisible,
                }),
                2 if row.is_group => intents.push(Intent::LayerAction {
                    target: row.id,
                    action: LayerAction::ToggleExpand,
                }),
                4 if row.has_mask => intents.push(Intent::LayerAction {
                    target: row.id,
                    action: LayerAction::ToggleEditTarget,
                }),
                _ => {
                    state.borrow_mut().document.select_layer(row.id);
                    selected.set(Some(index));
                }
            }
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
        .icon(18.0, eye)
        .icon(14.0, triangle)
        .icon(24.0, thumbnail)
        .icon(14.0, mask_icon)
        .padding(space::XS)
        .gap(space::XXS)
        .columns(vec![
            ListColumn::flexible(),
            ListColumn::fixed(42.0).tone(Tone::Muted),
        ])
        .count(count)
        .selected(selected)
        .on_activate(activate)
        .on_context(context)
        .on_cell(cell)
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
