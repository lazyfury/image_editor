//! 图层拖拽排序 / 换父：应用层实现（不改 igui 的 `List`）。
//!
//! 行几何靠 `ListState::rows()` + `igui_ui::control(...).rect`；`List` 的行口气
//! 已经按视口顺序给出，数据下标 = `visible_range().start + slot`。拖动时：
//!
//! - 超过 [`DRAG_SLOP`] 才算拖拽，否则就是一次普通点击（选中由 `List` 处理）；
//! - 行内上 25% = 放到这行下面、下 25% = 放到上面、中间 50% = 放进组（非组取近侧）；
//! - 靠近列表上下边缘自动滚动；悬停组中段 [`HOVER_EXPAND_MS`] 后自动展开；
//! - 非法目标（拖进自己的子树 / 拖回原位）不产生命令，只提示。
//!
//! 投放指示线在 [`EditorView::paint`] 里、UI 之后画（2px 高亮线 / 整行描边）。

use std::time::Instant;

use igui_core::{Rect, Vec2};
use igui_render::{Paint, PaintContext};

use crate::document::{LayerId, MoveLayerCommand, MoveTarget};

use super::*;

/// 起拖阈值（逻辑像素）。
pub const DRAG_SLOP: f32 = 4.0;
/// 拖到列表边缘多近开始自动滚动（逻辑像素）。
const EDGE_SCROLL: f32 = 16.0;
/// 自动滚动速度（逻辑像素 / 帧）。
const EDGE_SCROLL_STEP: f32 = 10.0;
/// 悬停组中段多久自动展开（毫秒）。
pub const HOVER_EXPAND_MS: u128 = 400;

/// 一次拖拽的状态。
#[derive(Debug, Clone, Copy)]
pub(super) struct LayerDrag {
    pub from: LayerId,
    pub start: Vec2,
    pub started: bool,
    pub target: Option<DropTarget>,
    /// 悬停展开用：上一个「组中段」目标与进入时间。
    pub hover_group: Option<(LayerId, Instant)>,
}

/// 投放位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTarget {
    /// 放到这一行的下面（同一父级）。
    Before(LayerId),
    /// 放到这一行的上面（同一父级）。
    After(LayerId),
    /// 放进组（组的顶部）。
    Into(LayerId),
    /// 放到根层最上面（列表下方空白）。
    Root,
}

impl DropTarget {
    fn to_move(self) -> MoveTarget {
        match self {
            DropTarget::Before(id) => MoveTarget::Before(id),
            DropTarget::After(id) => MoveTarget::After(id),
            DropTarget::Into(id) => MoveTarget::Into(id),
            DropTarget::Root => MoveTarget::Position(None, usize::MAX),
        }
    }
}

impl EditorView {
    /// 列表容器矩形（拖拽命中用）。
    fn layer_list_rect(&self) -> Option<Rect> {
        let container = self.layer_state.container()?;
        igui_ui::control(&self.tree, container).map(|control| control.rect)
    }

    /// 指针下的行：返回（数据下标、行号、行矩形）。
    fn layer_row_at(&self, position: Vec2) -> Option<(usize, LayerId, Rect)> {
        let range = self.layer_state.visible_range();
        for (slot, node) in self.layer_state.rows().into_iter().enumerate() {
            if slot >= range.len() {
                break;
            }
            let Some(control) = igui_ui::control(&self.tree, node) else {
                continue;
            };
            let rect = control.rect;
            if position.y >= rect.top() && position.y <= rect.bottom() {
                let index = range.start + slot;
                let id = self.layer_rows.borrow().get(index).map(|row| row.id)?;
                return Some((index, id, rect));
            }
        }
        None
    }

    /// 指针位置换算成投放目标。
    fn layer_drop_target(&self, position: Vec2) -> Option<DropTarget> {
        let from = self.layer_drag.as_ref()?.from;
        let list = self.layer_list_rect()?;
        let Some((_, row_id, rect)) = self.layer_row_at(position) else {
            // 列表下方空白 = 根层最上面；列表上方空白同样处理（放到最上面）。
            return (position.y >= list.top()).then_some(DropTarget::Root);
        };
        if row_id == from {
            return None;
        }
        let is_group = self
            .state
            .borrow()
            .document
            .layer(row_id)
            .is_some_and(|layer| layer.is_group());
        let height = rect.size.height.max(1.0);
        let fraction = ((position.y - rect.top()) / height).clamp(0.0, 1.0);
        let target = if is_group && (0.25..0.75).contains(&fraction) {
            DropTarget::Into(row_id)
        } else if fraction < 0.5 {
            DropTarget::Before(row_id)
        } else {
            DropTarget::After(row_id)
        };
        self.is_legal_drop(from, target).then_some(target)
    }

    /// 拖进自己的子树 / 放回原位都不算合法目标。
    fn is_legal_drop(&self, from: LayerId, target: DropTarget) -> bool {
        let state = self.state.borrow();
        let document = &state.document;
        let Some(layer) = document.layer(from) else {
            return false;
        };
        let from_parent = layer.parent;
        let from_siblings = document.children(from_parent);
        let from_index = from_siblings.iter().position(|id| *id == from);
        match target {
            DropTarget::Into(group) => {
                document.can_move(from, Some(group), 0)
                    && !(from_parent == Some(group)
                        && from_index
                            == Some(document.children(Some(group)).len().saturating_sub(1)))
            }
            DropTarget::Before(sibling) | DropTarget::After(sibling) => {
                if document.is_in_subtree(sibling, from) {
                    return false;
                }
                let Some(sibling_layer) = document.layer(sibling) else {
                    return false;
                };
                let target_parent = sibling_layer.parent;
                if !document.can_move(from, target_parent, 0) {
                    return false;
                }
                let siblings = document.children(target_parent);
                let Some(sibling_index) = siblings.iter().position(|id| *id == sibling) else {
                    return false;
                };
                let desired = if matches!(target, DropTarget::Before(_)) {
                    sibling_index
                } else {
                    sibling_index + 1
                };
                // 同一父级里，搬走自己会让后面的兄弟前移一位。
                let desired = if from_parent == target_parent
                    && from_index.is_some_and(|index| index < desired)
                {
                    desired - 1
                } else {
                    desired
                };
                !(from_parent == target_parent && from_index == Some(desired))
            }
            DropTarget::Root => {
                from_parent.is_some() || from_index != Some(from_siblings.len().saturating_sub(1))
            }
        }
    }

    /// `PointerDown`：落在图层列表上就记下起点（不消费，`List` 照常选中）。
    pub(super) fn layer_drag_press(&mut self, position: Vec2) {
        let Some(list) = self.layer_list_rect() else {
            return;
        };
        if !list.contains(position) {
            return;
        }
        let Some((_, id, _)) = self.layer_row_at(position) else {
            return;
        };
        self.layer_drag = Some(LayerDrag {
            from: id,
            start: position,
            started: false,
            target: None,
            hover_group: None,
        });
    }

    /// `PointerMove`：返回是否消费了事件（真拖动时是）。
    pub(super) fn layer_drag_move(&mut self, position: Vec2) -> bool {
        let Some(mut drag) = self.layer_drag else {
            return false;
        };
        if !drag.started {
            if (position - drag.start).length() < DRAG_SLOP {
                return false;
            }
            drag.started = true;
        }
        // 靠近列表上下边缘自动滚动。
        if let Some(list) = self.layer_list_rect() {
            if position.y < list.top() + EDGE_SCROLL {
                self.layer_state.scroll_by(-EDGE_SCROLL_STEP);
            } else if position.y > list.bottom() - EDGE_SCROLL {
                self.layer_state.scroll_by(EDGE_SCROLL_STEP);
            }
        }
        let target = self.layer_drop_target(position);
        // 悬停组中段 400ms 自动展开。
        if let Some(DropTarget::Into(group)) = target {
            match drag.hover_group {
                Some((hovered, since)) if hovered == group => {
                    if since.elapsed().as_millis() >= HOVER_EXPAND_MS {
                        self.state.borrow_mut().document.set_expanded(group, true);
                        self.refresh_layers();
                        drag.hover_group = None;
                    }
                }
                _ => drag.hover_group = Some((group, Instant::now())),
            }
        } else {
            drag.hover_group = None;
        }
        drag.target = target;
        self.layer_drag = Some(drag);
        drag.started
    }

    /// `PointerUp`：真拖动时提交一条命令（一步 undo），返回是否消费了事件。
    pub(super) fn layer_drag_release(&mut self, position: Vec2) -> bool {
        let Some(mut drag) = self.layer_drag.take() else {
            return false;
        };
        if !drag.started {
            return false;
        }
        drag.target = drag.target.or_else(|| self.layer_drop_target(position));
        let Some(target) = drag.target else {
            *self.message.borrow_mut() = Some("这个位置不能放".to_string());
            return true;
        };
        let mut state = self.state.borrow_mut();
        let command = MoveLayerCommand::new(&state.document, drag.from, target.to_move());
        state.execute(Box::new(command));
        drop(state);
        self.texture_dirty = true;
        true
    }

    /// 拖拽中丢掉状态（Esc / 窗口失焦）。
    pub(super) fn layer_drag_cancel(&mut self) {
        self.layer_drag = None;
    }

    /// 双击图层行：组切换展开 / 折叠，栅格层打开重命名。
    pub(super) fn toggle_layer_group_at(&mut self, position: Vec2) -> bool {
        let Some((_, id, _)) = self.layer_row_at(position) else {
            return false;
        };
        let (is_group, expanded) = {
            let state = self.state.borrow();
            let Some(layer) = state.document.layer(id) else {
                return false;
            };
            (layer.is_group(), layer.expanded)
        };
        if !is_group {
            if let Some(anchor) = self.rename_anchor() {
                self.open_rename(anchor);
            }
            return true;
        }
        self.state.borrow_mut().document.set_expanded(id, !expanded);
        self.refresh_layers();
        true
    }

    /// 在 UI 之后画投放指示线。
    pub(super) fn paint_layer_drag(&self, ctx: &mut PaintContext) {
        let Some(drag) = self.layer_drag else {
            return;
        };
        if !drag.started {
            return;
        }
        let Some(target) = drag.target else {
            return;
        };
        let color = self.theme.palette().selection;
        let paint = Paint::new(color);
        match target {
            DropTarget::Before(id) | DropTarget::After(id) => {
                let Some((_, _, rect)) = self.layer_row_at_target(id) else {
                    return;
                };
                let y = if matches!(target, DropTarget::Before(_)) {
                    rect.top()
                } else {
                    rect.bottom()
                };
                ctx.fill_rect(
                    Rect::from_min_max(
                        Vec2::new(rect.left() + 4.0, y - 1.0),
                        Vec2::new(rect.right() - 4.0, y + 1.0),
                    ),
                    paint,
                );
            }
            DropTarget::Into(id) => {
                if let Some((_, _, rect)) = self.layer_row_at_target(id) {
                    ctx.stroke_rect(rect, 2.0, paint);
                }
            }
            DropTarget::Root => {
                if let Some(list) = self.layer_list_rect() {
                    let y = list.bottom() - 1.0;
                    ctx.fill_rect(
                        Rect::from_min_max(
                            Vec2::new(list.left() + 4.0, y - 1.0),
                            Vec2::new(list.right() - 4.0, y + 1.0),
                        ),
                        paint,
                    );
                }
            }
        }
    }

    /// 某个图层 id 的行矩形（指示线用）。
    fn layer_row_at_target(&self, id: LayerId) -> Option<(usize, LayerId, Rect)> {
        let range = self.layer_state.visible_range();
        for (slot, node) in self.layer_state.rows().into_iter().enumerate() {
            let index = range.start + slot;
            let row_id = self.layer_rows.borrow().get(index).map(|row| row.id)?;
            if row_id == id {
                let rect = igui_ui::control(&self.tree, node)?.rect;
                return Some((index, row_id, rect));
            }
        }
        None
    }
}
