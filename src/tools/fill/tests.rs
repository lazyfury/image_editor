//! 填充测试：洪水填充的容差 / 连续 / 裁剪 / 撤销，按选区填充，魔棒区域。

use super::*;

use crate::canvas::Selection;
use crate::document::{Color, Document, History, PixelRegion, Point};
use crate::tools::fill::flood::flood_fill;
use crate::tools::tool::{PointerEvent, Tool, ToolContext};

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
        magic_wand_region(&layer.pixels, Point::new(0, 0), (16, 16), (99, 99), 0, true).is_none()
    );
}
