//! 图层：像素 + 可见性 / 透明度 / 混合模式 / 位置 / 蒙版 / 分组。

use super::id::LayerId;
use super::pixel_buffer::PixelBuffer;
use super::point::Point;

/// 图层混合模式。Phase 2 只有 `Normal`。
///
/// TODO(v1.1): 增加 `Multiply` / `Screen` / `Overlay` / `Darken` / `Lighten`
/// 等；合成在 Phase 4/GPU 阶段接进来，这里先把枚举形状定好，避免以后改
/// `Layer` 的公共字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BlendMode {
    #[default]
    Normal,
}

/// 节点类型：栅格层（有像素）或图层组（只有子节点与自己的属性）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LayerKind {
    #[default]
    Raster,
    /// 组：没有像素（`pixels` 恒为 0×0），合成时先叠子节点再整体压回父级。
    Group,
}

/// 一张图层蒙版：灰度覆盖率图。
///
/// 约定 `r == g == b` = 覆盖率（`0` 隐藏、`255` 显示），`a = 255`；尺寸与
/// 所属栅格层的像素缓冲一致（组蒙版是文档大小）。画笔把颜色按亮度写回来，
/// 所以「画黑 = 隐藏、画白 = 显示」。
#[derive(Debug, Clone, PartialEq)]
pub struct LayerMask {
    pub pixels: PixelBuffer,
    pub enabled: bool,
}

impl LayerMask {
    /// 全显（全白）的蒙版。
    #[allow(dead_code)] // 测试与「从选区创建」的调用方用。
    pub fn opaque(width: u32, height: u32) -> Self {
        Self {
            pixels: PixelBuffer::filled(width, height, super::color::Color::WHITE),
            enabled: true,
        }
    }

    /// `(x, y)` 处的覆盖率（`0.0..=1.0`）；停用的蒙版恒为 1。
    pub fn coverage(&self, x: u32, y: u32) -> f32 {
        if !self.enabled {
            return 1.0;
        }
        self.pixels.get_pixel(x, y).r as f32 / 255.0
    }
}

/// 一个图层节点。像素尺寸由 `PixelBuffer` 决定，可以小于/大于文档；
/// 组节点的缓冲恒为空。
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub kind: LayerKind,
    /// 父组（`None` = 根）。组的子节点在 `Document::layers` 里是连续区间。
    pub parent: Option<LayerId>,
    /// 组是否展开（UI 状态；不进撤销，但写进工作区文件）。
    pub expanded: bool,
    pub visible: bool,
    /// 0.0..=1.0，写入时会被 [`Layer::set_opacity`] / [`Document`] 钳制。
    ///
    /// [`Document`]: super::Document
    pub opacity: f32,
    pub blend_mode: BlendMode,
    /// 图层像素缓冲区的原点：`pixels[0, 0]` 在文档坐标里的位置。
    ///
    /// 可以是负数、缓冲区也可以比文档大，所以移出画布的内容会留在缓冲里
    /// （可以再移回来），不会因为移动而丢掉。组恒为 [`Point::ZERO`]。
    pub position: Point,
    pub pixels: PixelBuffer,
    /// 蒙版：像素蒙版，覆盖率乘到这一层的 alpha 上。
    pub mask: Option<LayerMask>,
}

impl Layer {
    /// 一个可见、不透明、`Normal`、原点对齐的新栅格图层。
    pub fn new(name: impl Into<String>, pixels: PixelBuffer) -> Self {
        Self {
            id: LayerId::next(),
            name: name.into(),
            kind: LayerKind::Raster,
            parent: None,
            expanded: true,
            visible: true,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            position: Point::ZERO,
            pixels,
            mask: None,
        }
    }

    /// 一个空图层组（没有像素）。
    pub fn group(name: impl Into<String>) -> Self {
        Self {
            kind: LayerKind::Group,
            pixels: PixelBuffer::new(0, 0),
            ..Self::new(name, PixelBuffer::new(0, 0))
        }
    }

    pub fn is_group(&self) -> bool {
        self.kind == LayerKind::Group
    }

    /// 设置不透明度，钳到 `0.0..=1.0`（非有限值按 1.0 处理）。
    #[allow(dead_code)] // 由图层不透明度控件消费（面板走 Document::set_layer_opacity）。
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = clamp_opacity(opacity);
    }

    /// 蒙版在**图层缓冲坐标** `(x, y)` 上的覆盖率；没有蒙版 / 停用时是 1。
    pub fn mask_coverage(&self, x: u32, y: u32) -> f32 {
        match &self.mask {
            Some(mask) => mask.coverage(x, y),
            None => 1.0,
        }
    }
}

/// 把任意浮点钳成合法的图层不透明度。
pub(crate) fn clamp_opacity(opacity: f32) -> f32 {
    if opacity.is_finite() {
        opacity.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_layer_defaults_are_sane() {
        let layer = Layer::new("Layer 1", PixelBuffer::new(4, 4));
        assert_eq!(layer.name, "Layer 1");
        assert_eq!(layer.kind, LayerKind::Raster);
        assert_eq!(layer.parent, None);
        assert!(layer.expanded);
        assert!(layer.visible);
        assert_eq!(layer.opacity, 1.0);
        assert_eq!(layer.blend_mode, BlendMode::Normal);
        assert_eq!(layer.position, Point::ZERO);
        assert!(layer.mask.is_none());
    }

    #[test]
    fn a_group_has_no_pixels_and_no_position() {
        let group = Layer::group("组");
        assert!(group.is_group());
        assert_eq!((group.pixels.width, group.pixels.height), (0, 0));
        assert_eq!(group.position, Point::ZERO);
        assert!(group.expanded);
    }

    #[test]
    fn set_opacity_clamps_and_rejects_non_finite() {
        let mut layer = Layer::new("L", PixelBuffer::new(1, 1));
        layer.set_opacity(0.25);
        assert_eq!(layer.opacity, 0.25);
        layer.set_opacity(-2.0);
        assert_eq!(layer.opacity, 0.0);
        layer.set_opacity(3.0);
        assert_eq!(layer.opacity, 1.0);
        layer.set_opacity(f32::NAN);
        assert_eq!(layer.opacity, 1.0);
    }

    #[test]
    fn mask_coverage_is_one_without_a_mask_and_tracks_the_red_channel() {
        let mut layer = Layer::new("L", PixelBuffer::new(1, 1));
        assert_eq!(layer.mask_coverage(0, 0), 1.0);
        let mut mask = LayerMask::opaque(1, 1);
        mask.pixels
            .set_pixel(0, 0, super::super::color::Color::BLACK);
        layer.mask = Some(mask.clone());
        assert_eq!(layer.mask_coverage(0, 0), 0.0);
        mask.enabled = false;
        layer.mask = Some(mask);
        assert_eq!(layer.mask_coverage(0, 0), 1.0);
    }
}
