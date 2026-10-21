//! 选区测试：矩形 / 多边形 / 掩码 / 羽化 / 合并 / 轮廓。

use super::*;

#[test]
fn select_all_covers_every_pixel_and_its_outline_is_the_document_border() {
    let selection = Selection::all((16, 12));
    assert_eq!(selection.area(), 16 * 12);
    assert!(selection.contains(0, 0) && selection.contains(15, 11));
    // 边界线按段合并：整块矩形只剩外框的 4 条边（以前是每像素一条，共 56 条）。
    assert_eq!(selection.outline().len(), 4, "{:?}", selection.outline());
}

#[test]
fn invert_swaps_inside_and_outside() {
    let selection = Selection::rect(PixelRegion::new(2, 2, 4, 4), (10, 10));
    let inverted = selection.invert();
    assert_eq!(inverted.area(), 10 * 10 - selection.area());
    assert!(!inverted.contains(3, 3), "原来选中的现在不选中");
    assert!(inverted.contains(0, 0) && inverted.contains(9, 9));
    // 轮廓仍然是闭合的一圈（能画出来），不是空的。
    assert!(!inverted.outline().is_empty());
}

#[test]
fn inverting_twice_returns_the_original_mask() {
    // 掩码来回一趟不变；轮廓会按掩码边界重算（套索的折线轮廓换成像素边），
    // 所以只比掩码。
    let selection = Selection::polygon(
        &[
            Vec2::new(1.0, 1.0),
            Vec2::new(8.0, 2.0),
            Vec2::new(5.0, 9.0),
        ],
        (10, 10),
    );
    let twice = selection.invert().invert();
    assert_eq!(twice.area(), selection.area());
    for y in 0..10 {
        for x in 0..10 {
            assert_eq!(twice.contains(x, y), selection.contains(x, y), "({x}, {y})");
        }
    }
}

#[test]
fn a_wand_style_mask_gets_merged_edges_and_cached_area() {
    // 4×4 文档，选中一竖条的中间两像素：包围盒、面积都对，边按段合并。
    let mut mask = vec![0u8; 16];
    for y in 1..3 {
        mask[y * 4 + 2] = 255;
    }
    let selection = Selection::from_mask((4, 4), mask);
    assert_eq!(selection.area(), 2);
    assert_eq!(selection.bounds(), Some(PixelRegion::new(2, 1, 1, 2)));
    // 横边 2 条（上下各一）+ 竖边 2 条（左右各一），都是合并后的段。
    assert_eq!(selection.outline().len(), 4);
    assert!(!selection.is_empty());
    assert_eq!(selection.area(), 2, "area 是缓存的，不重复扫掩码");
}

#[test]
fn an_empty_mask_has_no_bounds() {
    let selection = Selection::from_mask((4, 4), vec![0u8; 16]);
    assert_eq!(selection.area(), 0);
    assert!(selection.is_empty());
    assert_eq!(selection.bounds(), None);
    assert!(selection.outline().is_empty());
}

#[test]
fn union_and_subtract_combine_the_masks() {
    let left = Selection::rect(PixelRegion::new(0, 0, 4, 4), (10, 10));
    let right = Selection::rect(PixelRegion::new(2, 0, 4, 4), (10, 10));
    // 并集：0..6 一共 6×4 个像素（中间重叠 2 列）。
    assert_eq!(left.union(&right).area(), 6 * 4);
    // 差集：左边去掉重叠的 2 列。
    assert_eq!(left.subtract(&right).area(), 2 * 4);
    // 完全挖空 -> 空选区。
    assert_eq!(left.subtract(&left).area(), 0);
    // 尺寸不同不 panic，退回原样。
    let other_doc = Selection::rect(PixelRegion::new(0, 0, 2, 2), (20, 20));
    assert_eq!(left.union(&other_doc), left);
}

#[test]
fn merge_mode_follows_the_modifier_keys() {
    assert_eq!(MergeMode::from_modifiers(false, false), MergeMode::Replace);
    assert_eq!(MergeMode::from_modifiers(true, false), MergeMode::Add);
    assert_eq!(MergeMode::from_modifiers(false, true), MergeMode::Subtract);
    assert_eq!(
        MergeMode::from_modifiers(true, true),
        MergeMode::Subtract,
        "Alt 优先"
    );
}

#[test]
fn merging_without_a_base_selection_is_just_the_new_one() {
    let next = Selection::rect(PixelRegion::new(1, 1, 2, 2), (8, 8));
    assert_eq!(
        MergeMode::Add.apply(None, next.clone()).map(|s| s.area()),
        Some(4)
    );
    assert_eq!(
        MergeMode::Subtract
            .apply(None, next.clone())
            .map(|s| s.area()),
        Some(4),
        "没有旧选区时减选 = 新选区"
    );
    // 减空了就没有选区。
    let base = Selection::rect(PixelRegion::new(0, 0, 8, 8), (8, 8));
    assert_eq!(MergeMode::Subtract.apply(Some(&base), base.clone()), None);
}

#[test]
fn feathering_fades_the_edges_without_moving_them() {
    // 16×16 全选羽化 2px：外圈变淡、中心仍然是满覆盖，描边仍在外框上。
    let selection = Selection::all((16, 16)).feathered(2.0);
    assert!(
        selection.coverage(0, 8) < 0.5,
        "边缘应变淡：{}",
        selection.coverage(0, 8)
    );
    assert_eq!(selection.coverage(8, 8), 1.0, "中心仍满覆盖");
    // 面积按覆盖率折算，所以羽化后比原来小。
    assert!(selection.area() < 16 * 16);
    assert!(selection.area() > 16 * 16 / 2, "不该掉太多");
    // 选区还在（boudns/轮廓没被抹掉）。
    assert_eq!(selection.bounds(), Some(PixelRegion::new(0, 0, 16, 16)));
    assert!(!selection.outline().is_empty());
}

#[test]
fn feathering_with_a_zero_radius_is_a_no_op() {
    let selection = Selection::rect(PixelRegion::new(2, 2, 4, 4), (10, 10));
    assert_eq!(selection.feathered(0.0), selection);
    assert_eq!(selection.feathered(-3.0), selection);
}

#[test]
fn a_rect_selection_matches_the_pixel_region() {
    let region = PixelRegion::new(2, 3, 4, 5);
    let selection = Selection::rect(region, (10, 10));
    for y in 0..10 {
        for x in 0..10 {
            assert_eq!(
                selection.contains(x, y),
                region.contains(x, y),
                "({x}, {y})"
            );
        }
    }
    assert_eq!(selection.area(), region.area());
    assert_eq!(selection.bounds(), Some(region));
}

#[test]
fn a_rect_outline_is_the_four_sides() {
    let selection = Selection::rect(PixelRegion::new(2, 3, 4, 5), (10, 10));
    assert_eq!(
        selection.outline(),
        &[
            (Vec2::new(2.0, 3.0), Vec2::new(6.0, 3.0)),
            (Vec2::new(6.0, 3.0), Vec2::new(6.0, 8.0)),
            (Vec2::new(6.0, 8.0), Vec2::new(2.0, 8.0)),
            (Vec2::new(2.0, 8.0), Vec2::new(2.0, 3.0)),
        ]
    );
}

/// 三角形套索：斜边以内选中，以外不选。
#[test]
fn a_triangle_polygon_fills_only_its_interior() {
    let points = [
        Vec2::new(2.0, 2.0),
        Vec2::new(8.0, 2.0),
        Vec2::new(2.0, 8.0),
    ];
    let selection = Selection::polygon(&points, (10, 10));
    assert!(selection.contains(3, 3), "靠近直角的内部");
    assert!(!selection.contains(7, 7), "斜边以外");
    assert!(!selection.contains(0, 0), "多边形以外");
    assert_eq!(selection.bounds(), Some(PixelRegion::new(2, 2, 5, 5)));
}

/// 凹多边形（C 形）：凹口里的像素不该被选中。
#[test]
fn a_concave_polygon_excludes_its_notch() {
    let points = [
        Vec2::new(2.0, 2.0),
        Vec2::new(8.0, 2.0),
        Vec2::new(8.0, 4.0),
        Vec2::new(4.0, 4.0),
        Vec2::new(4.0, 6.0),
        Vec2::new(8.0, 6.0),
        Vec2::new(8.0, 8.0),
        Vec2::new(2.0, 8.0),
    ];
    let selection = Selection::polygon(&points, (10, 10));
    assert!(selection.contains(3, 3), "C 的上臂");
    assert!(selection.contains(3, 7), "C 的下臂");
    assert!(selection.contains(6, 3), "C 的右侧");
    assert!(!selection.contains(6, 5), "凹口里不该选中");
}

#[test]
fn a_degenerate_polygon_selects_nothing() {
    let selection = Selection::polygon(&[Vec2::new(1.0, 1.0), Vec2::new(2.0, 2.0)], (8, 8));
    assert!(selection.is_empty());
    assert_eq!(selection.bounds(), None);
}

/// 掩码构造：包含关系照搬掩码，轮廓是「选中像素朝向未选中邻居」的边。
#[test]
fn a_mask_selection_traces_its_boundary() {
    // 3×3 里只有中心一个像素被选中。
    let mut mask = vec![0u8; 9];
    mask[4] = 255;
    let selection = Selection::from_mask((3, 3), mask);
    assert!(selection.contains(1, 1));
    assert!(!selection.contains(0, 0));
    assert_eq!(selection.area(), 1);
    assert_eq!(selection.bounds(), Some(PixelRegion::new(1, 1, 1, 1)));
    assert_eq!(
        selection.outline(),
        &[
            (Vec2::new(1.0, 1.0), Vec2::new(2.0, 1.0)),
            (Vec2::new(1.0, 2.0), Vec2::new(2.0, 2.0)),
            (Vec2::new(1.0, 1.0), Vec2::new(1.0, 2.0)),
            (Vec2::new(2.0, 1.0), Vec2::new(2.0, 2.0)),
        ]
    );
}
