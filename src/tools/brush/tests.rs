//! 画笔 / 橡皮的单元测试（从 `mod.rs` 拆出来：那边只留实现）。
//!
//! 这里的每一条都对应一条规则：采样密度、像素完美连线、选区裁剪、每笔不透明度
//! 上限、脏矩形、撤销区域、对称镜像、颗粒映射与散布。

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

/// 一条横笔的 alpha 行（用来比较纹理的周期性）。
fn alpha_row(document: &Document, y: u32, from: u32, to: u32) -> Vec<u8> {
    (from..to).map(|x| at(document, x, y).a).collect()
}

/// 周期 `period` 上有多相似（1 = 完全一样）。
fn periodicity(row: &[u8], period: usize) -> f32 {
    if row.len() <= period {
        return 0.0;
    }
    let mut same = 0.0;
    let mut count = 0.0;
    for x in 0..row.len() - period {
        let delta = (f32::from(row[x]) - f32::from(row[x + period])).abs();
        same += 1.0 - delta / 255.0;
        count += 1.0;
    }
    if count == 0.0 {
        0.0
    } else {
        same / count
    }
}

/// 用给定映射 + 颗粒比例画一条横笔，返回中心行的 alpha。
fn stroke_row(mapping: GrainMapping, size: f32, scale: f32) -> Vec<u8> {
    let textures = crate::tools::BrushTextures::default();
    let mut document = Document::with_background("d", 240, 96, Color::TRANSPARENT);
    let mut brush = BrushTool::paint().with_color(Color::BLACK);
    brush.size = size;
    brush.hard = false;
    brush.hardness = 1.0;
    brush.shape = BrushShape::Round;
    brush.spacing_ratio = 0.08;
    brush.grain = textures
        .get("ink")
        .map(|texture| Grain::new(texture).with_mapping(mapping).with_scale(scale));
    assert!(brush.grain.is_some(), "应该加载到 ink.png");
    stroke(
        &mut brush,
        &mut document,
        &[Vec2::new(20.0, 48.0), Vec2::new(220.0, 48.0)],
    );
    alpha_row(&document, 48, 20, 220)
}

#[test]
fn a_bigger_grain_scale_is_what_breaks_the_repetition() {
    // 纹理的重复周期 = `颗粒比例 × 直径`（两种映射都一样）。所以"一整片区域不重复"
    // 靠的是把比例调大：跟笔的默认比例 1 = 每个直径重复一次（毛刷要的就是它）。
    let size = 40.0;
    let tight = stroke_row(GrainMapping::Dragged, size, 1.0);
    let loose = stroke_row(GrainMapping::CanvasLocked, size, 8.0);
    let period = size as usize;
    let repeated = periodicity(&tight, period);
    let not_repeated = periodicity(&loose, period);
    assert!(repeated > 0.9, "比例 1 应该每个直径重复一次：{repeated}");
    assert!(
        not_repeated < repeated - 0.2,
        "比例 8 的纹理不该按直径重复：{not_repeated} vs {repeated}"
    );
}

#[test]
fn a_canvas_locked_grain_is_pinned_to_the_document() {
    // 钉画布：纹理坐标来自文档坐标，所以往旁边挪 3px 再画，纹路跟着挪
    // （跟笔的纹路只跟弧长走，横向挪动纹路不变）。
    let row_at = |mapping: GrainMapping, y: f32| {
        let textures = crate::tools::BrushTextures::default();
        let mut document = Document::with_background("d", 160, 120, Color::TRANSPARENT);
        let mut brush = BrushTool::paint().with_color(Color::BLACK);
        brush.size = 40.0;
        brush.hard = false;
        brush.hardness = 1.0;
        brush.spacing_ratio = 0.08;
        brush.grain = textures
            .get("ink")
            .map(|texture| Grain::new(texture).with_mapping(mapping));
        stroke(
            &mut brush,
            &mut document,
            &[Vec2::new(20.0, y), Vec2::new(140.0, y)],
        );
        alpha_row(&document, y as u32, 20, 140)
    };
    let dragged_a = row_at(GrainMapping::Dragged, 40.0);
    let dragged_b = row_at(GrainMapping::Dragged, 43.0);
    let locked_a = row_at(GrainMapping::CanvasLocked, 40.0);
    let locked_b = row_at(GrainMapping::CanvasLocked, 43.0);
    assert!(
        periodicity(&dragged_a, 1) > 0.0,
        "两条笔迹都应该画出来（前提检查）"
    );
    let same = |a: &[u8], b: &[u8]| {
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|(x, y)| (*x as i32 - *y as i32).abs() <= 2)
    };
    assert!(same(&dragged_a, &dragged_b), "跟笔的纹路与横向位置无关");
    assert!(!same(&locked_a, &locked_b), "钉画布的纹路要随文档位置变");
}

#[test]
fn extra_stamps_scatter_around_the_path_without_leaving_it() {
    // 「个数 + 散布」：多出来的图章撒在笔迹两侧，但不该跑出 散布 × 直径 之外。
    let mut document = Document::with_background("d", 200, 120, Color::TRANSPARENT);
    let mut brush = BrushTool::paint().with_color(Color::BLACK);
    brush.size = 20.0;
    brush.hard = false;
    brush.hardness = 1.0;
    brush.shape = BrushShape::Round;
    brush.count = 6;
    brush.scatter = 0.5;
    brush.scatter_rotation = 1.0;
    brush.opacity = 0.2;
    stroke(
        &mut brush,
        &mut document,
        &[Vec2::new(40.0, 60.0), Vec2::new(160.0, 60.0)],
    );
    let pixels = &document.active_layer().unwrap().pixels;
    let mut above = 0;
    let mut below = 0;
    let mut stray = 0;
    for y in 0..pixels.height {
        for x in 0..pixels.width {
            if at(&document, x, y).a == 0 {
                continue;
            }
            let offset = y as f32 - 60.0;
            if offset > 1.0 {
                above += 1;
            } else if offset < -1.0 {
                below += 1;
            }
            // 散布 0.5 × 直径 20 = 10px，再加半个笔尖 = 20px。
            if offset.abs() > brush.size * 0.5 + brush.scatter * brush.size + 1.0 {
                stray += 1;
            }
        }
    }
    assert!(above > 0 && below > 0, "散布应该撒到笔迹两侧");
    assert_eq!(stray, 0, "散布飞出范围了：{stray} 个像素");
    let _ = (above, below);
}

#[test]
fn the_same_seed_paints_exactly_the_same_scattered_stroke() {
    // 散布靠随机数，但同种子必须同像素（不然测试没法断言，用户也没法复现）。
    let render = || {
        let mut document = Document::with_background("d", 120, 80, Color::TRANSPARENT);
        let mut brush = BrushTool::paint().with_color(Color::BLACK);
        brush.size = 12.0;
        brush.hard = false;
        brush.count = 4;
        brush.scatter = 0.6;
        brush.scatter_rotation = 1.0;
        stroke(
            &mut brush,
            &mut document,
            &[Vec2::new(20.0, 40.0), Vec2::new(100.0, 40.0)],
        );
        painted(&document)
    };
    assert_eq!(render(), render(), "同种子的两笔应该完全一样");
}

#[test]
fn a_textured_brush_leaves_a_non_uniform_impression() {
    // 毛笔预设的笔尖纹理有毛束 / 湿边：同一笔里应该有明显深浅，还有「没压到」的
    // 空隙（纯程序化笔尖只会有中心深、边缘一圈渐变）。
    let mut document = Document::with_background("d", 160, 96, Color::TRANSPARENT);
    let textures = crate::tools::BrushTextures::default();
    let mut brush = BrushTool::paint().with_color(Color::BLACK);
    brush.size = 48.0;
    brush.hard = false;
    brush.hardness = 1.0;
    brush.shape = BrushShape::Round;
    brush.grain = textures
        .get("ink")
        .map(|texture| Grain::new(texture).with_mapping(GrainMapping::Dragged));
    brush.spacing_ratio = 0.12;
    assert!(brush.grain.is_some(), "应该加载到 ink.png");
    stroke(
        &mut brush,
        &mut document,
        &[Vec2::new(40.0, 48.0), Vec2::new(120.0, 48.0)],
    );

    let mut alphas = std::collections::BTreeSet::new();
    let mut holes = 0;
    for y in 20..76 {
        for x in 30..130 {
            let alpha = at(&document, x, y).a;
            if alpha > 0 {
                alphas.insert(alpha);
            } else if (x as i32 - 80).abs() < 20 {
                holes += 1;
            }
        }
    }
    assert!(
        alphas.len() >= 8,
        "纹理笔应该画出多档深浅，实际 {} 档",
        alphas.len()
    );
    assert!(holes > 20, "笔尖纹理里应该有没压到的空隙，实际 {holes}");
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
fn a_vertical_symmetry_stroke_is_mirrored_about_the_document_mid_line() {
    // 32×32 文档、垂直对称：左边画一下，右边对称位置也有。像素级镜像
    // （x' = width - 1 - x），像素画也严格对称。
    let mut document = document();
    let mut brush = BrushTool::paint().with_color(Color::RED);
    brush.symmetry = Symmetry::Vertical;
    stroke(&mut brush, &mut document, &[Vec2::new(10.0, 20.0)]);
    assert_eq!(at(&document, 10, 20), Color::RED);
    assert_eq!(at(&document, 21, 20), Color::RED, "x' = 32 - 1 - 10");
    assert_eq!(at(&document, 20, 20), Color::WHITE, "中间不该被压");
}

#[test]
fn four_way_symmetry_presses_four_pixels_and_off_presses_one() {
    let mut off = document();
    let mut brush = BrushTool::paint().with_color(Color::RED);
    stroke(&mut brush, &mut off, &[Vec2::new(6.0, 4.0)]);
    assert_eq!(at(&off, 6, 4), Color::RED);
    assert_eq!(at(&off, 25, 4), Color::WHITE, "关掉对称只有一处");

    let mut both = document();
    let mut brush = BrushTool::paint().with_color(Color::RED);
    brush.symmetry = Symmetry::Both;
    stroke(&mut brush, &mut both, &[Vec2::new(6.0, 4.0)]);
    for (x, y) in [(6, 4), (25, 4), (6, 27), (25, 27)] {
        assert_eq!(at(&both, x, y), Color::RED, "({x}, {y})");
    }
}

#[test]
fn full_smoothing_lags_the_ink_by_half_a_segment() {
    // 默认（平滑 1）：曲线只能画到「当前点与下一个点的中点」，所以一次 move
    // 之后光标处的像素还没落笔 —— 抬笔时才补完。
    let mut document = document();
    let mut history = History::new();
    let mut brush = BrushTool::paint().with_color(Color::RED);
    brush.on_pointer_down(
        &mut context(&mut document, &mut history),
        left(Vec2::new(4.0, 16.0)),
    );
    brush.on_pointer_move(
        &mut context(&mut document, &mut history),
        left(Vec2::new(28.0, 16.0)),
    );
    assert_eq!(
        at(&document, 28, 16),
        Color::WHITE,
        "平滑模式下落笔滞后：光标处的像素还没压到"
    );
    assert_eq!(at(&document, 16, 16), Color::RED, "中点已经画到");
    brush.on_pointer_up(
        &mut context(&mut document, &mut history),
        left(Vec2::new(28.0, 16.0)),
    );
    assert_eq!(at(&document, 28, 16), Color::RED, "抬笔时补完最后半段");
}

#[test]
fn zero_smoothing_follows_the_cursor_exactly() {
    // 平滑 0：折线直连，move 一下光标处立刻落笔（像素画要的「笔到手到」）。
    let mut document = document();
    let mut history = History::new();
    let mut brush = BrushTool::paint().with_color(Color::RED);
    brush.smooth = 0.0;
    brush.on_pointer_down(
        &mut context(&mut document, &mut history),
        left(Vec2::new(4.0, 4.0)),
    );
    brush.on_pointer_move(
        &mut context(&mut document, &mut history),
        left(Vec2::new(28.0, 4.0)),
    );
    assert_eq!(at(&document, 28, 4), Color::RED, "光标处应立刻落笔");
    brush.on_pointer_move(
        &mut context(&mut document, &mut history),
        left(Vec2::new(28.0, 28.0)),
    );
    assert_eq!(at(&document, 28, 4), Color::RED, "拐角仍然在");
    assert_eq!(at(&document, 28, 28), Color::RED, "第二段也到位");
    brush.on_pointer_up(
        &mut context(&mut document, &mut history),
        left(Vec2::new(28.0, 28.0)),
    );
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
fn the_undo_region_is_the_stroke_box_not_the_whole_layer() {
    // 抬笔不再扫整层 diff：撤销区域 = 这一笔实际写过的包围盒。
    let mut document = Document::new("d", 64, 64);
    let mut history = History::new();
    let mut brush = BrushTool::paint().with_color(Color::RED);
    brush.size = 4.0;
    brush.hard = false;
    {
        let mut ctx = context(&mut document, &mut history);
        brush.stroke_path(&mut ctx, &[Vec2::new(10.0, 10.0), Vec2::new(30.0, 12.0)]);
    }
    let region = history.undo_region().expect("应记录撤销区域");
    assert!(
        region.width <= 26 && region.height <= 10,
        "区域应贴着笔迹（4px 笔 + 20px 长），实际 {region:?}"
    );
    assert!(region.area() < 64 * 64 / 4, "不该是整层：{region:?}");
}

#[test]
fn a_stroke_that_changes_nothing_pushes_no_command() {
    // 整笔落在文档外：脏区里没有任何真实变化 -> 不入栈（有 bbox 也一样）。
    let mut document = document();
    let mut history = History::new();
    let mut brush = BrushTool::paint().with_color(Color::RED);
    {
        let mut ctx = context(&mut document, &mut history);
        brush.stroke_path(
            &mut ctx,
            &[Vec2::new(-50.0, -50.0), Vec2::new(-40.0, -50.0)],
        );
    }
    assert!(!history.can_undo());
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
