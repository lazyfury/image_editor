//! 轮廓：把点串 / 掩码变成**合并过**的边界线段（整块矩形只有 4 条）。

use igui_core::Vec2;

use super::Edge;

/// 把一串点收成闭合的线段（最后一段回到起点）。
pub(super) fn closed_edges(points: &[Vec2]) -> Vec<Edge> {
    if points.len() < 2 {
        return Vec::new();
    }
    (0..points.len())
        .map(|index| (points[index], points[(index + 1) % points.len()]))
        .collect()
}

/// 掩码的边界边，**按段合并**：整块矩形只有 4 条边，4K 全选不再产出上万条线段。
///
/// 横边逐条行边界比较「上一行 vs 这一行」，竖边逐条列边界比较左右两侧；相邻的
/// 差异段合成一条线段，所以描边（蚂蚁线）命令数从「每像素一条」降到「每段一条」。
pub(super) fn mask_edges(width: u32, height: u32, mask: &[u8]) -> Vec<Edge> {
    // 描边画在「覆盖率 ≥ 50%」那圈上：羽化后的边界看起来才在选区边缘。
    let selected = |x: i64, y: i64| -> bool {
        x >= 0
            && y >= 0
            && x < width as i64
            && y < height as i64
            && mask[y as usize * width as usize + x as usize] >= 128
    };
    let mut edges = Vec::new();

    // 横边：行边界 y 两侧选中状态不同的区间。
    for y in 0..=height as i64 {
        let mut x = 0i64;
        while x < width as i64 {
            if selected(x, y - 1) == selected(x, y) {
                x += 1;
                continue;
            }
            let start = x;
            while x < width as i64 && selected(x, y - 1) != selected(x, y) {
                x += 1;
            }
            edges.push((
                Vec2::new(start as f32, y as f32),
                Vec2::new(x as f32, y as f32),
            ));
        }
    }

    // 竖边：列边界 x 两侧选中状态不同的区间。
    for x in 0..=width as i64 {
        let mut y = 0i64;
        while y < height as i64 {
            if selected(x - 1, y) == selected(x, y) {
                y += 1;
                continue;
            }
            let start = y;
            while y < height as i64 && selected(x - 1, y) != selected(x, y) {
                y += 1;
            }
            edges.push((
                Vec2::new(x as f32, start as f32),
                Vec2::new(x as f32, y as f32),
            ));
        }
    }

    edges
}
