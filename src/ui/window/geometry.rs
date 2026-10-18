//! 窗口拖动缩放的纯几何：命中哪几条边、按边改矩形、把矩形钳进视口。
//!
//! 与树 / 绘制无关，边界情况可以直接单测。

use igui_core::{Rect, Size, Vec2};

/// 拖动缩放的命中带宽度：贴住窗口四条边，向内的这一圈算「边框」。
pub(super) const RESIZE_BORDER: f32 = 6.0;

/// 可组合的缩放边（含角，就是两条边同时为真）。
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(super) struct ResizeEdges {
    pub(super) left: bool,
    pub(super) top: bool,
    pub(super) right: bool,
    pub(super) bottom: bool,
}

impl ResizeEdges {
    pub(super) fn any(self) -> bool {
        self.left || self.top || self.right || self.bottom
    }
}

/// 指针落在窗口的哪几条边上（含角）。不在窗口里返回 `None`。
pub(super) fn resize_edges(rect: Rect, position: Vec2) -> Option<ResizeEdges> {
    if !rect.contains(position) {
        return None;
    }
    let mut edges = ResizeEdges::default();
    if position.x < rect.left() + RESIZE_BORDER {
        edges.left = true;
    }
    if position.x >= rect.right() - RESIZE_BORDER {
        edges.right = true;
    }
    if position.y < rect.top() + RESIZE_BORDER {
        edges.top = true;
    }
    if position.y >= rect.bottom() - RESIZE_BORDER {
        edges.bottom = true;
    }
    edges.any().then_some(edges)
}

/// 按抓边把 `start` 改到新尺寸，并钳到视口与最小尺寸里。
pub(super) fn resize_rect(
    start: Rect,
    edges: ResizeEdges,
    delta: Vec2,
    min: Vec2,
    viewport: Size,
) -> Rect {
    let mut left = start.left();
    let mut top = start.top();
    let mut right = start.right();
    let mut bottom = start.bottom();
    if edges.left {
        left = (left + delta.x).max(0.0).min(right - min.x);
    }
    if edges.right {
        right = (right + delta.x).max(left + min.x);
        right = right.min(viewport.width.max(left + min.x));
    }
    if edges.top {
        top = (top + delta.y).max(0.0).min(bottom - min.y);
    }
    if edges.bottom {
        bottom = (bottom + delta.y).max(top + min.y);
        bottom = bottom.min(viewport.height.max(top + min.y));
    }
    Rect::from_min_max(Vec2::new(left, top), Vec2::new(right, bottom))
}

/// 把窗口整个钳进视口（视口更小时只缩到视口大小，不越界）。
pub(super) fn clamp_to_viewport(rect: Rect, viewport: Size) -> Rect {
    let width = rect.size.width.min(viewport.width.max(0.0));
    let height = rect.size.height.min(viewport.height.max(0.0));
    let x = rect.origin.x.clamp(0.0, (viewport.width - width).max(0.0));
    let y = rect
        .origin
        .y
        .clamp(0.0, (viewport.height - height).max(0.0));
    Rect::from_min_size(Vec2::new(x, y), Size::new(width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(Vec2::new(x, y), Size::new(w, h))
    }

    #[test]
    fn a_corner_hit_selects_both_edges() {
        let window = rect(100.0, 100.0, 300.0, 200.0);
        let edges = resize_edges(window, Vec2::new(101.0, 101.0)).expect("左上角");
        assert!(edges.left && edges.top && !edges.right && !edges.bottom);
    }

    #[test]
    fn an_interior_point_is_not_a_resize_hit() {
        let window = rect(100.0, 100.0, 300.0, 200.0);
        assert!(resize_edges(window, window.center()).is_none());
        assert!(
            resize_edges(window, Vec2::new(10.0, 10.0)).is_none(),
            "窗口外"
        );
    }

    #[test]
    fn resizing_the_bottom_right_cannot_go_below_the_minimum() {
        let window = rect(0.0, 0.0, 300.0, 200.0);
        let edges = ResizeEdges {
            right: true,
            bottom: true,
            ..ResizeEdges::default()
        };
        let resized = resize_rect(
            window,
            edges,
            Vec2::new(-1000.0, -1000.0),
            Vec2::new(200.0, 120.0),
            Size::new(800.0, 600.0),
        );
        assert_eq!(resized.size, Size::new(200.0, 120.0));
    }

    #[test]
    fn resizing_stops_at_the_viewport_edge() {
        let window = rect(0.0, 0.0, 300.0, 200.0);
        let edges = ResizeEdges {
            right: true,
            bottom: true,
            ..ResizeEdges::default()
        };
        let resized = resize_rect(
            window,
            edges,
            Vec2::new(10_000.0, 10_000.0),
            Vec2::new(100.0, 100.0),
            Size::new(500.0, 400.0),
        );
        assert_eq!(resized.size, Size::new(500.0, 400.0));
    }

    #[test]
    fn a_window_smaller_than_the_viewport_is_kept_inside() {
        let window = rect(700.0, 500.0, 200.0, 150.0);
        let clamped = clamp_to_viewport(window, Size::new(800.0, 600.0));
        assert_eq!(clamped, rect(600.0, 450.0, 200.0, 150.0));
    }

    #[test]
    fn a_window_larger_than_the_viewport_shrinks_to_it() {
        let window = rect(-50.0, -50.0, 1000.0, 800.0);
        let clamped = clamp_to_viewport(window, Size::new(800.0, 600.0));
        assert_eq!(clamped, rect(0.0, 0.0, 800.0, 600.0));
    }
}
