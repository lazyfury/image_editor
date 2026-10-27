//! CPU 合成器：`Normal` 混合 + 图层不透明度 + 图层位置偏移 + 隔离组 + 蒙版。
//!
//! 图层栈是前序 DFS 的扁平数组（见 [`Document`](crate::document::Document)），
//! 组先把自己的子树合成到一块**区域大小**的临时缓冲，再按组的不透明度 / 蒙版
//! 整体压回父级（隔离组）。所以局部合成（脏矩形）仍然成立：临时缓冲只有
//! `region` 那么大。

use crate::document::{blend_over, Color, Document, Layer, LayerMask, PixelBuffer, PixelRegion};

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

        let tree = Tree::new(document);
        tree.composite(document, &tree.roots, &mut target.pixels, (0, 0), region);
    }
}

/// 一次合成用的父子索引（每帧建一次，O(n)）。
struct Tree {
    children: Vec<Vec<usize>>,
    roots: Vec<usize>,
}

impl Tree {
    fn new(document: &Document) -> Self {
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); document.layers.len()];
        let mut roots = Vec::new();
        for (index, layer) in document.layers.iter().enumerate() {
            match layer.parent.and_then(|parent| document.layer_index(parent)) {
                Some(parent) => children[parent].push(index),
                None => roots.push(index),
            }
        }
        Self { children, roots }
    }

    /// 从下到上合成一组兄弟节点到 `target`；`origin` 是目标第 (0,0) 像素对应的
    /// 文档坐标（根目标是 (0,0)，组临时缓冲是区域左上角）。
    fn composite(
        &self,
        document: &Document,
        nodes: &[usize],
        target: &mut PixelBuffer,
        origin: (i64, i64),
        region: PixelRegion,
    ) {
        for &index in nodes {
            let layer = &document.layers[index];
            if !layer.visible || layer.opacity <= 0.0 {
                continue;
            }
            if layer.is_group() {
                let mut scratch = PixelBuffer::new(region.width, region.height);
                self.composite(
                    document,
                    &self.children[index],
                    &mut scratch,
                    (region.x as i64, region.y as i64),
                    region,
                );
                blend_buffer(
                    target,
                    origin,
                    &scratch,
                    region,
                    layer.opacity,
                    layer.mask.as_ref(),
                );
            } else {
                blend_layer(target, origin, layer, region);
            }
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

/// 把 `region` 内的目标像素清成透明（区域外不动）。
fn clear_region(target: &mut PixelBuffer, region: PixelRegion) {
    for y in region.y..region.bottom() {
        let start = (y as usize * target.width as usize + region.x as usize) * 4;
        let end = start + region.width as usize * 4;
        for pixel in target.data[start..end].chunks_exact_mut(4) {
            pixel.copy_from_slice(&Color::TRANSPARENT.to_rgba8());
        }
    }
}

/// 把一个栅格层按位置偏移、不透明度与蒙版合成到目标上。
///
/// 目标坐标 `(0,0)` 对应文档 `origin`；`region` 是文档坐标。
fn blend_layer(target: &mut PixelBuffer, origin: (i64, i64), layer: &Layer, region: PixelRegion) {
    let opacity = layer.opacity;
    let mask = layer.mask.as_ref().filter(|mask| mask.enabled);
    let dx = layer.position.x as i64;
    let dy = layer.position.y as i64;

    // 「图层映射到目标后的范围」∩ 脏矩形：先在**目标坐标**里求交集，再换成图层索引，
    // 否则偏移过的图层会在错误的像素上合成。
    let tx_start = dx.max(region.x as i64).max(origin.0);
    let tx_end = (dx + layer.pixels.width as i64)
        .min(region.right() as i64)
        .min(target.width as i64 + origin.0);
    let ty_start = dy.max(region.y as i64).max(origin.1);
    let ty_end = (dy + layer.pixels.height as i64)
        .min(region.bottom() as i64)
        .min(target.height as i64 + origin.1);
    if tx_start >= tx_end || ty_start >= ty_end {
        return;
    }
    let x_start = tx_start - dx;
    let x_end = tx_end - dx;
    let y_start = ty_start - dy;
    let y_end = ty_end - dy;

    // 快路：同尺寸对齐 + 目标就是文档坐标时，按行切片逐 4 字节块处理。
    let aligned = mask.is_none()
        && origin == (0, 0)
        && dx == 0
        && dy == 0
        && layer.pixels.width == target.width
        && layer.pixels.height == target.height;
    if aligned {
        let opaque = opacity >= 1.0;
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
            let source = layer.pixels.get_pixel(x, y);
            if source.is_transparent() {
                continue;
            }
            let coverage = match mask {
                Some(mask) => mask.coverage(x, y),
                None => 1.0,
            };
            if coverage <= 0.0 {
                continue;
            }
            let dest_x = (x as i64 + dx - origin.0) as u32;
            let dest_y = (y as i64 + dy - origin.1) as u32;
            if coverage >= 1.0 && opacity >= 1.0 && source.a == 255 {
                target.set_pixel(dest_x, dest_y, source);
            } else {
                target.blend_pixel(dest_x, dest_y, source, opacity * coverage);
            }
        }
    }
}

/// 把组临时缓冲（区域大小）合成到父目标上，按组的不透明度与蒙版（文档坐标）。
fn blend_buffer(
    target: &mut PixelBuffer,
    origin: (i64, i64),
    buffer: &PixelBuffer,
    region: PixelRegion,
    opacity: f32,
    mask: Option<&LayerMask>,
) {
    let mask = mask.filter(|mask| mask.enabled);
    let opaque = opacity >= 1.0 && mask.is_none();
    for y in 0..region.height {
        for x in 0..region.width {
            let source = buffer.get_pixel(x, y);
            if source.is_transparent() {
                continue;
            }
            let doc_x = region.x + x;
            let doc_y = region.y + y;
            let coverage = match mask {
                Some(mask) => mask.coverage(doc_x, doc_y),
                None => 1.0,
            };
            if coverage <= 0.0 {
                continue;
            }
            let dest_x = (doc_x as i64 - origin.0) as u32;
            let dest_y = (doc_y as i64 - origin.1) as u32;
            if opaque && coverage >= 1.0 && source.a == 255 {
                target.set_pixel(dest_x, dest_y, source);
            } else {
                target.blend_pixel(dest_x, dest_y, source, opacity * coverage);
            }
        }
    }
}

/// 合成**单个**像素：按 [`CpuRenderer`] 的同一套规则（可见性、不透明度、
/// 图层偏移、蒙版、隔离组、source-over）把文档叠一遍，只算 `(x, y)`。
/// 吸管取色用。
///
/// 不分配整张目标图，所以比全量合成便宜得多。
pub fn sample_pixel(document: &Document, x: u32, y: u32) -> Color {
    let tree = Tree::new(document);
    let mut out = [0u8, 0, 0, 0];
    sample_nodes(document, &tree, &tree.roots, x, y, &mut out);
    Color::from_rgba8(out)
}

fn sample_nodes(
    document: &Document,
    tree: &Tree,
    nodes: &[usize],
    x: u32,
    y: u32,
    out: &mut [u8; 4],
) {
    for &index in nodes {
        let layer = &document.layers[index];
        if !layer.visible || layer.opacity <= 0.0 {
            continue;
        }
        if layer.is_group() {
            let mut inner = [0u8, 0, 0, 0];
            sample_nodes(document, tree, &tree.children[index], x, y, &mut inner);
            let coverage = match layer.mask.as_ref().filter(|mask| mask.enabled) {
                Some(mask) => mask.coverage(x, y),
                None => 1.0,
            };
            let alpha = layer.opacity * coverage;
            if inner[3] == 0 || alpha <= 0.0 {
                continue;
            }
            blend_over(out, &inner, alpha);
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
        let coverage = layer.mask_coverage(source_x, source_y);
        blend_over(out, &source.to_rgba8(), layer.opacity * coverage);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Layer, LayerMask, Point};

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
            "区域外保持上一帧"
        );
        for y in region.y..region.bottom() {
            for x in region.x..region.right() {
                assert_eq!(
                    partial.pixels.get_pixel(x, y),
                    full.pixels.get_pixel(x, y),
                    "({x}, {y}) 局部与整图不一致"
                );
            }
        }
    }

    // -- 组 / 蒙版 --------------------------------------------------------

    /// 背景（黑） + 组[ 红, 蓝 ]，组的不透明度 `opacity`。
    fn group_fixture(opacity: f32) -> Document {
        let mut document = Document::with_background("d", 2, 2, Color::BLACK);
        let group = document.add_group("组");
        document.set_layer_opacity(group, opacity);
        document.add_layer_with_pixels("红", PixelBuffer::filled(2, 2, Color::RED));
        document.add_layer_with_pixels("蓝", PixelBuffer::filled(2, 2, Color::new(0, 0, 255, 255)));
        document
    }

    #[test]
    fn group_opacity_applies_once_to_the_flattened_group() {
        // 蓝盖住红，再整体 50% 压到黑底上 —— 逐层乘 50% 会得到红+蓝的混色。
        let document = group_fixture(0.5);
        let pixel = render(&document).get_pixel(0, 0);
        assert!(pixel.r <= 2, "红应该被组内的蓝完全盖住：{pixel:?}");
        assert!(
            (i32::from(pixel.b) - 128).abs() <= 2,
            "组不透明度只作用一次：{pixel:?}"
        );
        assert!(pixel.a >= 254);
    }

    #[test]
    fn hiding_a_group_hides_its_whole_subtree() {
        let mut document = group_fixture(1.0);
        let group = document.layers[1].id;
        assert!(document.set_layer_visible(group, false));
        let pixel = render(&document).get_pixel(0, 0);
        assert_eq!(pixel, Color::BLACK, "组藏起来后只剩背景");
    }

    #[test]
    fn a_layer_mask_scales_its_alpha() {
        let mut document = Document::empty("d", 2, 1, Color::BLACK);
        let mut layer = Layer::new("L", PixelBuffer::filled(2, 1, Color::RED));
        let mut mask = LayerMask::opaque(2, 1);
        mask.pixels.set_pixel(0, 0, Color::BLACK);
        layer.mask = Some(mask);
        document.layers.push(layer);

        let pixels = render(&document);
        assert_eq!(pixels.get_pixel(0, 0), Color::TRANSPARENT, "遮住的像素不画");
        assert_eq!(pixels.get_pixel(1, 0), Color::RED);
    }

    #[test]
    fn a_group_mask_scales_the_flattened_group() {
        let mut document = group_fixture(1.0);
        let group = document.layers[1].id;
        let mut mask = LayerMask::opaque(2, 2);
        mask.pixels.set_pixel(1, 1, Color::BLACK);
        assert!(document.set_layer_mask(group, mask));
        let pixels = render(&document);
        assert_eq!(
            pixels.get_pixel(1, 1),
            Color::BLACK,
            "组蒙版遮住的像素露背景"
        );
        assert_eq!(pixels.get_pixel(0, 0), Color::new(0, 0, 255, 255));
    }

    #[test]
    fn a_disabled_mask_is_ignored() {
        let mut document = Document::empty("d", 1, 1, Color::BLACK);
        let mut layer = Layer::new("L", PixelBuffer::filled(1, 1, Color::RED));
        let mut mask = LayerMask::opaque(1, 1);
        mask.pixels.set_pixel(0, 0, Color::BLACK);
        mask.enabled = false;
        layer.mask = Some(mask);
        document.layers.push(layer);
        assert_eq!(render(&document).get_pixel(0, 0), Color::RED);
    }

    #[test]
    fn a_partial_render_with_groups_and_masks_matches_the_full_one() {
        let mut document = group_fixture(0.4);
        let group = document.layers[1].id;
        let mut mask = LayerMask::opaque(2, 2);
        mask.pixels.set_pixel(0, 1, Color::new(90, 90, 90, 255));
        assert!(document.set_layer_mask(group, mask));
        let nested = document.add_group("组内组");
        document.move_into(nested, group);
        document.add_layer_with_pixels("绿", PixelBuffer::filled(2, 2, Color::new(0, 255, 0, 255)));

        let mut full = RenderTarget::new(2, 2);
        CpuRenderer.render(&document, &mut full);
        for region in [
            PixelRegion::new(0, 0, 1, 1),
            PixelRegion::new(1, 0, 1, 2),
            PixelRegion::new(0, 1, 2, 1),
        ] {
            let mut partial = RenderTarget::new(2, 2);
            CpuRenderer.render(&document, &mut partial);
            CpuRenderer.render_region(&document, &mut partial, region);
            for y in region.y..region.bottom() {
                for x in region.x..region.right() {
                    assert_eq!(
                        partial.pixels.get_pixel(x, y),
                        full.pixels.get_pixel(x, y),
                        "{region:?} 的 ({x}, {y}) 与整图不一致"
                    );
                }
            }
        }
    }

    #[test]
    fn the_eyedropper_agrees_with_the_composite_through_groups_and_masks() {
        let mut document = group_fixture(0.5);
        let group = document.layers[1].id;
        let mut mask = LayerMask::opaque(2, 2);
        mask.pixels.set_pixel(0, 0, Color::BLACK);
        assert!(document.set_layer_mask(group, mask));
        let pixels = render(&document);
        for y in 0..2 {
            for x in 0..2 {
                assert_eq!(sample_pixel(&document, x, y), pixels.get_pixel(x, y));
            }
        }
    }

    #[test]
    fn a_group_with_a_bigger_child_is_clipped_by_the_region() {
        // 子层带偏移、比文档大：局部合成仍然只写区域内的像素。
        let mut document = Document::with_background("d", 4, 4, Color::WHITE);
        let group = document.add_group("组");
        let mut child = Layer::new("大", PixelBuffer::filled(8, 8, Color::RED));
        child.position = Point::new(-2, -2);
        assert!(document.insert_subtree(vec![child], Some(group), 0));

        let mut full = RenderTarget::new(4, 4);
        CpuRenderer.render(&document, &mut full);
        let region = PixelRegion::new(1, 1, 2, 2);
        let mut partial = RenderTarget::new(4, 4);
        CpuRenderer.render(&document, &mut partial);
        CpuRenderer.render_region(&document, &mut partial, region);
        for y in 0..4 {
            for x in 0..4 {
                if region.x <= x && x < region.right() && region.y <= y && y < region.bottom() {
                    assert_eq!(partial.pixels.get_pixel(x, y), full.pixels.get_pixel(x, y));
                }
            }
        }
    }
}
