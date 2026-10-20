//! CPU 合成器：`Normal` 混合 + 图层不透明度 + 图层位置偏移。

use crate::document::{blend_over, Color, Document, PixelBuffer, PixelRegion};

use super::{RenderTarget, Renderer};

/// 第一版合成器。纯 CPU，无依赖，可无头测试。
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuRenderer;

impl Renderer for CpuRenderer {
    fn render(&self, document: &Document, target: &mut RenderTarget) {
        self.render_region(
            document,
            target,
            PixelRegion::new(0, 0, document.width, document.height),
        );
    }

    /// 只重算 `region`：画笔一笔只脏几百像素时，4K 文档也不用每帧扫 830 万像素。
    ///
    /// 目标尺寸变了（换文档 / 缩放画布尺寸）就直接整图重来，否则局部结果是错的。
    fn render_region(&self, document: &Document, target: &mut RenderTarget, region: PixelRegion) {
        if target.pixels.width != document.width || target.pixels.height != document.height {
            target.resize(document.width, document.height);
            return self.render(document, target);
        }
        let region = region.clamped(document.width, document.height);
        if region.area() == 0 {
            return;
        }
        target.resize(document.width, document.height);
        // 画布底是透明的；白色背景由文档里的“背景”图层提供，这样隐藏背景
        // 图层时能露出透明的棋盘格背景（`canvas::checkerboard`，只在显示用的
        // 合成结果上生成，不影响导出的 PNG）。
        clear_region(&mut target.pixels, region);

        // layers 从下到上，直接顺序 source-over 即可。
        for layer in &document.layers {
            if !layer.visible || layer.opacity <= 0.0 {
                continue;
            }
            blend_layer(&mut target.pixels, layer, region);
        }
    }
}

/// 逐 4 字节块合成（同尺寸对齐的快路）：透明跳过、不透明覆盖、其余 source-over。
fn blend_rows(dst: &mut [u8], src: &[u8], opacity: f32, opaque: bool) {
    for (dst, src) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let alpha = src[3];
        if alpha == 0 {
            continue;
        }
        if opaque && alpha == 255 {
            dst.copy_from_slice(src);
            continue;
        }
        blend_over(dst, src, opacity);
    }
}

/// 把 `region` 内的目标像素清成 `color`（区域外不动）。
fn clear_region(target: &mut PixelBuffer, region: PixelRegion) {
    for y in region.y..region.bottom() {
        let start = (y as usize * target.width as usize + region.x as usize) * 4;
        let end = start + region.width as usize * 4;
        for pixel in target.data[start..end].chunks_exact_mut(4) {
            pixel.copy_from_slice(&Color::TRANSPARENT.to_rgba8());
        }
    }
}

/// 把一个图层按位置偏移与不透明度合成到目标上（只处理 `region` 内的目标像素）。
fn blend_layer(target: &mut PixelBuffer, layer: &crate::document::Layer, region: PixelRegion) {
    let opacity = layer.opacity;
    // 不透明度拉满时，alpha=255 的源像素可以直接覆盖目标（source-over 的
    // 特例），省掉每像素的浮点混合。背景层与画笔实心部分都走这条路。
    let opaque = opacity >= 1.0;
    let dx = layer.position.x as i64;
    let dy = layer.position.y as i64;

    // 「图层映射到目标后的范围」∩ 脏矩形：先在**目标坐标**里求交集，再换成图层索引，
    // 否则偏移过的图层会在错误的像素上合成。
    let tx_start = dx.max(0).max(region.x as i64);
    let tx_end = (dx + layer.pixels.width as i64)
        .min(target.width as i64)
        .min(region.right() as i64);
    let ty_start = dy.max(0).max(region.y as i64);
    let ty_end = (dy + layer.pixels.height as i64)
        .min(target.height as i64)
        .min(region.bottom() as i64);
    if tx_start >= tx_end || ty_start >= ty_end {
        return;
    }
    let x_start = tx_start - dx;
    let x_end = tx_end - dx;
    let y_start = ty_start - dy;
    let y_end = ty_end - dy;

    let aligned = dx == 0
        && dy == 0
        && layer.pixels.width == target.width
        && layer.pixels.height == target.height;
    if aligned {
        // 快路：同尺寸对齐，按行切片逐 4 字节块处理，省掉每像素的坐标与边界检查。
        let (x0, x1) = (x_start as usize, x_end as usize);
        let row = target.width as usize;
        for y in y_start as usize..y_end as usize {
            let start = (y * row + x0) * 4;
            let end = (y * row + x1) * 4;
            blend_rows(
                &mut target.data[start..end],
                &layer.pixels.data[start..end],
                opacity,
                opaque,
            );
        }
        return;
    }

    for y in y_start as u32..y_end as u32 {
        for x in x_start as u32..x_end as u32 {
            let src = layer.pixels.get_pixel(x, y);
            if src.is_transparent() {
                continue;
            }
            let dest_x = (x as i64 + dx) as u32;
            let dest_y = (y as i64 + dy) as u32;
            if opaque && src.a == 255 {
                target.set_pixel(dest_x, dest_y, src);
            } else {
                target.blend_pixel(dest_x, dest_y, src, opacity);
            }
        }
    }
}

/// 合成**单个**像素：按 [`CpuRenderer`] 的同一套规则（可见性、不透明度、
/// 图层偏移、source-over）把图层叠一遍，只算 `(x, y)`。吸管取色用。
///
/// 不分配整张目标图，所以比全量合成便宜得多。
pub fn sample_pixel(document: &Document, x: u32, y: u32) -> Color {
    let mut out = [0u8, 0, 0, 0];
    for layer in &document.layers {
        if !layer.visible || layer.opacity <= 0.0 {
            continue;
        }
        let source_x = x as i64 - layer.position.x as i64;
        let source_y = y as i64 - layer.position.y as i64;
        if source_x < 0 || source_y < 0 {
            continue;
        }
        let (source_x, source_y) = (source_x as u32, source_y as u32);
        if !layer.pixels.contains(source_x, source_y) {
            continue;
        }
        let source = layer.pixels.get_pixel(source_x, source_y);
        if source.is_transparent() {
            continue;
        }
        blend_over(&mut out, &source.to_rgba8(), layer.opacity);
    }
    Color::from_rgba8(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Layer, PixelBuffer, Point};

    fn render(document: &Document) -> PixelBuffer {
        let mut target = RenderTarget::new(0, 0);
        CpuRenderer.render(document, &mut target);
        target.pixels
    }

    #[test]
    fn a_single_opaque_layer_is_copied_through() {
        let mut document = Document::empty("d", 2, 1, Color::BLACK);
        let layer = Layer::new("L", PixelBuffer::filled(2, 1, Color::RED));
        document.layers.push(layer);
        let pixels = render(&document);
        assert_eq!(pixels.get_pixel(0, 0), Color::RED);
        assert_eq!(pixels.get_pixel(1, 0), Color::RED);
    }

    #[test]
    fn hidden_layers_are_skipped() {
        let mut document = Document::empty("d", 1, 1, Color::BLACK);
        let mut layer = Layer::new("L", PixelBuffer::filled(1, 1, Color::RED));
        layer.visible = false;
        document.layers.push(layer);
        assert_eq!(render(&document).get_pixel(0, 0), Color::TRANSPARENT);
    }

    #[test]
    fn a_partial_render_matches_the_full_one_inside_the_region() {
        // 局部合成必须和整图逐像素一致（这是脏矩形正确性的唯一标准）。
        let mut document = Document::with_background("d", 16, 16, Color::WHITE);
        let id = document.active_layer().unwrap().id;
        document.set_layer_position(id, crate::document::Point::new(3, 2));
        let layer = document.layer_mut(id).unwrap();
        for y in 2..8 {
            for x in 3..9 {
                layer.pixels.set_pixel(x, y, Color::RED);
            }
        }

        let renderer = CpuRenderer;
        let mut full = RenderTarget::new(16, 16);
        renderer.render(&document, &mut full);

        let mut partial = RenderTarget::new(16, 16);
        // 先整图一次（得到「上一帧」的底），再局部重算一条带子。
        renderer.render(&document, &mut partial);
        let region = PixelRegion::new(4, 2, 6, 4);
        partial.pixels.set_pixel(0, 0, Color::BLACK); // 区域外故意涂脏
        renderer.render_region(&document, &mut partial, region);

        assert_eq!(
            partial.pixels.get_pixel(0, 0),
            Color::BLACK,
            "区域外不该被重算"
        );
        for y in region.y..region.bottom() {
            for x in region.x..region.right() {
                assert_eq!(
                    partial.pixels.get_pixel(x, y),
                    full.pixels.get_pixel(x, y),
                    "({x}, {y}) 应和整图一致"
                );
            }
        }
    }

    #[test]
    fn the_top_layer_wins() {
        let mut document = Document::empty("d", 1, 1, Color::BLACK);
        document
            .layers
            .push(Layer::new("bottom", PixelBuffer::filled(1, 1, Color::RED)));
        document
            .layers
            .push(Layer::new("top", PixelBuffer::filled(1, 1, Color::WHITE)));
        assert_eq!(render(&document).get_pixel(0, 0), Color::WHITE);
    }

    #[test]
    fn opacity_blends_towards_the_background() {
        let mut document = Document::empty("d", 1, 1, Color::BLACK);
        document
            .layers
            .push(Layer::new("bg", PixelBuffer::filled(1, 1, Color::WHITE)));
        let mut top = Layer::new("top", PixelBuffer::filled(1, 1, Color::RED));
        top.opacity = 0.5;
        document.layers.push(top);

        let color = render(&document).get_pixel(0, 0);
        assert_eq!(color.a, 255);
        assert_eq!(color.r, 255);
        assert!((color.g as i32 - 128).abs() <= 1, "g = {}", color.g);
        assert!((color.b as i32 - 128).abs() <= 1, "b = {}", color.b);
    }

    #[test]
    fn layer_position_offsets_the_pixels() {
        let mut document = Document::empty("d", 3, 1, Color::BLACK);
        let mut layer = Layer::new("L", PixelBuffer::filled(1, 1, Color::RED));
        layer.position = Point::new(2, 0);
        document.layers.push(layer);

        let pixels = render(&document);
        assert_eq!(pixels.get_pixel(0, 0), Color::TRANSPARENT);
        assert_eq!(pixels.get_pixel(1, 0), Color::TRANSPARENT);
        assert_eq!(pixels.get_pixel(2, 0), Color::RED);
    }

    #[test]
    #[ignore = "perf probe: --ignored --nocapture"]
    fn probe_composite_and_clone_cost() {
        use std::time::Instant;
        let mut document = Document::new("d", 800, 600);
        document.add_layer("L1");
        document.add_layer("L2");
        let mut target = RenderTarget::new(800, 600);
        CpuRenderer.render(&document, &mut target);
        let n = 100;
        let timer = Instant::now();
        for _ in 0..n {
            CpuRenderer.render(&document, &mut target);
        }
        let render_ms = timer.elapsed().as_secs_f64() / n as f64 * 1000.0;
        let timer = Instant::now();
        for _ in 0..n {
            std::hint::black_box(target.pixels.clone());
        }
        let clone_ms = timer.elapsed().as_secs_f64() / n as f64 * 1000.0;

        let one = Document::new("d", 800, 600);
        let mut target1 = RenderTarget::new(800, 600);
        CpuRenderer.render(&one, &mut target1);
        let timer = Instant::now();
        for _ in 0..n {
            CpuRenderer.render(&one, &mut target1);
        }
        let one_ms = timer.elapsed().as_secs_f64() / n as f64 * 1000.0;

        eprintln!(
            "render 3 layers {render_ms:.2} ms, 1 layer {one_ms:.2} ms, clone {clone_ms:.2} ms"
        );
    }

    #[test]
    fn negative_positions_clip_at_the_canvas_edge() {
        let mut document = Document::empty("d", 2, 1, Color::BLACK);
        let mut layer = Layer::new("L", PixelBuffer::filled(2, 1, Color::RED));
        layer.position = Point::new(-1, 0);
        document.layers.push(layer);

        let pixels = render(&document);
        assert_eq!(pixels.get_pixel(0, 0), Color::RED, "右半部分落到 x=0");
        assert_eq!(pixels.get_pixel(1, 0), Color::TRANSPARENT);
    }

    #[test]
    fn a_layer_larger_than_the_document_composites_only_the_visible_part() {
        // 3×3 图层超出 2×2 文档，放在 (-1,-1)：超出部分被裁掉，可见部分正常。
        let mut document = Document::empty("d", 2, 2, Color::BLACK);
        let mut pixels = PixelBuffer::filled(3, 3, Color::rgb(0, 0, 255));
        pixels.set_pixel(1, 1, Color::RED);
        let mut layer = Layer::new("L", pixels);
        layer.position = Point::new(-1, -1);
        document.layers.push(layer);

        let rendered = render(&document);
        assert_eq!(
            rendered.get_pixel(0, 0),
            Color::RED,
            "图层(1,1) -> 文档(0,0)"
        );
        assert_eq!(rendered.get_pixel(1, 1), Color::rgb(0, 0, 255));
    }

    #[test]
    fn sample_pixel_matches_the_full_composite() {
        let mut document = Document::empty("d", 3, 2, Color::BLACK);
        document
            .layers
            .push(Layer::new("bg", PixelBuffer::filled(3, 2, Color::WHITE)));
        let mut top = Layer::new("top", PixelBuffer::filled(1, 1, Color::RED));
        top.position = Point::new(1, 1);
        document.layers.push(top);

        let full = render(&document);
        for y in 0..2 {
            for x in 0..3 {
                assert_eq!(
                    sample_pixel(&document, x, y),
                    full.get_pixel(x, y),
                    "({x}, {y}) 应与全量合成一致"
                );
            }
        }
    }

    #[test]
    fn sample_pixel_respects_visibility_and_position() {
        let mut document = Document::empty("d", 2, 1, Color::BLACK);
        let mut hidden = Layer::new("hidden", PixelBuffer::filled(2, 1, Color::RED));
        hidden.visible = false;
        document.layers.push(hidden);
        // 全透明 -> 保持透明（不会误报成背景色）。
        assert_eq!(sample_pixel(&document, 0, 0), Color::TRANSPARENT);

        // 偏移图层：只有落到画布内才被采样。
        let mut layer = Layer::new("L", PixelBuffer::filled(1, 1, Color::RED));
        layer.position = Point::new(1, 0);
        document.layers.push(layer);
        assert_eq!(sample_pixel(&document, 0, 0), Color::TRANSPARENT);
        assert_eq!(sample_pixel(&document, 1, 0), Color::RED);
    }
}
