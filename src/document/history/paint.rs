//! 像素笔触命令：记录受影响区域的前后像素（一笔的撤销内存 ∝ 笔迹覆盖面积）。

use super::super::color::Color;
use super::super::document::{Document, PixelTarget};
use super::super::id::LayerId;
use super::super::pixel_buffer::PixelBuffer;
use super::super::region::PixelRegion;
use super::{Command, ReplayPatch};

/// 一次画笔 / 橡皮笔触：记录受影响区域的前后像素。
#[derive(Debug)]
pub struct PaintCommand {
    layer: LayerId,
    /// 写图层缓冲还是它的蒙版。
    target: PixelTarget,
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
        Self::capture_target(PixelTarget::Layer, layer, before, after, label)
    }

    /// 同 [`capture`](Self::capture)，但可以指定写图层还是蒙版。
    pub fn capture_target(
        target: PixelTarget,
        layer: LayerId,
        before: &PixelBuffer,
        after: &PixelBuffer,
        label: &'static str,
    ) -> Option<Self> {
        let region = PixelRegion::diff(before, after)?;
        Some(Self {
            layer,
            target,
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
        Self::capture_in_target(PixelTarget::Layer, layer, region, before, after, label)
    }

    /// 同 [`capture_in`](Self::capture_in)，但可以指定写图层还是蒙版。
    pub fn capture_in_target(
        target: PixelTarget,
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
            target,
            region,
            before: before_pixels,
            after: after_pixels,
            label,
        })
    }

    /// 命令写入的目标缓冲（目标被删掉时 `None`）。
    fn target_pixels<'a>(&self, document: &'a mut Document) -> Option<&'a mut PixelBuffer> {
        let layer = document.layer_mut(self.layer)?;
        match self.target {
            PixelTarget::Layer => Some(&mut layer.pixels),
            PixelTarget::Mask => layer.mask.as_mut().map(|mask| &mut mask.pixels),
        }
    }

    /// 把 `pixels` 写回目标（图层或蒙版）；目标被删掉时返回 `false`。
    fn write(&self, document: &mut Document, pixels: &[Color]) -> bool {
        match self.target_pixels(document) {
            Some(target) => {
                target.put_region(self.region, pixels);
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
        // `.igtl` 的补丁还没有「目标」字段：蒙版笔画先当作一次结构步骤
        // （有 label、没有像素补丁），等时间线格式升版再补。
        match self.target {
            PixelTarget::Layer => Some(ReplayPatch {
                layer: self.layer,
                region: self.region,
                pixels: &self.after,
            }),
            PixelTarget::Mask => None,
        }
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
