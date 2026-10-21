//! 拖动 / 缩放：把指针落点翻译成手势，再按手势改窗口矩形。
//!
//! 命中顺序：控件（交给树）> 边框（缩放）> 标题栏（移动）> 窗口内空白（不动）。
//! 纯几何在 `geometry.rs`，这里只负责「哪只手势 + 用它改 `rect`」。

use super::geometry::{resize_edges, resize_rect, ResizeEdges};
use super::*;

/// 指针按下的窗口手势。
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub(super) enum Gesture {
    #[default]
    None,
    /// 标题栏拖动：记录按下时的窗口矩形与指针，移动时整体平移。
    Move {
        start_rect: Rect,
        start_pointer: Vec2,
    },
    /// 边框缩放：记录抓的是哪几条边。
    Resize {
        edges: ResizeEdges,
        start_rect: Rect,
        start_pointer: Vec2,
    },
}

impl Window {
    /// 指针在窗口里的落点：点交互控件返回 `false`（交给树），否则起手势。
    pub(super) fn begin_gesture(&mut self, position: Vec2) -> bool {
        let rect = self.rect.get();
        if !rect.contains(position) {
            self.close.set(true);
            return true;
        }
        if igui_ui::hit_test(&self.tree, position).is_some() {
            return false;
        }
        if let Some(edges) = resize_edges(rect, position) {
            self.gesture.set(Gesture::Resize {
                edges,
                start_rect: rect,
                start_pointer: position,
            });
            return true;
        }
        if position.y <= rect.top() + TITLE_BAR_HEIGHT {
            self.gesture.set(Gesture::Move {
                start_rect: rect,
                start_pointer: position,
            });
            return true;
        }
        false
    }

    /// 有手势时按指针更新矩形；返回是否消费了这次移动。
    pub(super) fn apply_gesture(&mut self, position: Vec2) -> bool {
        let viewport = self.viewport.get().logical_size();
        match self.gesture.get() {
            Gesture::Move {
                start_rect,
                start_pointer,
            } => {
                let moved = start_rect.translate(position - start_pointer);
                self.rect.set(clamp_to_viewport(moved, viewport));
                true
            }
            Gesture::Resize {
                edges,
                start_rect,
                start_pointer,
            } => {
                let resized = resize_rect(
                    start_rect,
                    edges,
                    position - start_pointer,
                    self.min_size,
                    viewport,
                );
                self.rect.set(resized);
                true
            }
            Gesture::None => false,
        }
    }
}
