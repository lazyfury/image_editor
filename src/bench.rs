//! 图层 / 图层树 UI 的性能基准（`--bench`）。
//!
//! 只做**程序化测量**：`std::time::Instant` + 多次取中位数，不看画面、不开窗口。
//! 建议用 release 跑（debug 构建里 `Document::insert_subtree` 的 `debug_assert`
//! 会每次校验整棵树，构造文档本身会被拖慢）：
//!
//! ```bash
//! cargo run --release -- --bench
//! ```
//!
//! 覆盖四块：
//!
//! 1. **合成**：平铺层 / 层蒙版 / 隔离组 / 嵌套组，整图与脏矩形局部合成；
//! 2. **图层树**：`rows()`、`validate()`、插入 / 移动的算法成本（不碰合成）；
//! 3. **UI 帧**：图层面板用虚拟化 `List`，测 `update + layout + paint` 的帧成本
//!    与绘制命令数（1000 行和 10 行的命令数应该差不多）；
//! 4. **缩略图**：`scaled_to_fit(22)` 的单层成本（视口 + 250ms 节流后每帧只做
//!    几行，这里量的是每次重建的总成本）。
//!
//! 数字只用于同一台机器上的相对比较；报告的均值取 7 次采样的中位数，
//! 第一次迭代作为预热丢弃。

use std::time::{Duration, Instant};

use igui_core::{Size, ViewportSize};
use igui_render::PaintContext;

use crate::app::state::AppState;
use crate::document::{Color, Document, Layer, LayerMask, LayerRow, PixelBuffer};
use crate::renderer::{CpuRenderer, RenderTarget, Renderer};
use crate::ui::{EditorView, LayerAction};

/// 采样次数（含预热的那一次）。
const SAMPLES: usize = 7;

/// 基准入口：跑完打印报告，返回进程退出码。
pub fn run() -> i32 {
    println!("image_editor 性能基准");
    println!(
        "  构建：{}   （release 更有参考价值：cargo run --release -- --bench）",
        if cfg!(debug_assertions) {
            "debug（含 debug_assert）"
        } else {
            "release"
        }
    );
    println!();

    composite_scenarios();
    mask_scenarios();
    group_scenarios();
    tree_scenarios();
    ui_scenarios();
    thumbnail_scenarios();

    println!("说明：时间是单次调用的中位数（{SAMPLES} 次采样，首次预热）；");
    println!("      合成 / 缩略图只含 CPU 计算，不含上传与 GPU 绘制。");
    0
}

// -- 合成 -----------------------------------------------------------------

fn composite_scenarios() {
    println!("== 合成（CpuRenderer，整图 vs 脏矩形）==");
    println!("  文档        层数   形态        整图(ms)   局部 32²(ms)  局部 256²(ms)  ns/px");
    for (width, height) in [(128u32, 128u32), (2048, 2048), (4096, 2160)] {
        let counts: &[usize] = if width >= 4096 {
            &[1, 2, 4]
        } else if width >= 2048 {
            &[1, 4, 8]
        } else {
            &[1, 8, 32, 128]
        };
        for &count in counts {
            let document = flat_document(width, height, count);
            let mut target = RenderTarget::new(width, height);
            let renderer = CpuRenderer;
            renderer.render(&document, &mut target);
            let full = measure(|| renderer.render(&document, &mut target));
            let small_side = width.min(height).min(32);
            let medium_side = width.min(height).min(256);
            let region = |side: u32| {
                crate::document::PixelRegion::new(
                    (width - side) / 2,
                    (height - side) / 2,
                    side,
                    side,
                )
            };
            let small =
                measure(|| renderer.render_region(&document, &mut target, region(small_side)));
            let medium =
                measure(|| renderer.render_region(&document, &mut target, region(medium_side)));
            let ns_per_px = full * 1e6 / (width as f64 * height as f64);
            println!(
                "  {:>9}  {:>4}   平铺      {:>8.3}  {:>11.3}  {:>11.3}  {:>6.2}",
                format!("{width}×{height}"),
                count,
                full,
                small,
                medium,
                ns_per_px
            );
        }
    }
    println!();
}

fn mask_scenarios() {
    println!("== 合成：图层蒙版 / 组蒙版（128² 与 2048²）==");
    println!("  文档        形态                整图(ms)   256²(ms)");
    for (width, height) in [(128u32, 128u32), (2048, 2048)] {
        // 2K 的每层 16 MB，蒙版再加一份：层数压到 4，避免基准把内存吃爆。
        let layers = if width >= 2048 { 4 } else { 8 };
        let flat = flat_document(width, height, layers);
        let layer_masks = flat_with_layer_masks(width, height, layers);
        let grouped = grouped_document(width, height, layers / 2, 2, 0);
        let group_mask = grouped_document(width, height, layers / 2, 2, 1);
        let mut target = RenderTarget::new(width, height);
        let renderer = CpuRenderer;
        for (label, document) in [
            ("8 层，无蒙版", &flat),
            ("8 层，每层蒙版", &layer_masks),
            ("4 组 × 2 层，无组蒙版", &grouped),
            ("4 组 × 2 层，组蒙版", &group_mask),
        ] {
            renderer.render(document, &mut target);
            let full = measure(|| renderer.render(document, &mut target));
            let side = width.min(height).min(256);
            let region = measure(|| {
                renderer.render_region(
                    document,
                    &mut target,
                    crate::document::PixelRegion::new(
                        (width - side) / 2,
                        (height - side) / 2,
                        side,
                        side,
                    ),
                )
            });
            println!(
                "  {:>9}  {:<18}  {:>8.3}  {:>8.3}",
                format!("{width}×{height}"),
                label,
                full,
                region
            );
        }
    }
    println!("  （蒙版把 coverage=0 的像素整块跳过，所以有蒙版反而略快，不是异常。）");
    println!();
}

fn group_scenarios() {
    println!("== 合成：隔离组嵌套深度（128²，16 层）==");
    println!("  形态                      整图(ms)   局部 64²(ms)");
    let variants: [(&str, Document); 4] = [
        ("16 层平铺", flat_document(128, 128, 16)),
        ("4 组 × 4 层", grouped_document(128, 128, 4, 4, 0)),
        ("2 组 × (2 组 × 4 层)", nested_document(128, 128, 2, 2, 4)),
        ("1 组逐层嵌 16 深", chain_document(128, 128, 16)),
    ];
    let mut target = RenderTarget::new(128, 128);
    let renderer = CpuRenderer;
    for (label, document) in &variants {
        renderer.render(document, &mut target);
        let full = measure(|| renderer.render(document, &mut target));
        let region = measure(|| {
            renderer.render_region(
                document,
                &mut target,
                crate::document::PixelRegion::new(32, 32, 64, 64),
            )
        });
        println!("  {:<22}  {:>8.3}  {:>8.3}", label, full, region);
    }
    println!();
}

// -- 图层树 ---------------------------------------------------------------

fn tree_scenarios() {
    println!("== 图层树：纯算法（不合成）==");
    println!("  节点数   形态        rows(µs)  validate(ms)  insert(µs)  move(µs)");
    for &count in &[1_000usize, 10_000] {
        for (label, document) in [
            ("平铺", flat_document(8, 8, count)),
            ("组深 4", tree_document(8, 8, count, 4)),
        ] {
            let mut document = document;
            let rows: Vec<LayerRow> = document.rows();
            let rows_time = measure(|| {
                std::hint::black_box(document.rows());
            });
            let validate = measure(|| {
                let _ = std::hint::black_box(document.validate());
            });
            // 插一个新节点（每次测完删掉，保持规模不变）。
            let insert = measure(|| {
                let layer = Layer::new("bench", PixelBuffer::new(1, 1));
                let id = layer.id;
                if document.insert_subtree(vec![layer], None, usize::MAX) {
                    document.remove_subtree(id);
                }
            });
            let id = rows.last().map(|row| row.id);
            let move_time = match id {
                Some(id) => measure(|| {
                    let parent = document.layer(id).and_then(|layer| layer.parent);
                    let siblings = document.children(parent);
                    if let Some(index) = siblings.iter().position(|child| *child == id) {
                        let target = usize::MAX;
                        document.move_node(id, parent, target);
                        document.move_node(id, parent, index);
                    }
                }),
                None => f64::NAN,
            };
            println!(
                "  {:>7}  {:<8}  {:>7.2}  {:>9.3}  {:>8.2}  {:>7.2}",
                count,
                label,
                rows_time * 1000.0,
                validate,
                insert * 1000.0,
                move_time * 1000.0
            );
        }
    }
    println!();
}

// -- UI 帧 ----------------------------------------------------------------

fn ui_scenarios() {
    println!("== UI 帧：图层面板行数（稳态，虚拟化 List）==");
    println!("  行数    控件   绘制命令   update(ms)  layout(ms)  paint(ms)  整帧(ms)");
    for &count in &[8usize, 100, 1_000, 5_000] {
        let document = flat_document(128, 128, count);
        let state = AppState {
            document,
            ..AppState::default()
        };
        let mut view = EditorView::new(crate::theme::editor_theme(false), state);
        let viewport = ViewportSize::new(Size::new(1280.0, 800.0));
        view.layout(viewport);
        view.update();
        view.layout(viewport);

        let controls = view.control_count();
        let mut commands = 0;
        let update = measure(|| view.update());
        let layout = measure(|| view.layout(viewport));
        let paint = measure(|| {
            let mut ctx = PaintContext::new();
            view.paint(&mut ctx);
            commands = ctx.len();
        });
        // 真实一帧：update -> layout -> paint。
        let frame = measure(|| {
            view.update();
            view.layout(viewport);
            let mut ctx = PaintContext::new();
            view.paint(&mut ctx);
            std::hint::black_box(ctx.len());
        });
        println!(
            "  {:>5}  {:>5}  {:>8}  {:>10.3}  {:>10.3}  {:>8.3}  {:>8.3}",
            count, controls, commands, update, layout, paint, frame
        );
    }
    println!();

    // 编辑后的一帧：revision 变化 → refresh_layers 重建行缓存（+ 缩略图节流）。
    println!("== UI 帧：编辑后（revision 变化，重建行缓存）==");
    println!("  行数    整帧(ms)");
    for &count in &[8usize, 100, 1_000, 5_000] {
        let document = flat_document(128, 128, count);
        let state = AppState {
            document,
            ..AppState::default()
        };
        let mut view = EditorView::new(crate::theme::editor_theme(false), state);
        let viewport = ViewportSize::new(Size::new(1280.0, 800.0));
        view.layout(viewport);
        view.update();
        view.layout(viewport);
        let frame = measure(|| {
            view.layer_action(LayerAction::ToggleVisible);
            view.update();
            view.layout(viewport);
            let mut ctx = PaintContext::new();
            view.paint(&mut ctx);
            std::hint::black_box(ctx.len());
        });
        println!("  {:>5}  {:>8.3}", count, frame);
    }
    println!();
}

// -- 缩略图 ---------------------------------------------------------------

fn thumbnail_scenarios() {
    println!("== 缩略图：scaled_to_fit(22)（每次重建 / 每层）==");
    println!("  文档        每层(ms)   视口 20 行(ms)");
    for (width, height) in [(128u32, 128u32), (2048, 2048), (4096, 2160)] {
        let document = flat_document(width, height, 24);
        let layer = &document.layers[0];
        let scaled = measure(|| {
            std::hint::black_box(layer.pixels.scaled_to_fit(22));
        });
        // 列表行高 28：800px 高的窗口大约 20 行可见。
        let rows = measure(|| {
            for layer in document.layers.iter().take(20) {
                std::hint::black_box(layer.pixels.scaled_to_fit(22));
            }
        });
        println!(
            "  {:>9}  {:>8.3}  {:>12.3}",
            format!("{width}×{height}"),
            scaled,
            rows
        );
    }
    println!();
}

// -- 构造文档（直接拼 `layers`，保持前序 DFS 不变量） ----------------------

fn layer(name: &str, width: u32, height: u32, tint: u8) -> Layer {
    let mut layer = Layer::new(
        name,
        PixelBuffer::filled(width, height, Color::new(200, tint, 90, 128)),
    );
    // 顶层不透明黑，保证每层都真的参与合成。
    layer.opacity = 0.9;
    layer
}

/// `count` 个平铺层（从下到上），背景完全不透明。
fn flat_document(width: u32, height: u32, count: usize) -> Document {
    let mut document = Document::empty("bench", width, height, Color::BLACK);
    for index in 0..count {
        document
            .layers
            .push(layer(&format!("L{index}"), width, height, index as u8));
    }
    document.active_layer = document.layers.last().map(|layer| layer.id);
    document
}

/// 每个栅格层挂一张左半遮住的蒙版（走覆盖率合成路径）。
fn flat_with_layer_masks(width: u32, height: u32, count: usize) -> Document {
    let mut document = flat_document(width, height, count);
    for layer in &mut document.layers {
        layer.mask = Some(half_mask(width, height));
    }
    document
}

/// `groups` 个组，每组 `per_group` 个栅格层；`group_mask = 1` 时给组挂全显蒙版。
fn grouped_document(
    width: u32,
    height: u32,
    groups: usize,
    per_group: usize,
    group_mask: u8,
) -> Document {
    let mut document = Document::empty("bench", width, height, Color::BLACK);
    let mut tint = 0u8;
    for group_index in 0..groups {
        let mut group = Layer::group(format!("G{group_index}"));
        let group_id = group.id;
        if group_mask != 0 {
            group.mask = Some(half_mask(width, height));
        }
        document.layers.push(group);
        for _ in 0..per_group {
            let mut child = layer(&format!("L{tint}"), width, height, tint);
            child.parent = Some(group_id);
            document.layers.push(child);
            tint = tint.wrapping_add(1);
        }
    }
    document.active_layer = document.layers.last().map(|layer| layer.id);
    document
}

/// 左半遮住、右半全显的蒙版（0 与 1 两条采样分支都走到）。
fn half_mask(width: u32, height: u32) -> LayerMask {
    let mut mask = LayerMask::opaque(width, height);
    for y in 0..height {
        let row = y as usize * width as usize;
        for x in 0..width / 2 {
            let index = (row + x as usize) * 4;
            mask.pixels.data[index..index + 4].copy_from_slice(&[0, 0, 0, 255]);
        }
    }
    mask
}

/// `outer` 个组，每个里面再套 `inner` 个组、每组 `per_group` 个栅格层。
fn nested_document(
    width: u32,
    height: u32,
    outer: usize,
    inner: usize,
    per_group: usize,
) -> Document {
    let mut document = Document::empty("bench", width, height, Color::BLACK);
    let mut tint = 0u8;
    for o in 0..outer {
        let outer_group = Layer::group(format!("O{o}"));
        let outer_id = outer_group.id;
        document.layers.push(outer_group);
        for i in 0..inner {
            let mut group = Layer::group(format!("O{o}I{i}"));
            let group_id = group.id;
            group.parent = Some(outer_id);
            document.layers.push(group);
            for _ in 0..per_group {
                let mut child = layer(&format!("L{tint}"), width, height, tint);
                child.parent = Some(group_id);
                document.layers.push(child);
                tint = tint.wrapping_add(1);
            }
        }
    }
    document.active_layer = document.layers.last().map(|layer| layer.id);
    document
}

/// 1 个组一路嵌套 `depth` 层，每层里放一个栅格层（最深的组里放两个）。
fn chain_document(width: u32, height: u32, depth: usize) -> Document {
    let mut document = Document::empty("bench", width, height, Color::BLACK);
    let mut parent: Option<crate::document::LayerId> = None;
    for index in 0..depth {
        let mut group = Layer::group(format!("G{index}"));
        group.parent = parent;
        parent = Some(group.id);
        document.layers.push(group);
        // 每个组里一个栅格层（保持 16 层左右的总量级）。
        let mut child = layer(&format!("L{index}"), width, height, index as u8);
        child.parent = parent;
        document.layers.push(child);
    }
    document.active_layer = document.layers.last().map(|layer| layer.id);
    document
}

/// `count` 个节点的树：`depth` 层嵌套，剩下的节点都挂在最深的组下面。
fn tree_document(width: u32, height: u32, count: usize, depth: usize) -> Document {
    if depth <= 1 {
        return flat_document(width, height, count);
    }
    let mut document = Document::empty("bench", width, height, Color::BLACK);
    let levels = depth.min(count.saturating_sub(1));
    let mut parent: Option<crate::document::LayerId> = None;
    for index in 0..levels {
        let mut group = Layer::group(format!("G{index}"));
        group.parent = parent;
        parent = Some(group.id);
        document.layers.push(group);
    }
    for _ in 0..count.saturating_sub(levels) {
        let mut child = Layer::new("L", PixelBuffer::filled(width, height, Color::WHITE));
        child.parent = parent;
        document.layers.push(child);
    }
    document.active_layer = document.layers.last().map(|layer| layer.id);
    document
}

// -- 计时 -----------------------------------------------------------------

/// 跑 `SAMPLES` 次（第一次预热丢弃），返回中位数毫秒。
fn measure(mut work: impl FnMut()) -> f64 {
    let mut samples = Vec::with_capacity(SAMPLES);
    for index in 0..SAMPLES {
        let started = Instant::now();
        work();
        let elapsed = started.elapsed();
        if index > 0 {
            samples.push(elapsed);
        }
    }
    median_ms(&mut samples)
}

fn median_ms(samples: &mut [Duration]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.sort_unstable();
    let middle = samples.len() / 2;
    samples[middle].as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bench_documents_keep_the_tree_invariants() {
        for document in [
            flat_document(8, 8, 12),
            flat_with_layer_masks(8, 8, 4),
            grouped_document(8, 8, 2, 3, 1),
            nested_document(8, 8, 2, 2, 2),
            chain_document(8, 8, 4),
            tree_document(8, 8, 20, 3),
        ] {
            document.validate().expect("树不变量");
            assert!(!document.layers.is_empty());
            let rows = document.rows();
            assert!(rows.len() <= document.layers.len());
        }
    }

    #[test]
    fn measure_returns_a_median_in_milliseconds() {
        let mut samples = vec![
            Duration::from_millis(3),
            Duration::from_millis(1),
            Duration::from_millis(2),
        ];
        assert!((median_ms(&mut samples) - 2.0).abs() < 1e-6);
    }
}
