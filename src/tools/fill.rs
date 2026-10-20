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
    let Some(mask) = region(buffer, offset, document, seed, tolerance, contiguous, clip) else {
        return false;
    };
    // 种子本来就是填充色 -> 没有变化。
    if document_pixel(buffer, offset, seed.0, seed.1) == color {
        return false;
    }
    let (width, height) = document;
    let mut changed = false;
    for y in 0..height {
        for x in 0..width {
            if !mask[y as usize * width as usize + x as usize] {
                continue;
            }
            let bx = x as i64 - offset.x as i64;
            let by = y as i64 - offset.y as i64;
            if bx < 0 || by < 0 || bx >= buffer.width as i64 || by >= buffer.height as i64 {
                continue;
            }
            buffer.set_pixel(bx as u32, by as u32, color);
            changed = true;
        }
    }
    changed
}

/// 魔棒：从文档坐标 `seed` 选出一片容差区域，返回**文档坐标**的掩码。
///
/// 和 [`flood_fill`] 共用 [`region`]，但不落笔 —— 交给
/// [`Selection::from_mask`](crate::canvas::Selection::from_mask) 变成选区。
/// `None` = 种子不在文档里。
pub fn magic_wand_region(
    buffer: &PixelBuffer,
    offset: Point,
    document: (u32, u32),
    seed: (u32, u32),
    tolerance: u8,
    contiguous: bool,
) -> Option<Vec<bool>> {
    region(buffer, offset, document, seed, tolerance, contiguous, None)
}

/// 从文档坐标 `seed` 算出容差区域（**文档坐标**的掩码）。
///
/// 落笔的 [`flood_fill`] 和魔棒共用它。`None` = 种子不在文档 / 不可选。
/// 图层没覆盖到的文档像素当透明（[`document_pixel`]）。
fn region(
    buffer: &PixelBuffer,
    offset: Point,
    document: (u32, u32),
    seed: (u32, u32),
    tolerance: u8,
    contiguous: bool,
    clip: Option<&Selection>,
) -> Option<Vec<bool>> {
    let (width, height) = document;
    if seed.0 >= width || seed.1 >= height {
        return None;
    }
    let selectable = |x: u32, y: u32| clip.map_or(true, |selection| selection.contains(x, y));
    if !selectable(seed.0, seed.1) {
        return None;
    }
    let target = document_pixel(buffer, offset, seed.0, seed.1);
    let mut mask = vec![false; width as usize * height as usize];

    if !contiguous {
        for y in 0..height {
            for x in 0..width {
                let pixel = document_pixel(buffer, offset, x, y);
                if selectable(x, y) && within_tolerance(pixel, target, tolerance) {
                    mask[y as usize * width as usize + x as usize] = true;
                }
            }
        }
        return Some(mask);
    }

    // 连续：栈 + `mask` 兼作 `visited`，四连通。
    let mut stack = vec![seed];
    while let Some((x, y)) = stack.pop() {
        let index = y as usize * width as usize + x as usize;
        if mask[index] || !selectable(x, y) {
            continue;
        }
        if !within_tolerance(document_pixel(buffer, offset, x, y), target, tolerance) {
            continue;
        }
        mask[index] = true;
        if x > 0 {
            stack.push((x - 1, y));
        }
        if x + 1 < width {
            stack.push((x + 1, y));
        }
        if y > 0 {
            stack.push((x, y - 1));
        }
        if y + 1 < height {
            stack.push((x, y + 1));
        }
    }
    Some(mask)
}

/// 读文档坐标的像素；图层没覆盖到的地方当透明。
fn document_pixel(buffer: &PixelBuffer, offset: Point, x: u32, y: u32) -> Color {
    let bx = x as i64 - offset.x as i64;
    let by = y as i64 - offset.y as i64;
    if bx < 0 || by < 0 || bx >= buffer.width as i64 || by >= buffer.height as i64 {
        return Color::TRANSPARENT;
    }
    buffer.get_pixel(bx as u32, by as u32)
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
                // 羽化过的选区按覆盖率混合，硬选区里就是 0 / 1。
                let mut alpha = region.coverage(x, y);
                if let Some(clip) = clip {
                    alpha *= clip.coverage(x, y);
                }
                if alpha <= 0.0 {
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
                let before = layer.pixels.get_pixel(bx, by);
                if alpha >= 1.0 {
                    if before != color {
                        layer.pixels.set_pixel(bx, by, color);
                        changed = true;
                    }
                } else {
                    layer.pixels.blend_from(bx, by, before, color, alpha);
                    if layer.pixels.get_pixel(bx, by) != before {
                        changed = true;
                    }
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

    #[test]
    fn a_magic_wand_selects_the_connected_region_and_stops_at_the_border() {
        let mut document = blank();
        for y in 0..16 {
            for x in 0..6 {
                paint(&mut document, x, y, Color::RED);
            }
            paint(&mut document, 6, y, Color::BLACK);
        }
        let layer = document.active_layer().unwrap();
        let mask = magic_wand_region(&layer.pixels, Point::new(0, 0), (16, 16), (0, 0), 0, true)
            .expect("种子在图层里");
        assert!(mask[0], "种子被选");
        assert!(mask[5 * 16 + 5], "同侧连通区域被选");
        assert!(!mask[6], "边界线不选");
        assert!(!mask[7], "另一侧不选");
        assert_eq!(mask.iter().filter(|selected| **selected).count(), 6 * 16);
    }

    /// 容差放宽后，颜色略有差异的像素也会被选进来。
    #[test]
    fn a_magic_wand_tolerance_widens_the_region() {
        let mut document = blank();
        paint(&mut document, 0, 0, Color::RED);
        paint(&mut document, 1, 0, Color::rgb(250, 0, 0));
        let layer = document.active_layer().unwrap();
        let strict =
            magic_wand_region(&layer.pixels, Point::new(0, 0), (16, 16), (0, 0), 0, true).unwrap();
        assert!(!strict[1], "容差 0：邻近色不选");
        let loose =
            magic_wand_region(&layer.pixels, Point::new(0, 0), (16, 16), (0, 0), 5, true).unwrap();
        assert!(loose[1], "容差 5：邻近色选进来");
    }

    /// 种子落在文档外：没有区域（调用方据此不动选区）。
    #[test]
    fn a_magic_wand_seed_outside_the_document_has_no_region() {
        let document = blank();
        let layer = document.active_layer().unwrap();
        assert!(
            magic_wand_region(&layer.pixels, Point::new(0, 0), (16, 16), (99, 99), 0, true)
                .is_none()
        );
    }
}
