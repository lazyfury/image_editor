//! 画笔 / 橡皮：同一个引擎，只有 [`BrushMode`] 不同（§11）。
//!
//! 一次 stroke 的入口是 `on_pointer_down`，持续在 `on_pointer_move`，在
//! `on_pointer_up` 结束。每次 move 只往当前图层的 `PixelBuffer` 写像素；
//! 落笔时先快照整层，抬笔时用 [`PixelRegion::diff`] 抓出差异区域，
//! 提交成**一条** [`PaintCommand`] —— 一笔 = 一步 undo。
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
use crate::document::{Color, Document, LayerId, PaintCommand, PixelBuffer};

pub use tip::BrushShape;

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
    pub color: Color,
    pub mode: BrushMode,
    /// 可选选区：只有落在里面的像素会被写（Phase 8 / 12 的选区裁剪）。
    /// 视图在落笔前从编辑器状态同步过来；`Rc` 共享，克隆便宜。
    pub clip: Option<Rc<Selection>>,
    /// 像素模式：边缘不做抗锯齿（`coverage` 只有 0 / 1）。
    pub hard: bool,
    /// 笔刷形状。
    pub shape: BrushShape,
    drawing: bool,
    /// 采样器状态：上一个 / 上上个光标点（见 [`stroke`]）。
    last: Option<Vec2>,
    previous: Option<Vec2>,
    /// 像素笔已经落笔到的位置（Bresenham 连线的起点）。
    painted: Option<Vec2>,
    /// 复用的等距采样缓冲，避免每个光标事件都分配。
    dabs: Vec<Vec2>,
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
            hard: true,
            shape: BrushShape::Square,
            drawing: false,
            last: None,
            previous: None,
            painted: None,
            dabs: Vec::new(),
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
            let from = self
                .previous
                .map_or(last, |previous| stroke::midpoint(previous, last));
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
            // 第一个点：立即压一次，等第二个点到了再开始连线。
            out.push(to);
            self.last = Some(to);
            return;
        };
        if to == last {
            // 原地不动的 move 事件（有些系统会连发）不该再压一个点。
            return;
        }
        let from = self
            .previous
            .map_or(last, |previous| stroke::midpoint(previous, last));
        let end = stroke::midpoint(last, to);
        stroke::curve_dabs(from, last, end, self.spacing(), out);
        self.previous = Some(last);
        self.last = Some(to);
    }

    /// 把一批等距采样点压到当前图层上。
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
        self.painted = painted;
        dabs.clear();
    }

    /// 当前这一笔的写入上下文（目标图层 + 落笔前快照 + 本笔蒙版）。
    fn writer<'a>(&'a mut self, document: &'a mut Document) -> Option<Writer<'a>> {
        let layer = document.active_layer_mut()?;
        let offset = layer.position;
        let target = &mut layer.pixels;
        let clip = self.clip.clone();
        let (mode, color, opacity) = (self.mode, self.color, self.opacity);
        let pending = self.pending.as_mut()?;
        let PendingStroke { before, mask, .. } = pending;
        Some(Writer {
            mode,
            color,
            opacity,
            clip,
            offset,
            before,
            mask,
            target,
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
        if let Some(id) = ctx.document.active_layer().map(|layer| layer.id) {
            let (dx, dy) = ctx.document.ensure_layer_covers_document(id);
            ctx.history.translate_layer(id, dx, dy);
        }
        self.pending = ctx.document.active_layer().map(|layer| PendingStroke {
            layer: layer.id,
            before: layer.pixels.clone(),
            mask: StrokeMask::new(layer.pixels.width, layer.pixels.height),
        });
    }

    /// 收一笔：整笔的像素差异提交成**一条**命令；没有变化就不入栈。
    fn commit_stroke(&mut self, ctx: &mut ToolContext) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let command = match ctx.document.layer(pending.layer) {
            Some(layer) => {
                PaintCommand::capture(pending.layer, &pending.before, &layer.pixels, self.name())
            }
            None => None,
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

    /// 清掉一条笔触的采样状态。
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
mod tests {
    use super::*;
    use crate::document::{History, PixelRegion};

    fn document() -> Document {
        Document::new("d", 32, 32)
    }

    /// 一个最小的工具上下文：文档 + 该测试自己的空历史。
    fn context<'a>(document: &'a mut Document, history: &'a mut History) -> ToolContext<'a> {
        ToolContext { document, history }
    }

    fn left(position: Vec2) -> PointerEvent {
        PointerEvent {
            position,
            button: igui_core::PointerButton::Left,
        }
    }

    /// 画一串点（自己开 / 收一笔）。
    ///
    /// `stroke_to` 需要一笔真实的上下文（落笔快照 + 覆盖率蒙版），所以测试
    /// 统一走这个入口而不是直接调 `stroke_to`。
    fn stroke(brush: &mut BrushTool, document: &mut Document, points: &[Vec2]) {
        let Some(&first) = points.first() else {
            return;
        };
        let mut history = History::new();
        let mut ctx = context(document, &mut history);
        brush.on_pointer_down(&mut ctx, left(first));
        for point in points.iter().skip(1) {
            brush.on_pointer_move(&mut ctx, left(*point));
        }
        let last = *points.last().unwrap_or(&first);
        brush.on_pointer_up(&mut ctx, left(last));
    }

    fn at(document: &Document, x: u32, y: u32) -> Color {
        document.active_layer().unwrap().pixels.get_pixel(x, y)
    }

    /// 当前图层上所有非白像素（按行优先）：用来看「画了哪些像素」。
    fn painted(document: &Document) -> Vec<(u32, u32)> {
        let pixels = &document.active_layer().unwrap().pixels;
        (0..pixels.height)
            .flat_map(|y| (0..pixels.width).map(move |x| (x, y)))
            .filter(|(x, y)| at(document, *x, *y) != Color::WHITE)
            .collect()
    }

    #[test]
    fn a_single_dab_paints_the_center_of_the_layer() {
        let mut document = document();
        let mut brush = BrushTool::paint().with_color(Color::RED);
        stroke(&mut brush, &mut document, &[Vec2::new(16.0, 16.0)]);
        let color = at(&document, 16, 16);
        assert_eq!((color.r, color.g, color.b), (255, 0, 0));
        // 相邻像素还是背景（1px 笔只填光标下的那个像素）。
        assert_eq!(at(&document, 30, 30), Color::WHITE);
    }

    #[test]
    fn a_one_pixel_brush_snaps_to_the_pixel_under_the_cursor() {
        let mut document = document();
        let mut brush = BrushTool::paint().with_color(Color::RED);
        // 落点在像素 (10, 20) 内部而非中心：像素笔应实心填满该像素、无灰边。
        stroke(&mut brush, &mut document, &[Vec2::new(10.3, 20.7)]);
        assert_eq!(at(&document, 10, 20), Color::RED);
        assert_eq!(at(&document, 9, 20), Color::WHITE, "左邻不沾");
        assert_eq!(at(&document, 10, 19), Color::WHITE, "上邻不沾");
        assert_eq!(at(&document, 11, 21), Color::WHITE, "右下不沾");
    }

    #[test]
    fn hard_edges_paint_only_solid_pixels() {
        let mut document = document();
        let mut brush = BrushTool::paint().with_color(Color::RED);
        brush.size = 3.0;
        stroke(&mut brush, &mut document, &[Vec2::new(16.0, 16.0)]);
        // 硬边方形 3×3：全是实心红，没有半透明边。
        for y in 15..=17 {
            for x in 15..=17 {
                assert_eq!(at(&document, x, y), Color::RED, "({x}, {y})");
            }
        }
        assert_eq!(at(&document, 14, 16), Color::WHITE);
        assert_eq!(at(&document, 18, 16), Color::WHITE);
    }

    #[test]
    fn the_soft_brush_keeps_anti_aliased_edges() {
        let mut document = document();
        let mut brush = BrushTool::paint().with_color(Color::RED);
        brush.size = 3.0;
        brush.hard = false;
        stroke(&mut brush, &mut document, &[Vec2::new(16.0, 16.0)]);
        // 负向对照：软笔会产生中间色（既不是纯红也不是纯白）。
        let partial = (14..=18).any(|y| {
            (14..=18).any(|x| {
                let color = at(&document, x, y);
                color != Color::RED && color != Color::WHITE && color.a != 0
            })
        });
        assert!(partial, "软笔应产生抗锯齿边");
    }

    #[test]
    fn a_half_opacity_stroke_stays_half_transparent_along_the_whole_line() {
        // 软圆笔 20px、50% 不透明度：笔迹中心沿整条线一样深。旧实现是每个
        // stamp 混一次（间距 = 直径/4），同一像素被压 4 次，画出来接近全黑。
        let mut document = document();
        let mut brush = BrushTool::paint().with_color(Color::BLACK);
        brush.size = 20.0;
        brush.hard = false;
        brush.shape = BrushShape::Round;
        brush.opacity = 0.5;
        stroke(
            &mut brush,
            &mut document,
            &[Vec2::new(8.0, 16.0), Vec2::new(24.0, 16.0)],
        );
        for x in 10..=22 {
            let color = at(&document, x, 16);
            assert!(
                (color.r as i32 - 128).abs() <= 2,
                "x = {x}: r = {}",
                color.r
            );
        }
    }

    #[test]
    fn two_half_opacity_strokes_build_up_to_seventy_five_percent() {
        // 每一笔各自封顶 50%，两笔相交才是标准的 1 - (1 - 0.5)^2 = 75%。
        let mut document = document();
        let mut brush = BrushTool::paint().with_color(Color::BLACK);
        brush.opacity = 0.5;
        for _ in 0..2 {
            stroke(&mut brush, &mut document, &[Vec2::new(16.0, 16.0)]);
        }
        let color = at(&document, 16, 16);
        assert!((color.r as i32 - 64).abs() <= 2, "r = {}", color.r);
    }

    #[test]
    fn a_path_stroke_covers_every_segment_and_is_one_undo_step() {
        // 钢笔墨径描边：折线两段都要打满，而且整条路径只算一步。
        let mut document = document();
        let mut history = History::new();
        let mut brush = BrushTool::paint().with_color(Color::RED);
        {
            let mut ctx = context(&mut document, &mut history);
            brush.stroke_path(
                &mut ctx,
                &[
                    Vec2::new(2.0, 2.0),
                    Vec2::new(28.0, 2.0),
                    Vec2::new(28.0, 28.0),
                ],
            );
        }
        assert_eq!(history.undo_len(), 1, "整条路径 = 一步 undo");
        for x in 2..=28 {
            assert_eq!(at(&document, x, 2).r, 255, "横段 x = {x}");
        }
        for y in 2..=28 {
            assert_eq!(at(&document, 28, y).r, 255, "竖段 y = {y}");
        }
    }

    #[test]
    fn a_path_stroke_keeps_the_corner_and_does_not_bulge() {
        // 折角不能被平滑抹圆：拐角像素要压到，拐角外侧的像素不压。
        let mut document = document();
        let mut history = History::new();
        let mut brush = BrushTool::paint().with_color(Color::RED);
        {
            let mut ctx = context(&mut document, &mut history);
            brush.stroke_path(
                &mut ctx,
                &[
                    Vec2::new(4.0, 20.0),
                    Vec2::new(20.0, 20.0),
                    Vec2::new(20.0, 4.0),
                ],
            );
        }
        assert_eq!(at(&document, 20, 20).r, 255, "拐角要压到");
        assert_eq!(at(&document, 18, 22).r, 255, "拐角内侧也要压到");
    }

    #[test]
    fn a_soft_stroke_fades_towards_the_edge() {
        // 硬度 0 的圆笔：边缘像素明显比中心淡（硬度真的进了 stamp）。
        let mut document = Document::new("d", 64, 64);
        let mut brush = BrushTool::paint().with_color(Color::BLACK);
        brush.size = 20.0;
        brush.hard = false;
        brush.shape = BrushShape::Round;
        brush.hardness = 0.0;
        stroke(&mut brush, &mut document, &[Vec2::new(32.0, 32.0)]);
        let center = at(&document, 32, 32).r;
        let edge = at(&document, 41, 32).r;
        assert!(center < 20, "center = {center}");
        assert!(edge > 150 && edge < 250, "edge = {edge}");
    }

    #[test]
    fn an_even_square_brush_of_size_two_paints_four_pixels() {
        let mut document = document();
        let mut brush = BrushTool::paint().with_color(Color::RED);
        brush.size = 2.0;
        stroke(&mut brush, &mut document, &[Vec2::new(16.0, 16.0)]);
        // n=2：光标像素放左上，(16,16)..(17,17)。
        for (x, y) in [(16, 16), (17, 16), (16, 17), (17, 17)] {
            assert_eq!(at(&document, x, y), Color::RED, "({x}, {y})");
        }
        assert_eq!(at(&document, 15, 16), Color::WHITE);
        assert_eq!(at(&document, 18, 16), Color::WHITE);
    }

    #[test]
    fn a_one_pixel_hard_line_is_a_clean_bresenham_line() {
        let mut document = document();
        let mut history = History::new();
        let mut brush = BrushTool::paint().with_color(Color::RED);
        let mut ctx = context(&mut document, &mut history);
        brush.on_pointer_down(&mut ctx, left(Vec2::new(4.0, 4.0)));
        brush.on_pointer_move(&mut ctx, left(Vec2::new(11.0, 11.0)));
        brush.on_pointer_up(&mut ctx, left(Vec2::new(11.0, 11.0)));
        // 45° 斜线：正好 8 个像素，不会因重复压角 / 插值而加粗成 L 型。
        assert_eq!(painted(&document).len(), 8, "45° 1px 斜线应只有 8 个像素");
    }

    #[test]
    fn a_fast_stroke_paints_the_same_pixels_as_a_slow_one() {
        // 快速拖动时系统给的采样点很稀：按固定距离补出来的笔迹应该和慢慢画
        // （事件密）一模一样，而不是只剩几段直尺线。
        let mut fast_document = document();
        let mut fast_history = History::new();
        let mut fast = BrushTool::paint().with_color(Color::RED);
        {
            let mut ctx = context(&mut fast_document, &mut fast_history);
            fast.on_pointer_down(&mut ctx, left(Vec2::new(2.0, 2.0)));
            fast.on_pointer_move(&mut ctx, left(Vec2::new(30.0, 2.0)));
            fast.on_pointer_up(&mut ctx, left(Vec2::new(30.0, 2.0)));
        }

        let mut slow_document = document();
        let mut slow_history = History::new();
        let mut slow = BrushTool::paint().with_color(Color::RED);
        {
            let mut ctx = context(&mut slow_document, &mut slow_history);
            slow.on_pointer_down(&mut ctx, left(Vec2::new(2.0, 2.0)));
            for x in 3..=30 {
                slow.on_pointer_move(&mut ctx, left(Vec2::new(x as f32, 2.0)));
            }
            slow.on_pointer_up(&mut ctx, left(Vec2::new(30.0, 2.0)));
        }

        assert_eq!(painted(&fast_document), painted(&slow_document));
    }

    #[test]
    fn a_sparse_corner_is_rounded_instead_of_hitting_the_corner() {
        // 三个稀疏点画一个直角：二次中点平滑把拐角抹圆，所以拐角像素不会被
        // 压到（折线会），而两端仍然精确落笔。
        let mut document = document();
        let mut history = History::new();
        let mut brush = BrushTool::paint().with_color(Color::RED);
        let mut ctx = context(&mut document, &mut history);
        brush.on_pointer_down(&mut ctx, left(Vec2::new(4.0, 4.0)));
        brush.on_pointer_move(&mut ctx, left(Vec2::new(20.0, 4.0)));
        brush.on_pointer_move(&mut ctx, left(Vec2::new(20.0, 20.0)));
        brush.on_pointer_up(&mut ctx, left(Vec2::new(20.0, 20.0)));

        let pixels = painted(&document);
        assert!(!pixels.contains(&(20, 4)), "拐角应被抹圆：{pixels:?}");
        assert!(pixels.contains(&(4, 4)), "落笔点要压到");
        assert!(pixels.contains(&(20, 12)), "下一段的起点要接上");
        assert!(pixels.contains(&(20, 20)), "抬笔位置要压到");
    }

    #[test]
    fn a_one_pixel_stroke_never_doubles_a_pixel() {
        // 1px 像素笔 + 50% 不透明度：同一个像素只该压一次，所以斜线上下两端的
        // 颜色一样深（按事件点重复压会在一部分像素上叠出深点）。
        let mut document = document();
        let mut history = History::new();
        let mut brush = BrushTool::paint().with_color(Color::BLACK);
        brush.opacity = 0.5;
        let mut ctx = context(&mut document, &mut history);
        brush.on_pointer_down(&mut ctx, left(Vec2::new(4.0, 4.0)));
        for step in 1..=7 {
            let point = Vec2::new(4.0 + step as f32, 4.0 + step as f32);
            brush.on_pointer_move(&mut ctx, left(point));
        }
        brush.on_pointer_up(&mut ctx, left(Vec2::new(11.0, 11.0)));

        let colors: Vec<Color> = painted(&document)
            .into_iter()
            .map(|(x, y)| at(&document, x, y))
            .collect();
        assert_eq!(colors.len(), 8, "45° 1px 斜线：{colors:?}");
        assert!(
            colors.windows(2).all(|pair| pair[0] == pair[1]),
            "{colors:?}"
        );
    }

    #[test]
    fn painting_after_moving_a_layer_lands_under_the_cursor() {
        let mut document = document();
        let mut history = History::new();
        let id = document.active_layer().unwrap().id;
        // 图层右移 10：左边 10 格在文档里空出来。
        document.set_layer_position(id, crate::document::Point::new(10, 0));

        // 直接在空出来的文档坐标 (2, 2) 落笔。
        let mut brush = BrushTool::paint().with_color(Color::RED);
        brush.on_pointer_down(
            &mut context(&mut document, &mut history),
            left(Vec2::new(2.0, 2.0)),
        );
        brush.on_pointer_up(
            &mut context(&mut document, &mut history),
            left(Vec2::new(2.0, 2.0)),
        );

        let layer = document.active_layer().unwrap();
        assert_eq!(
            layer.position,
            crate::document::Point::ZERO,
            "落笔前烘进像素"
        );
        assert_eq!(
            layer.pixels.get_pixel(2, 2),
            Color::RED,
            "笔迹在文档坐标下落笔"
        );
        assert_eq!(
            layer.pixels.get_pixel(12, 2),
            Color::WHITE,
            "不会偏移到 +10"
        );
    }

    #[test]
    fn undo_after_moving_the_layer_still_targets_the_moved_stroke() {
        let mut document = document();
        let mut history = History::new();
        let id = document.active_layer().unwrap().id;
        let mut brush = BrushTool::paint().with_color(Color::RED);
        // 笔画 A 在文档 (4, 4)。
        brush.on_pointer_down(
            &mut context(&mut document, &mut history),
            left(Vec2::new(4.0, 4.0)),
        );
        brush.on_pointer_up(
            &mut context(&mut document, &mut history),
            left(Vec2::new(4.0, 4.0)),
        );
        // 图层右移 10，A 的像素跟着去缓冲区索引 14。
        document.set_layer_position(id, crate::document::Point::new(10, 0));
        // 笔画 B 触发缓冲区扩展 + 历史区域平移。
        brush.on_pointer_down(
            &mut context(&mut document, &mut history),
            left(Vec2::new(20.0, 20.0)),
        );
        brush.on_pointer_up(
            &mut context(&mut document, &mut history),
            left(Vec2::new(20.0, 20.0)),
        );

        assert_eq!(history.undo(&mut document), Some("画笔"), "先撤 B");
        assert_eq!(history.undo(&mut document), Some("画笔"), "再撤 A");
        let layer = document.active_layer().unwrap();
        assert_eq!(layer.position, crate::document::Point::ZERO);
        assert_eq!(
            layer.pixels.get_pixel(14, 4),
            Color::WHITE,
            "撤销 A 应命中平移后的坐标"
        );
    }

    #[test]
    fn erasing_lowers_alpha_instead_of_painting() {
        let mut document = document();
        let mut brush = BrushTool::eraser();
        stroke(&mut brush, &mut document, &[Vec2::new(16.0, 16.0)]);
        assert_eq!(at(&document, 16, 16).a, 0);
        assert_eq!(at(&document, 30, 30).a, 255, "画布角落还在");
    }

    #[test]
    fn a_move_stroke_covers_the_points_between_samples() {
        let mut document = document();
        let mut history = History::new();
        let mut brush = BrushTool::paint().with_color(Color::RED);
        brush.on_pointer_down(
            &mut context(&mut document, &mut history),
            left(Vec2::new(4.0, 16.0)),
        );
        assert!(brush.is_drawing());
        brush.on_pointer_move(
            &mut context(&mut document, &mut history),
            left(Vec2::new(28.0, 16.0)),
        );
        brush.on_pointer_up(
            &mut context(&mut document, &mut history),
            left(Vec2::new(28.0, 16.0)),
        );
        // 两端之间的中间像素也被覆盖（插值有效）。
        for x in 4..=28 {
            assert_eq!(at(&document, x, 16).r, 255, "x = {x} 应该被线段覆盖");
        }
    }

    #[test]
    fn opacity_blends_towards_the_background() {
        let mut document = document();
        let mut brush = BrushTool::paint().with_color(Color::BLACK);
        brush.opacity = 0.5;
        stroke(&mut brush, &mut document, &[Vec2::new(16.0, 16.0)]);
        let color = at(&document, 16, 16);
        assert_eq!(color.r, color.g);
        assert!(color.r > 100 && color.r < 160, "r = {}", color.r);
    }

    #[test]
    fn out_of_bounds_stamps_are_clipped_not_panicking() {
        // 圆心在角落：一半在界外、一半在界内，界内那半应被画上。
        let mut inside = document();
        let mut brush = BrushTool::paint().with_color(Color::RED);
        stroke(&mut brush, &mut inside, &[Vec2::new(0.0, 0.0)]);
        assert_eq!(at(&inside, 0, 0), Color::RED);

        // 完全在界外：不 panic，也不动界内。
        let mut outside = document();
        let mut brush = BrushTool::paint().with_color(Color::RED);
        stroke(&mut brush, &mut outside, &[Vec2::new(-50.0, -50.0)]);
        assert_eq!(at(&outside, 0, 0), Color::WHITE);
    }

    #[test]
    fn a_clip_region_confines_the_stroke() {
        let mut document = document();
        let size = (document.width, document.height);
        let mut brush = BrushTool::paint().with_color(Color::RED);
        brush.clip = Some(Rc::new(Selection::rect(
            PixelRegion::new(14, 14, 4, 4),
            size,
        )));
        stroke(&mut brush, &mut document, &[Vec2::new(16.0, 16.0)]);
        assert_eq!(at(&document, 16, 16), Color::RED, "选区内落笔");
        assert_eq!(at(&document, 20, 16), Color::WHITE, "选区外不落笔");
    }

    #[test]
    #[ignore = "perf probe: --ignored --nocapture"]
    fn probe_stroke_cost() {
        use std::time::Instant;
        let mut document = Document::new("d", 800, 600);
        let mut brush = BrushTool::paint().with_color(Color::RED);
        let n = 200;
        let timer = Instant::now();
        for _ in 0..n {
            stroke(
                &mut brush,
                &mut document,
                &[Vec2::new(100.0, 100.0), Vec2::new(700.0, 500.0)],
            );
        }
        let ms = timer.elapsed().as_secs_f64() / n as f64 * 1000.0;
        eprintln!("stroke (600px, size 1) {ms:.3} ms");
    }

    #[test]
    fn a_full_stroke_begin_to_end_toggles_drawing() {
        let mut document = document();
        let mut history = History::new();
        let mut brush = BrushTool::paint();
        assert!(!brush.is_drawing());
        brush.on_pointer_down(
            &mut context(&mut document, &mut history),
            left(Vec2::new(8.0, 8.0)),
        );
        assert!(brush.is_drawing());
        brush.on_pointer_up(
            &mut context(&mut document, &mut history),
            left(Vec2::new(8.0, 8.0)),
        );
        assert!(!brush.is_drawing());
        assert_eq!(brush.last, None);
    }

    #[test]
    fn one_stroke_pushes_exactly_one_undo_step() {
        let mut document = document();
        let mut history = History::new();
        let mut brush = BrushTool::paint().with_color(Color::RED);
        brush.on_pointer_down(
            &mut context(&mut document, &mut history),
            left(Vec2::new(4.0, 4.0)),
        );
        // 中间很多次 move，也只算**一笔**。
        for x in [8.0, 12.0, 16.0, 20.0] {
            brush.on_pointer_move(
                &mut context(&mut document, &mut history),
                left(Vec2::new(x, 4.0)),
            );
        }
        brush.on_pointer_up(
            &mut context(&mut document, &mut history),
            left(Vec2::new(20.0, 4.0)),
        );
        assert_eq!(history.undo_len(), 1);
    }

    #[test]
    fn undo_restores_the_pixels_and_redo_reapplies_them() {
        let mut document = document();
        let mut history = History::new();
        let mut brush = BrushTool::paint().with_color(Color::RED);
        brush.on_pointer_down(
            &mut context(&mut document, &mut history),
            left(Vec2::new(16.0, 16.0)),
        );
        brush.on_pointer_up(
            &mut context(&mut document, &mut history),
            left(Vec2::new(16.0, 16.0)),
        );
        assert_eq!(at(&document, 16, 16), Color::RED);

        assert_eq!(history.undo(&mut document), Some("画笔"));
        assert_eq!(at(&document, 16, 16), Color::WHITE, "撤销回到落笔前");

        assert_eq!(history.redo(&mut document), Some("画笔"));
        assert_eq!(at(&document, 16, 16), Color::RED);
    }

    #[test]
    fn a_stroke_with_no_pixel_change_pushes_no_command() {
        let mut document = document();
        let mut history = History::new();
        let mut brush = BrushTool::paint().with_color(Color::RED);
        // 整笔都在画布外：没有像素变化，不该占用一步撤销。
        brush.on_pointer_down(
            &mut context(&mut document, &mut history),
            left(Vec2::new(-100.0, -100.0)),
        );
        brush.on_pointer_up(
            &mut context(&mut document, &mut history),
            left(Vec2::new(-100.0, -100.0)),
        );
        assert!(!history.can_undo());
    }
}
