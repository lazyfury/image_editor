//! 洪水填充的纯算法：容差比较 + 连续 / 全局扫描，按文档坐标的选区裁剪。

use crate::document::{Color, PixelBuffer, Point};

use crate::canvas::Selection;

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
pub(super) fn region(
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
