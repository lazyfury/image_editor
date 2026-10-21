//! 羽化：对掩码做两遍盒式模糊（近似高斯），边缘从 255 淡到 0。

use super::Selection;

impl Selection {
    /// 羽化：把掩码做两遍盒式模糊（近似高斯），边缘从 255 平滑淡到 0。
    ///
    /// `radius` 是文档像素；`0` 或负值返回原样。两遍盒式模糊足够平滑，
    /// 而且代价只有两趟 O(宽 × 高)，4K 上也就几十毫秒（一次点击的事）。
    pub fn feathered(&self, radius: f32) -> Self {
        let radius = radius.round() as i64;
        if radius < 1 {
            return self.clone();
        }
        let (width, height) = (self.width as i64, self.height as i64);
        if width == 0 || height == 0 {
            return self.clone();
        }
        let mut mask: Vec<u8> = self.mask.clone();
        for _ in 0..2 {
            mask = box_blur(&mask, width, height, radius, false);
            mask = box_blur(&mask, width, height, radius, true);
        }
        Self::from_mask((self.width, self.height), mask)
    }
}

/// 一维盒式模糊（`vertical = false` 沿行、`true` 沿列）。
///
/// 越界按 **0**（未选中）算 —— 和 PS 一致：选区贴着画布边缘时，羽化会把那一边
/// 向内淡出，而不是当作「外面也选中」。
fn box_blur(mask: &[u8], width: i64, height: i64, radius: i64, vertical: bool) -> Vec<u8> {
    let mut out = vec![0u8; mask.len()];
    let (outer, inner) = if vertical {
        (width, height)
    } else {
        (height, width)
    };
    let index = |o: i64, i: i64| -> usize {
        if vertical {
            (i * width + o) as usize
        } else {
            (o * width + i) as usize
        }
    };
    let sample = |o: i64, i: i64| -> u32 {
        if i < 0 || i >= inner {
            0
        } else {
            mask[index(o, i)] as u32
        }
    };
    let window = (2 * radius + 1) as u32;
    for o in 0..outer {
        let mut sum: u32 = (-radius..=radius).map(|i| sample(o, i)).sum();
        for i in 0..inner {
            out[index(o, i)] = (sum / window) as u8;
            sum = sum + sample(o, i + radius + 1) - sample(o, i - radius);
        }
    }
    out
}
