//! 渐变工具：拖一条轴定义方向 / 长度，用多节点 ramp 填充选区。
//!
//! 拖动时直接改当前图层做实时预览（落笔前存快照，每次 move 从快照恢复再重画），
//! 松手用 [`PixelRegion::diff`] 抓差异提交**一条** undo。作用域 = 当前选区
//! （没有选区就是整层）；用 `blend_pixel`（source-over）合成，所以透明端会保留
//! 底下像素。

use std::cell::RefCell;
use std::f32::consts::PI;
use std::rc::Rc;

use igui_core::Vec2;

use super::tool::{PointerEvent, Tool, ToolContext};
use crate::canvas::Selection;
use crate::document::{Gradient, LayerId, PaintCommand, PixelBuffer, Point};

/// 渐变类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GradientKind {
    #[default]
    Linear,
    Reflected,
    Radial,
    Angle,
    Diamond,
}

impl GradientKind {
    pub const ALL: [GradientKind; 5] = [
        GradientKind::Linear,
        GradientKind::Reflected,
        GradientKind::Radial,
        GradientKind::Angle,
        GradientKind::Diamond,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Linear => "线性",
            Self::Reflected => "对称",
            Self::Radial => "径向",
            Self::Angle => "角度",
            Self::Diamond => "菱形",
        }
    }

    /// 选项栏 `+` 用的下一个类型。
    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|kind| *kind == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    /// 选项栏 `−` 用的上一个类型。
    pub fn prev(self) -> Self {
        let index = Self::ALL.iter().position(|kind| *kind == self).unwrap_or(0);
        Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// 像素中心 `p` 在渐变轴（`start` -> `end`）上的归一化位置 0..=1。
pub fn gradient_t(kind: GradientKind, start: Vec2, end: Vec2, p: Vec2) -> f32 {
    let v = end - start;
    let len2 = v.length_squared();
    if len2 <= f32::EPSILON {
        return 0.0;
    }
    let len = len2.sqrt();
    let direction = v / len;
    let rel = p - start;
    let t = match kind {
        GradientKind::Linear => (rel.x * v.x + rel.y * v.y) / len2,
        GradientKind::Reflected => ((rel.x * v.x + rel.y * v.y) / len2).abs(),
        GradientKind::Radial => rel.length() / len,
        GradientKind::Angle => {
            let angle = rel.y.atan2(rel.x) - v.y.atan2(v.x);
            let t = angle / (2.0 * PI);
            t - t.floor()
        }
        GradientKind::Diamond => {
            let along = rel.x * direction.x + rel.y * direction.y;
            let across = -rel.x * direction.y + rel.y * direction.x;
            (along.abs() + across.abs()) / len
        }
    };
    t.clamp(0.0, 1.0)
}

/// 把渐变写进 `buffer`（文档坐标 `0..document`，按 `offset` 映射到缓冲区，
/// 按 `clip` 裁剪）。返回是否碰到过像素。
#[allow(clippy::too_many_arguments)]
pub fn gradient_fill(
    buffer: &mut PixelBuffer,
    offset: Point,
    document: (u32, u32),
    start: Vec2,
    end: Vec2,
    kind: GradientKind,
    gradient: &Gradient,
    reverse: bool,
    opacity: f32,
    clip: Option<&Selection>,
) -> bool {
    if (end - start).length_squared() <= f32::EPSILON {
        return false;
    }
    let width = buffer.width;
    let height = buffer.height;
    let mut touched = false;
    for y in 0..document.1 {
        for x in 0..document.0 {
            if clip.is_some_and(|selection| !selection.contains(x, y)) {
                continue;
            }
            let bx = x as i64 - offset.x as i64;
            let by = y as i64 - offset.y as i64;
            if bx < 0 || by < 0 || bx >= width as i64 || by >= height as i64 {
                continue;
            }
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let mut t = gradient_t(kind, start, end, p);
            if reverse {
                t = 1.0 - t;
            }
            let color = gradient.evaluate(t);
            buffer.blend_pixel(bx as u32, by as u32, color, opacity);
            touched = true;
        }
    }
    touched
}

/// 落笔时的图层快照，拖动时用它恢复再重画。
#[derive(Debug, Clone, PartialEq)]
struct PendingGradient {
    layer: LayerId,
    before: PixelBuffer,
}

/// 渐变工具。颜色 ramp 是共享的（编辑器改、这里读）。
#[derive(Debug, Clone, PartialEq)]
pub struct GradientTool {
    pub kind: GradientKind,
    pub gradient: Rc<RefCell<Gradient>>,
    pub reverse: bool,
    pub opacity: f32,
    pub clip: Option<Rc<Selection>>,
    start: Option<Vec2>,
    end: Option<Vec2>,
    pending: Option<PendingGradient>,
}

impl GradientTool {
    pub fn new(gradient: Rc<RefCell<Gradient>>) -> Self {
        Self {
            kind: GradientKind::default(),
            gradient,
            reverse: false,
            opacity: 1.0,
            clip: None,
            start: None,
            end: None,
            pending: None,
        }
    }

    /// 是否正处在一次拖拽中。
    pub fn is_drawing(&self) -> bool {
        self.pending.is_some()
    }

    /// 拖动中的渐变轴（文档坐标），给视图画预览。
    pub fn preview(&self) -> Option<(Vec2, Vec2)> {
        Some((self.start?, self.end?))
    }

    /// 把当前轴重画到图层上（先从快照恢复）。
    fn repaint(&self, ctx: &mut ToolContext) {
        let (Some(start), Some(end), Some(pending)) = (self.start, self.end, self.pending.as_ref())
        else {
            return;
        };
        if let Some(layer) = ctx.document.layer_mut(pending.layer) {
            if layer.pixels.data.len() == pending.before.data.len() {
                layer.pixels.data.copy_from_slice(&pending.before.data);
            } else {
                layer.pixels = pending.before.clone();
            }
        }
        if (end - start).length() < 1.0 {
            return;
        }
        let document = (ctx.document.width, ctx.document.height);
        let offset = ctx
            .document
            .layer(pending.layer)
            .map(|layer| layer.position)
            .unwrap_or(Point::new(0, 0));
        let gradient = self.gradient.borrow();
        if let Some(layer) = ctx.document.layer_mut(pending.layer) {
            gradient_fill(
                &mut layer.pixels,
                offset,
                document,
                start,
                end,
                self.kind,
                &gradient,
                self.reverse,
                self.opacity,
                self.clip.as_deref(),
            );
        }
    }
}

impl Tool for GradientTool {
    fn name(&self) -> &'static str {
        "渐变"
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolContext, event: PointerEvent) {
        let Some(layer_id) = ctx.document.active_layer().map(|layer| layer.id) else {
            return;
        };
        let (dx, dy) = ctx.document.ensure_layer_covers_document(layer_id);
        ctx.history.translate_layer(layer_id, dx, dy);
        let Some(before) = ctx
            .document
            .layer(layer_id)
            .map(|layer| layer.pixels.clone())
        else {
            return;
        };
        self.start = Some(event.position);
        self.end = Some(event.position);
        self.pending = Some(PendingGradient {
            layer: layer_id,
            before,
        });
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolContext, event: PointerEvent) {
        if self.pending.is_none() {
            return;
        }
        self.end = Some(event.position);
        self.repaint(ctx);
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolContext, event: PointerEvent) {
        if self.pending.is_none() {
            return;
        }
        self.end = Some(event.position);
        self.repaint(ctx);
        let pending = self.pending.take().expect("pending");
        self.start = None;
        self.end = None;
        let command = ctx.document.layer(pending.layer).and_then(|layer| {
            PaintCommand::capture(pending.layer, &pending.before, &layer.pixels, self.name())
        });
        if let Some(command) = command {
            ctx.history.execute(Box::new(command), ctx.document);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Color, Document, History, PixelRegion};

    fn gradient() -> Rc<RefCell<Gradient>> {
        Rc::new(RefCell::new(Gradient::from_colors(
            Color::BLACK,
            Color::WHITE,
        )))
    }

    fn at(document: &Document, x: u32, y: u32) -> Color {
        document.active_layer().unwrap().pixels.get_pixel(x, y)
    }

    #[test]
    fn linear_t_is_the_projection_along_the_axis() {
        let start = Vec2::new(0.0, 0.0);
        let end = Vec2::new(10.0, 0.0);
        assert_eq!(gradient_t(GradientKind::Linear, start, end, start), 0.0);
        assert_eq!(gradient_t(GradientKind::Linear, start, end, end), 1.0);
        assert_eq!(
            gradient_t(GradientKind::Linear, start, end, Vec2::new(5.0, 3.0)),
            0.5
        );
        // 轴外 clamp。
        assert_eq!(
            gradient_t(GradientKind::Linear, start, end, Vec2::new(-5.0, 0.0)),
            0.0
        );
    }

    #[test]
    fn reflected_is_symmetric_about_the_start() {
        let start = Vec2::ZERO;
        let end = Vec2::new(10.0, 0.0);
        assert_eq!(
            gradient_t(GradientKind::Reflected, start, end, Vec2::new(-5.0, 0.0)),
            0.5
        );
        assert_eq!(
            gradient_t(GradientKind::Reflected, start, end, Vec2::new(5.0, 0.0)),
            0.5
        );
    }

    #[test]
    fn radial_t_is_distance_from_the_start_over_the_drag_length() {
        let start = Vec2::ZERO;
        let end = Vec2::new(10.0, 0.0);
        assert_eq!(gradient_t(GradientKind::Radial, start, end, start), 0.0);
        assert_eq!(
            gradient_t(GradientKind::Radial, start, end, Vec2::new(0.0, 10.0)),
            1.0
        );
        assert_eq!(
            gradient_t(GradientKind::Radial, start, end, Vec2::new(0.0, 5.0)),
            0.5
        );
    }

    #[test]
    fn diamond_is_one_at_both_axis_extremes() {
        let start = Vec2::ZERO;
        let end = Vec2::new(10.0, 0.0);
        assert_eq!(gradient_t(GradientKind::Diamond, start, end, start), 0.0);
        assert_eq!(
            gradient_t(GradientKind::Diamond, start, end, Vec2::new(10.0, 0.0)),
            1.0
        );
        assert_eq!(
            gradient_t(GradientKind::Diamond, start, end, Vec2::new(0.0, 10.0)),
            1.0
        );
    }

    #[test]
    fn angle_wraps_around_the_start() {
        let start = Vec2::ZERO;
        let end = Vec2::new(10.0, 0.0);
        // 轴方向上是 0；转半圈是 0.5；正上方是 0.75（逆时针）。
        let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
        assert!(close(
            gradient_t(GradientKind::Angle, start, end, Vec2::new(5.0, 0.0)),
            0.0
        ));
        assert!(close(
            gradient_t(GradientKind::Angle, start, end, Vec2::new(-5.0, 0.0)),
            0.5
        ));
        assert!(close(
            gradient_t(GradientKind::Angle, start, end, Vec2::new(0.0, -5.0)),
            0.75
        ));
    }

    #[test]
    fn a_linear_gradient_fills_from_start_to_end() {
        let mut document = Document::new("d", 16, 1);
        let gradient = Gradient::from_colors(Color::BLACK, Color::WHITE);
        let layer = document.active_layer_mut().unwrap();
        // 轴用首尾像素中心，端点正好是 0 / 1。
        gradient_fill(
            &mut layer.pixels,
            Point::new(0, 0),
            (16, 1),
            Vec2::new(0.5, 0.5),
            Vec2::new(15.5, 0.5),
            GradientKind::Linear,
            &gradient,
            false,
            1.0,
            None,
        );
        assert_eq!(at(&document, 0, 0), Color::BLACK, "起点是起始色");
        assert_eq!(at(&document, 15, 0), Color::WHITE, "终点是结束色");
        let mid = at(&document, 8, 0);
        assert!(mid.r > 100 && mid.r < 160, "中间应插值，实际 {mid:?}");
    }

    #[test]
    fn a_transparent_end_leaves_the_pixels_underneath() {
        let mut document = Document::new("d", 16, 1);
        let gradient = Gradient::from_colors(Color::RED, Color::RED.with_alpha(0));
        let layer = document.active_layer_mut().unwrap();
        gradient_fill(
            &mut layer.pixels,
            Point::new(0, 0),
            (16, 1),
            Vec2::new(0.5, 0.5),
            Vec2::new(15.5, 0.5),
            GradientKind::Linear,
            &gradient,
            false,
            1.0,
            None,
        );
        assert_eq!(at(&document, 0, 0), Color::RED, "起点不透明");
        assert_eq!(at(&document, 15, 0), Color::WHITE, "透明端保留底下的白");
    }

    #[test]
    fn the_selection_confines_the_gradient() {
        let mut document = Document::new("d", 16, 1);
        let gradient = Gradient::from_colors(Color::BLACK, Color::WHITE);
        let clip = Selection::rect(PixelRegion::new(0, 0, 8, 1), (16, 1));
        let layer = document.active_layer_mut().unwrap();
        gradient_fill(
            &mut layer.pixels,
            Point::new(0, 0),
            (16, 1),
            Vec2::new(0.5, 0.5),
            Vec2::new(15.5, 0.5),
            GradientKind::Linear,
            &gradient,
            false,
            1.0,
            Some(&clip),
        );
        assert_ne!(at(&document, 7, 0), Color::WHITE, "选区内被画");
        assert_eq!(at(&document, 8, 0), Color::WHITE, "选区外保持白");
    }

    #[test]
    fn a_zero_length_drag_is_a_no_op() {
        let mut document = Document::new("d", 8, 1);
        let gradient = Gradient::from_colors(Color::BLACK, Color::WHITE);
        let layer = document.active_layer_mut().unwrap();
        assert!(!gradient_fill(
            &mut layer.pixels,
            Point::new(0, 0),
            (8, 1),
            Vec2::new(4.0, 0.5),
            Vec2::new(4.0, 0.5),
            GradientKind::Linear,
            &gradient,
            false,
            1.0,
            None,
        ));
        assert_eq!(at(&document, 4, 0), Color::WHITE);
    }

    #[test]
    fn a_drag_commits_one_undo_step() {
        let mut document = Document::new("d", 16, 1);
        let mut history = History::new();
        let mut tool = GradientTool::new(gradient());
        let mut ctx = ToolContext {
            document: &mut document,
            history: &mut history,
        };
        let pointer = |x: f32| PointerEvent {
            position: Vec2::new(x, 0.5),
            button: igui_core::PointerButton::Left,
        };
        tool.on_pointer_down(&mut ctx, pointer(0.0));
        tool.on_pointer_move(&mut ctx, pointer(16.0));
        tool.on_pointer_up(&mut ctx, pointer(16.0));
        assert_eq!(history.undo_len(), 1, "一次拖拽 = 一步 undo");
        history.undo(&mut document);
        assert_eq!(at(&document, 0, 0), Color::WHITE, "撤销还原");
    }

    #[test]
    fn kind_cycles_forward_and_back() {
        assert_eq!(GradientKind::Linear.next(), GradientKind::Reflected);
        assert_eq!(GradientKind::Linear.prev(), GradientKind::Diamond);
        assert_eq!(GradientKind::Diamond.next(), GradientKind::Linear);
    }
}
