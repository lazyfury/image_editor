//! 像素笔触命令：记录受影响区域的前后像素（一笔的撤销内存 ∝ 笔迹覆盖面积）。

use super::super::color::Color;
use super::super::document::Document;
use super::super::id::LayerId;
use super::super::pixel_buffer::PixelBuffer;
use super::super::region::PixelRegion;
use super::{Command, ReplayPatch};

/// 一次画笔 / 橡皮笔触：记录受影响区域的前后像素。
#[derive(Debug)]
pub struct PaintCommand {
    layer: LayerId,
    region: PixelRegion,
    before: Vec<Color>,
    after: Vec<Color>,
    label: &'static str,
}

impl PaintCommand {
    /// 从落笔前快照与当前图层缓冲抓出差异区域；没有像素变化时返回 `None`。
    pub fn capture(
        layer: LayerId,
        before: &PixelBuffer,
        after: &PixelBuffer,
        label: &'static str,
    ) -> Option<Self> {
        let region = PixelRegion::diff(before, after)?;
        Some(Self {
            layer,
            region,
            before: before.region(region),
            after: after.region(region),
            label,
        })
    }

    /// 用调用方给的区域（画笔的脏矩形）抓差异，不扫整层。
    ///
    /// `region` 是**图层缓冲区坐标**；区域内没有像素变化（例如整笔都被选区 /
    /// 图层边界挡掉）时返回 `None`，不入栈。
    pub fn capture_in(
        layer: LayerId,
        region: PixelRegion,
        before: &PixelBuffer,
        after: &PixelBuffer,
        label: &'static str,
    ) -> Option<Self> {
        let region = region.clamped(after.width, after.height);
        if region.area() == 0 {
            return None;
        }
        let before_pixels = before.region(region);
        let after_pixels = after.region(region);
        if before_pixels == after_pixels {
            return None;
        }
        Some(Self {
            layer,
            region,
            before: before_pixels,
            after: after_pixels,
            label,
        })
    }

    /// 把 `pixels` 写回目标图层；图层已被删掉时返回 `false`。
    fn write(&self, document: &mut Document, pixels: &[Color]) -> bool {
        match document.layer_mut(self.layer) {
            Some(layer) => {
                layer.pixels.put_region(self.region, pixels);
                true
            }
            None => false,
        }
    }
}

impl Command for PaintCommand {
    fn region(&self) -> Option<PixelRegion> {
        Some(self.region)
    }

    fn replay(&self) -> Option<ReplayPatch<'_>> {
        Some(ReplayPatch {
            layer: self.layer,
            region: self.region,
            pixels: &self.after,
        })
    }

    fn execute(&mut self, document: &mut Document) {
        if self.write(document, &self.after) {
            document.touch();
        }
    }

    fn undo(&mut self, document: &mut Document) {
        if self.write(document, &self.before) {
            document.touch();
        }
    }

    fn label(&self) -> &'static str {
        self.label
    }

    fn translate_for(&mut self, layer: LayerId, dx: i32, dy: i32) {
        if self.layer != layer {
            return;
        }
        self.region = self.region.translated(dx.max(0) as u32, dy.max(0) as u32);
    }
}
