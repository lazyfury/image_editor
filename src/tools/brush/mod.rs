//! 画笔 / 橡皮：同一个引擎，只有 [`BrushMode`] 不同。
//!
//! 一次 stroke 的入口是 `on_pointer_down`，持续在 `on_pointer_move`，在
//! `on_pointer_up` 结束。每次 move 只往当前图层的 `PixelBuffer` 写像素；
//! 落笔时先快照整层，抬笔时用这一笔的脏矩形抓出差异，提交成**一条**
//! [`PaintCommand`] —— 一笔 = 一步 undo。
//!
//! 责任划分：
//! - [`tip`]：笔尖几何（形状 / 直径 / 硬度 -> 一个 stamp 的覆盖率）。
//! - [`mask`]：一笔的覆盖率蒙版 + 合成 —— `opacity` 是**每一笔**的不透明度，
//!   密集重叠的 stamp 不会把颜色叠深。
//! - [`stroke`]：光标事件 -> 平滑、等距的落笔点（快速拖动也不会变成折线）。
//! - [`preset`]：选项栏「预设」菜单用的命名参数组合。
//!
//! [`PixelRegion::diff`]: crate::document::PixelRegion::diff

pub mod mask;
pub mod preset;
pub mod tip;

use std::rc::Rc;

use igui_core::Vec2;

use super::stroke;
use super::tool::{PointerEvent, Tool, ToolContext};
use crate::canvas::Selection;
use crate::document::{
    Color, Document, LayerId, PaintCommand, PixelBuffer, PixelRegion, PixelTarget,
};

pub use tip::{BrushShape, Symmetry};

use mask::{StrokeMask, Writer};
use tip::Tip;

/// 画笔模式。擦除复用同一套 stamp / 插值逻辑。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrushMode {
    Paint,
    Erase,
}

/// 落笔时记下的快照，抬笔时用它算出差异区域。
#[derive(Debug, Clone, PartialEq)]
struct PendingStroke {
    layer: LayerId,
    /// 写图层缓冲还是蒙版。
    target: PixelTarget,
    before: PixelBuffer,
    /// 本笔的覆盖率蒙版（与图层缓冲区同尺寸），让 `opacity` 按「一笔」算。
    mask: StrokeMask,
}

/// 圆形画笔。尺寸是直径（文档像素），`opacity` 0..=1。
#[derive(Debug, Clone, PartialEq)]
pub struct BrushTool {
    /// 直径，文档像素。
    pub size: f32,
    /// 每一笔的不透明度（蒙版合成，不是每个 stamp 的）。
    pub opacity: f32,
    /// 笔尖硬度：1 = 只有 1px 抗锯齿边，0 = 从中心渐变到边缘。
    pub hardness: f32,
    /// 平滑强度：1 = 二次中点平滑（默认，快笔画不出折角），0 = 纯折线（像素画笔到手到）。
    pub smooth: f32,
    pub color: Color,
    pub mode: BrushMode,
    /// 可选选区：只有落在里面的像素会被写（选区的裁剪 / 羽化）。
    /// 视图在落笔前从编辑器状态同步过来；`Rc` 共享，克隆便宜。
    pub clip: Option<Rc<Selection>>,
    /// 像素模式：边缘不做抗锯齿（`coverage` 只有 0 / 1）。
    pub hard: bool,
    /// 笔刷形状。
    pub shape: BrushShape,
    /// 对称绘画（镜像轴 = 文档中线）。
    pub symmetry: Symmetry,
    /// 落笔间距相对直径的比例（默认 0.25）。
    pub spacing_ratio: f32,
    drawing: bool,
    /// 采样器状态：上一个 / 上上个光标点（见 [`stroke`]）。
    last: Option<Vec2>,
    previous: Option<Vec2>,
    /// 像素笔已经落笔到的位置（Bresenham 连线的起点）。
    painted: Option<Vec2>,
    /// 复用的等距采样缓冲，避免每个光标事件都分配。
    dabs: Vec<Vec2>,
    /// **整笔**实际写过的文档像素包围盒（抬笔时做撤销区域，`take_dirty` 不动它）。
    stroke_dirty: Option<PixelRegion>,
    /// 上次 [`Self::take_dirty`] 之后的脏区（视图拿它做局部合成）。
    dirty_since_take: Option<PixelRegion>,
    /// 当前笔触落笔时的图层快照；不在笔画中时为 `None`。
    pending: Option<PendingStroke>,
}

impl BrushTool {
    pub const MIN_SIZE: f32 = Tip::MIN_SIZE;
    pub const MAX_SIZE: f32 = Tip::MAX_SIZE;

    /// 默认的绘画画笔（黑、1px、不透明、硬边方形）。
    ///
    /// 1px + 硬边 + 最近邻过滤是像素图默认值：放大后画出的就是硬边单像素。
    pub fn paint() -> Self {
        Self {
            size: 1.0,
            opacity: 1.0,
            color: Color::BLACK,
            mode: BrushMode::Paint,
            clip: None,
            hardness: 1.0,
            smooth: 1.0,
            hard: true,
            shape: BrushShape::Square,
            symmetry: Symmetry::Off,
            spacing_ratio: 0.25,
            drawing: false,
            last: None,
            previous: None,
            painted: None,
            dabs: Vec::new(),
            stroke_dirty: None,
            dirty_since_take: None,
            pending: None,
        }
    }

    /// 橡皮构造器。运行时靠 `mode` 切换，这个构造器给测试 / 以后的
    /// “新建橡皮”用。
    #[allow(dead_code)]
    pub fn eraser() -> Self {
        Self {
            mode: BrushMode::Erase,
            ..Self::paint()
        }
    }

    /// 是否正处在一笔的中间。
    pub fn is_drawing(&self) -> bool {
        self.drawing
    }

    /// 取走「上次报告以来」写过的像素包围盒（文档坐标），视图拿它做局部合成。
    ///
    /// 整笔的包围盒另外记着（抬笔时做撤销区域），所以反复调用不会丢撤销范围。
    pub fn take_dirty(&mut self) -> Option<PixelRegion> {
        self.dirty_since_take.take()
    }

    /// 把 `color` 也带上。运行时直接改 `self.brush.color`，这里给测试构造画笔用。
    #[allow(dead_code)]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// 当前的笔尖几何（几何只取决于外观参数）。
    pub fn tip(&self) -> Tip {
        Tip {
            size: self.size,
            hardness: self.hardness,
            shape: self.shape,
            hard: self.hard,
            spacing_ratio: self.spacing_ratio,
        }
    }

    /// 落笔点之间的固定间距（文档像素），见 [`Tip::spacing`]。
    fn spacing(&self) -> f32 {
        self.tip().spacing()
    }

    /// 从上一个光标点画到 `to`。第一个点只压一次。
    ///
    /// `to` 是**文档坐标**的光标位置；真正的落笔点由 [`stroke`] 采样器给出
    /// （平滑 + 等距），写入时按当前图层的 `position`（缓冲区原点）换算到
    /// 缓冲区索引，所以笔迹始终对着光标。
    pub fn stroke_to(&mut self, document: &mut Document, to: Vec2) {
        let mut dabs = std::mem::take(&mut self.dabs);
        dabs.clear();
        self.extend_path(to, &mut dabs);
        self.paint_dabs(document, &mut dabs);
        self.dabs = dabs;
    }

    /// 抬笔：把平滑滞后留下的最后半段补完，笔迹终点正好落在 `to`。
    fn finish_stroke(&mut self, document: &mut Document, to: Vec2) {
        let mut dabs = std::mem::take(&mut self.dabs);
        dabs.clear();
        if let Some(last) = self.last {
            let ease = 1.0 - self.smooth.clamp(0.0, 1.0);
            let from = self.previous.map_or(last, |previous| {
                stroke::midpoint(previous, last).lerp(last, ease)
            });
            if self.previous.is_none() {
                // 只点了一下（没有第二个采样点）：起笔那个 dab 在这里补上。
                dabs.push(from);
            }
            stroke::curve_dabs(from, last, to, self.spacing(), &mut dabs);
            self.paint_dabs(document, &mut dabs);
        }
        self.dabs = dabs;
        self.reset_sampling();
    }

    /// 采样器：推进光标状态，把这批等距落笔点推进 `out`。
    ///
    /// 三点一组 `previous / last / to` 画一段二次贝塞尔（见 [`stroke`]）：起点是
    /// `(previous + last) / 2`（第一段从 `last` 本身出发），控制点是 `last`，
    /// 终点是 `(last + to) / 2`。终点需要「下一个点」，所以画的过程中笔迹末端
    /// 最多落后光标半个事件段，抬笔时由 [`Self::finish_stroke`] 补完。
    fn extend_path(&mut self, to: Vec2, out: &mut Vec<Vec2>) {
        let Some(last) = self.last else {
            self.last = Some(to);
            return;
        };
        if to == last {
            // 原地不动的 move 事件（有些系统会连发）不该再压一个点。
            return;
        }
        // 平滑强度把「锚点 / 光标」和「中点」混起来：1 = 平滑（终点是两段的中点，
        // 笔迹滞后半个事件段），0 = 折线（终点就是光标，笔到手到）。
        let ease = 1.0 - self.smooth.clamp(0.0, 1.0);
        let from = self.previous.map_or(last, |previous| {
            stroke::midpoint(previous, last).lerp(last, ease)
        });
        if self.previous.is_none() {
            // 起笔那个点现在才压，这样第一个 dab 正好落在按下的位置。
            out.push(from);
        }
        let end = stroke::midpoint(last, to).lerp(to, ease);
        stroke::curve_dabs(from, last, end, self.spacing(), out);
        self.previous = Some(last);
        self.last = Some(to);
    }

    /// 把一批等距采样点压到当前图层上。
    ///
    /// 对称绘画由 [`Writer`] 在像素级镜像（见 [`Symmetry::mirror_pixel`]），
    /// 所以这里只走一遍采样点。
    fn paint_dabs(&mut self, document: &mut Document, dabs: &mut Vec<Vec2>) {
        let tip = self.tip();
        // 1px 像素笔走 Bresenham：采样点之间可能斜向隔开，连线才不会断。
        let pixel_line = self.hard && self.size <= 1.0;
        let mut painted = self.painted;
        let Some(mut writer) = self.writer(document) else {
            return;
        };
        for point in dabs.iter().copied() {
            if pixel_line {
                painted = stroke_pixels(&mut writer, painted, point);
            } else {
                tip.cover(point, |x, y, coverage| writer.put(x, y, coverage));
            }
        }
        let dirty = writer.dirty();
        drop(writer);
        self.painted = painted;
        if let Some(region) = dirty {
            self.stroke_dirty = Some(self.stroke_dirty.map_or(region, |old| old.union(region)));
            self.dirty_since_take = Some(
                self.dirty_since_take
                    .map_or(region, |old| old.union(region)),
            );
        }
        dabs.clear();
    }

    /// 当前这一笔的写入上下文（目标图层 + 落笔前快照 + 本笔蒙版）。
    fn writer<'a>(&'a mut self, document: &'a mut Document) -> Option<Writer<'a>> {
        let document_size = (document.width, document.height);
        let clip = self.clip.clone();
        let (mode, color, opacity) = (self.mode, self.color, self.opacity);
        let pending = self.pending.as_mut()?;
        let PendingStroke {
            target: pending_target,
            before,
            mask,
            ..
        } = pending;
        let surface = document.active_surface_mut()?;
        // 笔画中途切了「编辑蒙版」：这一笔就作废，别把颜色写进错误的缓冲。
        if surface.target != *pending_target {
            return None;
        }
        let mask_mode = surface.target == PixelTarget::Mask;
        Some(Writer {
            mode,
            color,
            opacity,
            clip,
            offset: surface.position,
            symmetry: self.symmetry,
            document: document_size,
            before,
            mask,
            target: surface.pixels,
            mask_mode,
            dirty: None,
        })
    }

    /// 开一笔：把图层缓冲扩到覆盖整张文档、快照整层、开本笔的覆盖率蒙版。
    ///
    /// 落笔前扩大缓冲是为了「笔迹落在光标下」（选区外的文档区域也能画），
    /// 并且移出画布的内容保留在缓冲区里、不裁掉；缓冲整体平移了，旧命令记录
    /// 的区域要跟着平移。快照 + 蒙版让抬笔时能只提交差异、并且 `opacity` 按
    /// 「一笔」生效。
    fn begin_stroke(&mut self, ctx: &mut ToolContext) {
        self.reset_sampling();
        self.stroke_dirty = None;
        self.dirty_since_take = None;
        let Some((id, target)) = ctx
            .document
            .active_surface_mut()
            .map(|surface| (surface.id, surface.target))
        else {
            self.pending = None;
            return;
        };
        let (dx, dy) = ctx.document.ensure_layer_covers_document(id);
        ctx.history.translate_layer(id, dx, dy);
        let Some(surface) = ctx.document.active_surface_mut() else {
            self.pending = None;
            return;
        };
        if surface.target != target {
            self.pending = None;
            return;
        }
        let (width, height) = (surface.pixels.width, surface.pixels.height);
        self.pending = Some(PendingStroke {
            layer: id,
            target,
            before: surface.pixels.clone(),
            mask: StrokeMask::new(width, height),
        });
    }

    /// 收一笔：整笔的像素差异提交成**一条**命令；没有变化就不入栈。
    ///
    /// 区域有这一笔的脏矩形记录（[`Writer::dirty`]），所以不用扫整层做 diff：
    /// 4K 文档抬笔从「扫 830 万像素」变成「只比包围盒」。
    fn commit_stroke(&mut self, ctx: &mut ToolContext) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let document_region = self.stroke_dirty.take();
        let label = if pending.target == PixelTarget::Mask {
            "蒙版画笔"
        } else {
            self.name()
        };
        let after = ctx.document.surface_pixels(pending.layer, pending.target);
        let command = match (ctx.document.layer(pending.layer), after, document_region) {
            (Some(layer), Some(after), Some(region)) => {
                let region = region.in_buffer(
                    (layer.position.x, layer.position.y),
                    after.width,
                    after.height,
                );
                PaintCommand::capture_in_target(
                    pending.target,
                    pending.layer,
                    region,
                    &pending.before,
                    after,
                    label,
                )
            }
            _ => None,
        };
        if let Some(command) = command {
            ctx.history.execute(Box::new(command), ctx.document);
        }
    }

    /// 沿一条折线描一遍（钢笔的「描边路径」用）。
    ///
    /// 段内是**直线**（控制点取段首，不做中点平滑，折角保持折角），但间距、
    /// 每笔不透明度、硬度 / 预设、选区裁剪和「一条 undo」都和手画一笔一样。
    pub fn stroke_path(&mut self, ctx: &mut ToolContext, points: &[Vec2]) {
        let Some(&first) = points.first() else {
            return;
        };
        self.drawing = true;
        self.begin_stroke(ctx);

        let mut dabs = std::mem::take(&mut self.dabs);
        dabs.clear();
        dabs.push(first);
        self.paint_dabs(ctx.document, &mut dabs);
        for pair in points.windows(2) {
            dabs.clear();
            stroke::curve_dabs(pair[0], pair[0], pair[1], self.spacing(), &mut dabs);
            self.paint_dabs(ctx.document, &mut dabs);
        }
        self.dabs = dabs;

        self.drawing = false;
        self.commit_stroke(ctx);
    }

    /// 清掉采样器状态（**不动** `stroke_dirty`：抬笔前还要用它提交撤销区域）。
    fn reset_sampling(&mut self) {
        self.last = None;
        self.previous = None;
        self.painted = None;
    }
}

impl Default for BrushTool {
    fn default() -> Self {
        Self::paint()
    }
}

/// 像素笔：把上一个落笔像素到 `to` 之间的像素按 Bresenham 依次压一遍。
///
/// 采样点之间只隔 1px 左右，连起来才不会斜向断开。重复压到同一个像素时
/// 蒙版让它只合成一次，所以这里不需要自己去重。
fn stroke_pixels(writer: &mut Writer, from: Option<Vec2>, to: Vec2) -> Option<Vec2> {
    let (target_x, target_y) = (Tip::pixel_index(to.x), Tip::pixel_index(to.y));
    let Some(from) = from else {
        writer.put(target_x, target_y, 1.0);
        return Some(to);
    };
    let (mut x, mut y) = (Tip::pixel_index(from.x), Tip::pixel_index(from.y));
    if (x, y) == (target_x, target_y) {
        return Some(to);
    }
    let dx = (target_x - x).abs();
    let dy = -(target_y - y).abs();
    let step_x = if x < target_x { 1 } else { -1 };
    let step_y = if y < target_y { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += step_x;
        }
        if e2 <= dx {
            err += dx;
            y += step_y;
        }
        writer.put(x, y, 1.0);
        if x == target_x && y == target_y {
            break;
        }
    }
    Some(to)
}

impl Tool for BrushTool {
    fn name(&self) -> &'static str {
        match self.mode {
            BrushMode::Paint => "画笔",
            BrushMode::Erase => "橡皮",
        }
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolContext, event: PointerEvent) {
        self.drawing = true;
        self.begin_stroke(ctx);
        self.stroke_to(ctx.document, event.position);
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolContext, event: PointerEvent) {
        if self.drawing {
            self.stroke_to(ctx.document, event.position);
        }
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolContext, event: PointerEvent) {
        // 平滑只能画到「当前点与下一个点的中点」，最后半段在抬笔时补齐。
        self.finish_stroke(ctx.document, event.position);
        self.drawing = false;
        self.commit_stroke(ctx);
    }
}

#[cfg(test)]
mod tests;
