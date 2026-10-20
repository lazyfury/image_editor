//! 一笔的覆盖率蒙版 + 按蒙版合成。
//!
//! 笔迹上的 stamp 是密集重叠的（间距 = 直径/4），如果每压一次就往像素上混
//! 一次颜色，标着 50% 不透明度的笔会画出接近全黑（同一像素被压 ~4 次）。
//!
//! 所以每一笔记一个**覆盖率蒙版**：重复压到的像素只记最大覆盖率，合成时从
//! *落笔前的快照* 重算 —— 一个像素在这一笔里只会被"最终覆盖率 × 不透明度"
//! 影响一次。抬笔后蒙版随笔触一起丢掉，所以两笔相交仍然是标准的 `1-(1-a)²`。

use std::rc::Rc;

use crate::canvas::Selection;
use crate::document::{Color, PixelBuffer, Point};

use super::BrushMode;

/// 覆盖率蒙版：每个像素在本笔里的最大覆盖率（`0..=255`）。
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeMask {
    width: u32,
    height: u32,
    coverage: Vec<u8>,
}

impl StrokeMask {
    /// 和图层缓冲区同尺寸的空蒙版（全 0）。
    pub fn new(width: u32, height: u32) -> Self {
        let len = (width as usize).saturating_mul(height as usize);
        Self {
            width,
            height,
            coverage: vec![0; len],
        }
    }

    /// 记下 `(x, y)` 本笔的最大覆盖率。
    ///
    /// 返回 `Some(peak)` 表示需要按 `peak` 重算这个像素；`None` 表示这次不比
    /// 之前更覆盖它（像素不用动）。越界也是 `None`。
    pub fn raise(&mut self, x: u32, y: u32, coverage: f32) -> Option<f32> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let value = (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
        let index = y as usize * self.width as usize + x as usize;
        if value == 0 || value <= self.coverage[index] {
            return None;
        }
        self.coverage[index] = value;
        Some(value as f32 / 255.0)
    }
}

/// 一次 stamp 的写入上下文：目标缓冲区 + 落笔前快照 + 本笔蒙版 + 笔参数。
///
/// 只做「文档像素 -> 缓冲区索引 -> 按蒙版合成」这一件事；几何在
/// [`super::tip`]，笔触状态在 [`super`]。
pub struct Writer<'a> {
    pub mode: BrushMode,
    pub color: Color,
    /// 每一笔的不透明度（乘在蒙版覆盖率上）。
    pub opacity: f32,
    /// 选区裁剪（`None` = 不裁）。
    pub clip: Option<Rc<Selection>>,
    /// 图层缓冲区原点（文档坐标）。
    pub offset: Point,
    pub(super) before: &'a PixelBuffer,
    pub(super) mask: &'a mut StrokeMask,
    pub(super) target: &'a mut PixelBuffer,
}

impl Writer<'_> {
    /// 往文档坐标 `(doc_x, doc_y)` 合成 `coverage`；选区与图层边界在这里裁。
    pub fn put(&mut self, doc_x: i64, doc_y: i64, coverage: f32) {
        if let Some(clip) = self.clip.as_ref() {
            if doc_x < 0 || doc_y < 0 || !clip.contains(doc_x as u32, doc_y as u32) {
                return;
            }
        }
        let bx = doc_x - self.offset.x as i64;
        let by = doc_y - self.offset.y as i64;
        if bx < 0 || by < 0 || bx >= self.target.width as i64 || by >= self.target.height as i64 {
            return;
        }
        let (bx, by) = (bx as u32, by as u32);
        let Some(peak) = self.mask.raise(bx, by, coverage) else {
            return;
        };
        let alpha = peak * self.opacity;
        let base = self.before.get_pixel(bx, by);
        match self.mode {
            BrushMode::Paint => self.target.blend_from(bx, by, base, self.color, alpha),
            BrushMode::Erase => self.target.erase_from(bx, by, base, alpha),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个 8×8 的白色图层 + 它的快照 / 蒙版 / 写入上下文。
    struct Fixture {
        before: PixelBuffer,
        mask: StrokeMask,
        target: PixelBuffer,
    }

    impl Fixture {
        fn new() -> Self {
            let before = PixelBuffer::filled(8, 8, Color::WHITE);
            Self {
                mask: StrokeMask::new(8, 8),
                target: before.clone(),
                before,
            }
        }

        fn writer(&mut self) -> Writer<'_> {
            Writer {
                mode: BrushMode::Paint,
                color: Color::BLACK,
                opacity: 0.5,
                clip: None,
                offset: Point::ZERO,
                before: &self.before,
                mask: &mut self.mask,
                target: &mut self.target,
            }
        }

        fn pixel(&self, x: u32, y: u32) -> Color {
            self.target.get_pixel(x, y)
        }
    }

    #[test]
    fn a_pixel_is_composited_once_per_stroke_however_often_it_is_stamped() {
        // 同一位置压 4 次 50% 的 stamp：只能变一次（r ≈ 128），不能叠成黑。
        let mut fixture = Fixture::new();
        for _ in 0..4 {
            fixture.writer().put(2, 2, 1.0);
        }
        let color = fixture.pixel(2, 2);
        assert!((color.r as i32 - 128).abs() <= 1, "r = {}", color.r);
    }

    #[test]
    fn a_larger_coverage_still_updates_the_pixel() {
        // 先压 20% 再压 100%：结果是 100% 那一档（从快照重算，不是叠加）。
        let mut fixture = Fixture::new();
        fixture.writer().put(2, 2, 0.2);
        fixture.writer().put(2, 2, 1.0);
        let color = fixture.pixel(2, 2);
        assert!((color.r as i32 - 128).abs() <= 1, "r = {}", color.r);
    }

    #[test]
    fn an_erase_stroke_respects_the_mask_too() {
        let mut fixture = Fixture::new();
        for _ in 0..3 {
            let mut writer = fixture.writer();
            writer.mode = BrushMode::Erase;
            writer.put(3, 3, 1.0);
        }
        assert!((fixture.pixel(3, 3).a as i32 - 128).abs() <= 1);
    }

    #[test]
    fn the_selection_clips_the_stamp() {
        use crate::document::PixelRegion;

        let mut fixture = Fixture::new();
        let mut writer = fixture.writer();
        writer.clip = Some(Rc::new(Selection::rect(
            PixelRegion::new(0, 0, 4, 4),
            (8, 8),
        )));
        writer.put(2, 2, 1.0);
        writer.put(6, 6, 1.0);
        drop(writer);
        assert_ne!(fixture.pixel(2, 2), Color::WHITE, "选区内落笔");
        assert_eq!(fixture.pixel(6, 6), Color::WHITE, "选区外不落笔");
    }
}
