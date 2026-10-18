//! 油漆桶：洪水填充（容差 / 连续 / 全局），按选区裁剪。
//!
//! 纯算法是 [`flood_fill`]（只认 `PixelBuffer` + 文档坐标的 [`Selection`]）；
//! [`PaintBucketTool`] 把它包成一个 [`Tool`]：落笔前快照整层、填充后用
//! [`PixelRegion::diff`] 抓差异，一条 undo。

use std::rc::Rc;

use super::tool::{PointerEvent, Tool, ToolContext};
use crate::canvas::{document_to_pixel, Selection};
use crate::document::{Color, PaintCommand, PixelBuffer, Point};

/// 两个颜色是否在容差内（比较 RGBA 每个通道的最大差）。
fn within_tolerance(a: Color, b: Color, tolerance: u8) -> bool {
    let diff = |x: u8, y: u8| x.abs_diff(y);
    diff(a.r, b.r) <= tolerance
        && diff(a.g, b.g) <= tolerance
        && diff(a.b, b.b) <= tolerance
        && diff(a.a, b.a) <= tolerance
}

/// 从文档坐标 `seed` 洪水填充。
///
/// `offset` 是图层缓冲区原点（文档坐标，`ensure_layer_covers_document` 之后
/// `<= 0`），`document` 是文档尺寸（限制填充范围），`clip` 是文档坐标的选区。
/// 返回是否有像素变化。
// 低层填充 API：缓冲区 + 几何 + 填充参数 + 裁剪，就是它的全部输入。
#[allow(clippy::too_many_arguments)]
pub fn flood_fill(
    buffer: &mut PixelBuffer,
    offset: Point,
    document: (u32, u32),
    seed: (u32, u32),
    color: Color,
    tolerance: u8,
    contiguous: bool,
    clip: Option<&Selection>,
) -> bool {
    let width = buffer.width;
    let height = buffer.height;
    // 缓冲区像素是否可填：落在文档内、选区（若有）内、缓冲区覆盖范围内。
    let fillable = |bx: u32, by: u32| -> bool {
        let dx = bx as i64 + offset.x as i64;
        let dy = by as i64 + offset.y as i64;
        if dx < 0 || dy < 0 || dx >= document.0 as i64 || dy >= document.1 as i64 {
            return false;
        }
        clip.map_or(true, |selection| selection.contains(dx as u32, dy as u32))
    };
    let Some((sx, sy)) = buffer_seed(seed, offset, width, height) else {
        return false;
    };
    if !fillable(sx, sy) {
        return false;
    }
    let target = buffer.get_pixel(sx, sy);
    if target == color {
        return false;
    }

    if !contiguous {
        let mut changed = false;
        for by in 0..height {
            for bx in 0..width {
                if !fillable(bx, by) {
                    continue;
                }
                let pixel = buffer.get_pixel(bx, by);
                if within_tolerance(pixel, target, tolerance) && pixel != color {
                    buffer.set_pixel(bx, by, color);
                    changed = true;
                }
            }
        }
        return changed;
    }

    // 连续：栈 + `visited`，四连通。`visited` 而不是“填完再比色”，所以
    // 即使填充色仍在容差内也不会反复入栈。
    let mut visited = vec![false; width as usize * height as usize];
    let mut stack = vec![(sx, sy)];
    let mut changed = false;
    while let Some((bx, by)) = stack.pop() {
        let index = by as usize * width as usize + bx as usize;
        if visited[index] || !fillable(bx, by) {
            continue;
        }
        if !within_tolerance(buffer.get_pixel(bx, by), target, tolerance) {
            continue;
        }
        visited[index] = true;
        buffer.set_pixel(bx, by, color);
        changed = true;
        if bx > 0 {
            stack.push((bx - 1, by));
        }
        if bx + 1 < width {
            stack.push((bx + 1, by));
        }
        if by > 0 {
            stack.push((bx, by - 1));
        }
        if by + 1 < height {
            stack.push((bx, by + 1));
        }
    }
    changed
}

/// 文档像素 -> 图层缓冲区像素；不在缓冲区里返回 `None`。
fn buffer_seed(seed: (u32, u32), offset: Point, width: u32, height: u32) -> Option<(u32, u32)> {
    let bx = seed.0 as i64 - offset.x as i64;
    let by = seed.1 as i64 - offset.y as i64;
    if bx < 0 || by < 0 || bx >= width as i64 || by >= height as i64 {
        return None;
    }
    Some((bx as u32, by as u32))
}

/// 把 `region`（∩ `clip`，若有）内的像素全部填成 `color`，提交一条 undo。
///
/// 矩形填充 / 自由填充共用：`region` 是矩形或自由折线围出的 [`Selection`]。
pub fn fill_selection(
    ctx: &mut ToolContext,
    region: &Selection,
    clip: Option<&Selection>,
    color: Color,
) -> bool {
    let Some(layer_id) = ctx.document.active_layer().map(|layer| layer.id) else {
        return false;
    };
    let (dx, dy) = ctx.document.ensure_layer_covers_document(layer_id);
    ctx.history.translate_layer(layer_id, dx, dy);
    let Some(before) = ctx
        .document
        .layer(layer_id)
        .map(|layer| layer.pixels.clone())
    else {
        return false;
    };
    let (width, height) = (ctx.document.width, ctx.document.height);
    let offset = ctx
        .document
        .layer(layer_id)
        .map(|layer| layer.position)
        .unwrap_or(Point::new(0, 0));
    let changed = {
        let Some(layer) = ctx.document.layer_mut(layer_id) else {
            return false;
        };
        let mut changed = false;
        for y in 0..height {
            for x in 0..width {
                if !region.contains(x, y) || clip.is_some_and(|c| !c.contains(x, y)) {
                    continue;
                }
                let bx = x as i64 - offset.x as i64;
                let by = y as i64 - offset.y as i64;
                if bx < 0
                    || by < 0
                    || bx >= layer.pixels.width as i64
                    || by >= layer.pixels.height as i64
                {
                    continue;
                }
                let (bx, by) = (bx as u32, by as u32);
                if layer.pixels.get_pixel(bx, by) != color {
                    layer.pixels.set_pixel(bx, by, color);
                    changed = true;
                }
            }
        }
        changed
    };
    if !changed {
        return false;
    }
    let command = ctx
        .document
        .layer(layer_id)
        .and_then(|layer| PaintCommand::capture(layer_id, &before, &layer.pixels, "填充"));
    match command {
        Some(command) => {
            ctx.history.execute(Box::new(command), ctx.document);
            true
        }
        None => false,
    }
}

/// 油漆桶工具。颜色 / 容差 / 连续 / 选区由视图在落笔前同步。
#[derive(Debug, Clone, PartialEq)]
pub struct PaintBucketTool {
    pub color: Color,
    /// 0..=255，每个 RGBA 通道允许的最大差。
    pub tolerance: u8,
    /// `true` = 只填连通的同色区域；`false` = 填全图所有匹配像素。
    pub contiguous: bool,
    pub clip: Option<Rc<Selection>>,
}

impl PaintBucketTool {
    /// 容差步进。
    pub const TOLERANCE_STEP: u8 = 8;

    pub fn new() -> Self {
        Self {
            color: Color::BLACK,
            tolerance: 0,
            contiguous: true,
            clip: None,
        }
    }
}

impl Default for PaintBucketTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for PaintBucketTool {
    fn name(&self) -> &'static str {
        "油漆桶"
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolContext, event: PointerEvent) {
        let document_size = (ctx.document.width, ctx.document.height);
        let Some(seed) = document_to_pixel(event.position, document_size.0, document_size.1) else {
            return;
        };
        let Some(layer_id) = ctx.document.active_layer().map(|layer| layer.id) else {
            return;
        };
        // 缓冲区扩到覆盖整张文档，落点在文档内就一定能填到（跟画笔一致）。
        let (dx, dy) = ctx.document.ensure_layer_covers_document(layer_id);
        ctx.history.translate_layer(layer_id, dx, dy);
        let Some(before) = ctx
            .document
            .layer(layer_id)
            .map(|layer| layer.pixels.clone())
        else {
            return;
        };
        let offset = ctx
            .document
            .layer(layer_id)
            .map(|layer| layer.position)
            .unwrap_or(Point::new(0, 0));
        let changed = match ctx.document.layer_mut(layer_id) {
            Some(layer) => flood_fill(
                &mut layer.pixels,
                offset,
                document_size,
                seed,
                self.color,
                self.tolerance,
                self.contiguous,
                self.clip.as_deref(),
            ),
            None => false,
        };
        if !changed {
            return;
        }
        let command = ctx
            .document
            .layer(layer_id)
            .and_then(|layer| PaintCommand::capture(layer_id, &before, &layer.pixels, self.name()));
        if let Some(command) = command {
            ctx.history.execute(Box::new(command), ctx.document);
        }
    }

    fn on_pointer_move(&mut self, _ctx: &mut ToolContext, _event: PointerEvent) {}
    fn on_pointer_up(&mut self, _ctx: &mut ToolContext, _event: PointerEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, History, PixelRegion};

    const BLUE: Color = Color::rgb(0, 0, 255);

    fn blank() -> Document {
        Document::new("d", 16, 16)
    }

    fn paint(document: &mut Document, x: u32, y: u32, color: Color) {
        document
            .active_layer_mut()
            .unwrap()
            .pixels
            .set_pixel(x, y, color);
    }

    fn at(document: &Document, x: u32, y: u32) -> Color {
        document.active_layer().unwrap().pixels.get_pixel(x, y)
    }

    #[test]
    fn a_flood_fills_a_connected_region_and_stops_at_the_border() {
        let mut document = blank();
        // 左半边红，x=6 一条黑线，右边白。
        for y in 0..16 {
            for x in 0..6 {
                paint(&mut document, x, y, Color::RED);
            }
            paint(&mut document, 6, y, Color::BLACK);
        }
        let layer = document.active_layer_mut().unwrap();
        let changed = flood_fill(
            &mut layer.pixels,
            Point::new(0, 0),
            (16, 16),
            (0, 0),
            BLUE,
            0,
            true,
            None,
        );
        assert!(changed);
        assert_eq!(at(&document, 0, 0), BLUE, "种子被填");
        assert_eq!(at(&document, 5, 10), BLUE, "同侧连通区域被填");
        assert_eq!(at(&document, 6, 10), Color::BLACK, "边界线不动");
        assert_eq!(at(&document, 7, 10), Color::WHITE, "另一侧不填");
    }

    #[test]
    fn a_global_fill_ignores_connectivity() {
        let mut document = blank();
        paint(&mut document, 0, 0, Color::RED);
        paint(&mut document, 12, 12, Color::RED);
        let layer = document.active_layer_mut().unwrap();
        flood_fill(
            &mut layer.pixels,
            Point::new(0, 0),
            (16, 16),
            (0, 0),
            BLUE,
            0,
            false,
            None,
        );
        assert_eq!(at(&document, 0, 0), BLUE);
        assert_eq!(at(&document, 12, 12), BLUE, "不连通的同色也填");
    }

    #[test]
    fn the_tolerance_widens_the_match() {
        // 容差 0：只填种子本身。
        let mut document = blank();
        paint(&mut document, 0, 0, Color::rgb(100, 100, 100));
        paint(&mut document, 1, 0, Color::rgb(110, 100, 100));
        let layer = document.active_layer_mut().unwrap();
        flood_fill(
            &mut layer.pixels,
            Point::new(0, 0),
            (16, 16),
            (0, 0),
            BLUE,
            0,
            true,
            None,
        );
        assert_eq!(
            at(&document, 1, 0),
            Color::rgb(110, 100, 100),
            "容差 0 不匹配"
        );

        // 容差 16：新文档（种子仍是 100 灰），把相邻的 110 也带上。
        let mut document = blank();
        paint(&mut document, 0, 0, Color::rgb(100, 100, 100));
        paint(&mut document, 1, 0, Color::rgb(110, 100, 100));
        let layer = document.active_layer_mut().unwrap();
        flood_fill(
            &mut layer.pixels,
            Point::new(0, 0),
            (16, 16),
            (0, 0),
            BLUE,
            16,
            true,
            None,
        );
        assert_eq!(at(&document, 0, 0), BLUE);
        assert_eq!(at(&document, 1, 0), BLUE, "容差 16 匹配");
    }

    #[test]
    fn the_selection_confines_the_fill() {
        let mut document = blank();
        // 整张红，方便看出选区边界拦住了填充。
        for y in 0..16 {
            for x in 0..16 {
                paint(&mut document, x, y, Color::RED);
            }
        }
        let clip = Selection::rect(PixelRegion::new(0, 0, 4, 4), (16, 16));
        let layer = document.active_layer_mut().unwrap();
        flood_fill(
            &mut layer.pixels,
            Point::new(0, 0),
            (16, 16),
            (0, 0),
            BLUE,
            0,
            true,
            Some(&clip),
        );
        assert_eq!(at(&document, 3, 3), BLUE, "选区内被填");
        assert_eq!(at(&document, 4, 4), Color::RED, "选区外保持原色");
    }

    #[test]
    fn filling_with_the_same_color_is_a_no_op() {
        let mut document = blank();
        for y in 0..16 {
            for x in 0..16 {
                paint(&mut document, x, y, Color::RED);
            }
        }
        let layer = document.active_layer_mut().unwrap();
        assert!(!flood_fill(
            &mut layer.pixels,
            Point::new(0, 0),
            (16, 16),
            (0, 0),
            Color::RED,
            0,
            true,
            None,
        ));
    }

    #[test]
    fn the_bucket_commits_one_undo_step() {
        let mut document = blank();
        for y in 0..16 {
            for x in 0..16 {
                paint(&mut document, x, y, Color::RED);
            }
        }
        let mut history = History::new();
        let mut bucket = PaintBucketTool::new();
        bucket.color = BLUE;
        let mut ctx = ToolContext {
            document: &mut document,
            history: &mut history,
        };
        bucket.on_pointer_down(
            &mut ctx,
            PointerEvent {
                position: igui_core::Vec2::new(0.0, 0.0),
                button: igui_core::PointerButton::Left,
            },
        );
        assert_eq!(history.undo_len(), 1, "一次填充 = 一步 undo");
        history.undo(&mut document);
        assert_eq!(at(&document, 0, 0), Color::RED, "撤销还原");
    }
}
