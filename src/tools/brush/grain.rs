//! 颗粒：笔尖上那层纹理（覆盖率蒙版）在一笔里怎么映射到画布上。
//!
//! Procreate 把这三种方式叫「颗粒」的三态（默认每站重贴 / 「动态」/「纹理化」），
//! Photoshop 的对应物是「纹理」+ 图案混合模式。差别只在于 **uv 从哪来**：
//!
//! - [`GrainMapping::Stamp`]：uv 来自**笔尖本地坐标** —— 每个 dab 重贴同一张图
//!   （图案笔 / 像素笔）。
//! - [`GrainMapping::Dragged`]：uv 来自**笔迹坐标系** —— 横向跨笔迹、纵向沿弧长滚动。
//!   毛笔 / 刷子的毛靠这个：整条笔迹共享一片连续纹理，不会每个 dab 从头开始。
//! - [`GrainMapping::CanvasLocked`]：uv 来自**文档坐标** —— 纹理钉在画布上、不随笔移动
//!   （Procreate 的「纹理化」）。一整片区域的纹理是连续的，这是"区域描边"的观感来源。
//!
//! 三态共用同一套调制：`depth`（颗粒相对底色的强度：0 = 纯底色、1 = 纯颗粒）、
//! `min_depth`（抬最暗处，免得稀疏颗粒把笔迹打断）、`scale`（一块纹理覆盖几个直径）。

use std::rc::Rc;

use igui_core::Vec2;

use super::texture::BrushTexture;

/// 颗粒在一笔里的坐标映射方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GrainMapping {
    /// 每站重贴：纹理跟着笔尖走（图案笔）。
    #[default]
    Stamp,
    /// 钉在画布：纹理坐标取文档坐标（Procreate「纹理化」，区域连续）。
    CanvasLocked,
    /// 跟笔拖：纹理沿弧长滚动（Procreate「动态」/ 毛笔的毛）。
    Dragged,
}

/// 一个 dab 的几何 + 这一笔的上下文：几何决定**哪些像素**被盖住，
/// 这里的东西决定**纹理取哪一块**。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DabFrame {
    /// 圆心（文档坐标）。
    pub center: Vec2,
    /// 直径（文档像素）。
    pub size: f32,
    /// 固定旋转（`Stamp` 映射用；`Dragged` 由笔迹方向决定）。
    pub angle: f32,
    /// 笔迹方向（单位向量，`Dragged` 用）。
    pub direction: Vec2,
    /// 这一笔已经走过的弧长（`Dragged` 的滚动量）。
    pub distance: f32,
    /// 每笔一次的随机偏移（`CanvasLocked` 用，让两笔的纹路不重合）。
    pub offset: Vec2,
}

impl DabFrame {
    /// 一个不旋转、不滚动的图章（测试helper）。
    #[cfg(test)]
    pub fn stamp(center: Vec2, size: f32) -> Self {
        Self {
            center,
            size,
            angle: 0.0,
            direction: Vec2::new(1.0, 0.0),
            distance: 0.0,
            offset: Vec2::new(0.0, 0.0),
        }
    }

    pub fn with_center(mut self, center: Vec2) -> Self {
        self.center = center;
        self
    }

    /// 纹理一块占多少文档像素：0 或负 = 跟着直径走（Procreate「跟进尺寸」）。
    pub fn tile(&self, scale: f32) -> f32 {
        (self.size.max(1.0) * scale.max(0.05)).max(1.0)
    }
}

/// 一块颗粒纹理 + 它怎么铺。
#[derive(Debug, Clone, PartialEq)]
pub struct Grain {
    pub texture: Rc<BrushTexture>,
    pub mapping: GrainMapping,
    /// `Stamp` 映射的固定旋转（弧度）。
    pub angle: f32,
    /// 一块纹理覆盖几个直径（1 = 每直径一块；< 1 = 更密）。
    pub scale: f32,
    /// 0..=1：0 = 忽略纹理（纯底色），1 = 纯纹理（毛束之间留空）。
    pub depth: f32,
    /// 0..=1：把纹理最暗处抬到这个值以上，避免稀疏颗粒把笔迹打断。
    pub min_depth: f32,
    /// 颗粒对比度（1 = 原样；> 1 把浅淡的素材（比如一张偏白的图）拉开）。
    pub contrast: f32,
    /// 颗粒亮度偏移（-1..=1）。
    pub brightness: f32,
}

impl Grain {
    pub fn new(texture: Rc<BrushTexture>) -> Self {
        Self {
            texture,
            mapping: GrainMapping::Stamp,
            angle: 0.0,
            scale: 1.0,
            depth: 1.0,
            min_depth: 0.0,
            contrast: 1.0,
            brightness: 0.0,
        }
    }

    /// 当前挂着的纹理（`Rc` 共享，比指针就够判断"同一张图"）。
    pub fn texture(&self) -> &Rc<BrushTexture> {
        &self.texture
    }

    pub fn with_mapping(mut self, mapping: GrainMapping) -> Self {
        self.mapping = mapping;
        self
    }

    pub fn with_angle(mut self, angle: f32) -> Self {
        self.angle = angle;
        self
    }

    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    pub fn with_depth(mut self, depth: f32) -> Self {
        self.depth = depth;
        self
    }

    pub fn with_min_depth(mut self, min_depth: f32) -> Self {
        self.min_depth = min_depth;
        self
    }

    pub fn with_contrast(mut self, contrast: f32) -> Self {
        self.contrast = contrast;
        self
    }

    pub fn with_brightness(mut self, brightness: f32) -> Self {
        self.brightness = brightness;
        self
    }

    /// 纹理坐标：`local` 是相对圆心的位移（文档像素）。
    ///
    /// 返回**未回绕**的 uv；各分支自己决定哪里取 `fract`（跨笔迹的 u 不回转，
    /// 免得笔迹边缘出现接缝）。
    fn uv(&self, frame: DabFrame, local: Vec2) -> (f32, f32) {
        let tile = frame.tile(self.scale);
        match self.mapping {
            GrainMapping::Stamp => {
                let (sin, cos) = (-self.angle).sin_cos();
                let lx = local.x * cos - local.y * sin;
                let ly = local.x * sin + local.y * cos;
                (
                    (lx / tile + 0.5).clamp(0.0, 0.999_999),
                    (ly / tile + 0.5).clamp(0.0, 0.999_999),
                )
            }
            GrainMapping::Dragged => {
                let length = frame.direction.length();
                let direction = if length > 1e-4 {
                    frame.direction / length
                } else {
                    Vec2::new(1.0, 0.0)
                };
                let normal = Vec2::new(-direction.y, direction.x);
                let across = (local.x * normal.x + local.y * normal.y) / tile + 0.5;
                let along = (local.x * direction.x + local.y * direction.y + frame.distance) / tile;
                (across.clamp(0.0, 0.999_999), along.rem_euclid(1.0))
            }
            GrainMapping::CanvasLocked => {
                // 文档坐标 -> 纹理坐标：笔往哪画，露出来的都是同一片纹路。
                let x = frame.center.x + local.x + frame.offset.x;
                let y = frame.center.y + local.y + frame.offset.y;
                ((x / tile).rem_euclid(1.0), (y / tile).rem_euclid(1.0))
            }
        }
    }

    /// 这个像素上的颗粒覆盖率（已经算进 `depth` / `min_depth`）。
    ///
    /// `linear` = 双线性采样（像素模式传 `false`，硬边不会被插值糊掉）。
    pub fn coverage(&self, frame: DabFrame, local: Vec2, linear: bool) -> f32 {
        let (u, v) = self.uv(frame, local);
        let sample = self.texture.sample(u, v, linear).clamp(0.0, 1.0);
        // 对比度 / 亮度：Procreate 颗粒属性里的同名项。以**素材自己的均值**为轴，
        // 所以"整体偏白"的图也能被拉开，而不是一头顶到 1。
        let mean = self.texture.mean();
        let sample = ((sample - mean) * self.contrast.clamp(0.0, 8.0) + mean + self.brightness)
            .clamp(0.0, 1.0);
        // `min_depth` 抬最暗处：稀疏的毛束不会把笔迹切成虚线。
        let sample =
            self.min_depth.clamp(0.0, 1.0) + (1.0 - self.min_depth.clamp(0.0, 1.0)) * sample;
        // `depth` 在「纯底色」和「纯颗粒」之间插值。
        let depth = self.depth.clamp(0.0, 1.0);
        1.0 - depth + depth * sample
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Color, PixelBuffer};
    use crate::tools::brush::texture::BrushTexture;

    /// 左右各一半的纹理：左 0、右 1（哪边亮一眼就看出来）。
    fn half_texture() -> Rc<BrushTexture> {
        let mut pixels = PixelBuffer::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                let value = if x < 4 { 0 } else { 255 };
                pixels.set_pixel(x, y, Color::new(255, 255, 255, value));
            }
        }
        Rc::new(BrushTexture::from_pixels(&pixels).expect("蒙版"))
    }

    /// 25% 白点的棋盘纹理（用来数平铺次数）。
    fn dots_texture() -> Rc<BrushTexture> {
        let mut pixels = PixelBuffer::new(4, 4);
        for y in 0..4 {
            for x in 0..4 {
                let value = if x == 0 && y == 0 { 255 } else { 0 };
                pixels.set_pixel(x, y, Color::new(255, 255, 255, value));
            }
        }
        Rc::new(BrushTexture::from_pixels(&pixels).expect("蒙版"))
    }

    fn frame(center: Vec2, size: f32) -> DabFrame {
        DabFrame::stamp(center, size)
    }

    #[test]
    fn a_stamp_mapping_repeats_the_texture_at_every_dab() {
        // 每站重贴：两个相距很远的 dab，相对位置一样 -> 采样值一样。
        let grain = Grain::new(half_texture());
        let a = frame(Vec2::new(10.0, 10.0), 16.0);
        let b = frame(Vec2::new(110.0, 40.0), 16.0);
        let local = Vec2::new(2.0, 0.0);
        assert_eq!(
            grain.coverage(a, local, true),
            grain.coverage(b, local, true)
        );
        // 左半暗、右半亮。
        assert_eq!(grain.coverage(a, Vec2::new(-3.0, 0.0), true), 0.0);
        assert_eq!(grain.coverage(a, Vec2::new(3.0, 0.0), true), 1.0);
    }

    #[test]
    fn a_canvas_locked_mapping_moves_with_the_canvas_not_the_dab() {
        // 钉在画布：两个 dab 的**绝对位置**决定采样值，local 只是画布上的一点。
        let grain = Grain::new(half_texture()).with_mapping(GrainMapping::CanvasLocked);
        let a = frame(Vec2::new(0.0, 0.0), 16.0);
        let b = frame(Vec2::new(0.0, 0.0), 64.0);
        // 同一块画布上同一个点，笔大笔小都一样。
        let point = Vec2::new(6.0, 0.0);
        assert_eq!(
            grain.coverage(a, point, true),
            grain.coverage(b, point, true)
        );
        // 往右挪 8px（= 半个 tile）采样值就翻过来。
        let left = grain.coverage(frame(Vec2::new(0.0, 0.0), 16.0), Vec2::new(2.0, 0.0), true);
        let right = grain.coverage(frame(Vec2::new(8.0, 0.0), 16.0), Vec2::new(2.0, 0.0), true);
        assert_ne!(left, right, "钉画布的纹理应该随笔的绝对位置变");
    }

    /// 上半亮、下半暗的纹理：只看 v 就知道沿笔迹滚到哪儿了。
    fn v_half_texture() -> Rc<BrushTexture> {
        let mut pixels = PixelBuffer::new(4, 8);
        for y in 0..8 {
            for x in 0..4 {
                let value = if y < 4 { 255 } else { 0 };
                pixels.set_pixel(x, y, Color::new(255, 255, 255, value));
            }
        }
        Rc::new(BrushTexture::from_pixels(&pixels).expect("4×8"))
    }

    #[test]
    fn a_dragged_mapping_scrolls_along_the_stroke_arc() {
        // 跟笔：纹理沿弧长滚动 —— 走半块纹理就翻到另一半。
        let grain = Grain::new(v_half_texture()).with_mapping(GrainMapping::Dragged);
        let local = Vec2::new(0.0, 0.0);
        let at = |distance: f32| {
            let mut frame = frame(Vec2::new(50.0, 50.0), 16.0);
            frame.direction = Vec2::new(1.0, 0.0);
            frame.distance = distance;
            grain.coverage(frame, local, false)
        };
        assert_eq!(at(0.0), 1.0, "起笔在亮的那一半");
        assert_eq!(at(8.0), 0.0, "走过半块 = 暗的那一半");
        assert_eq!(at(16.0), 1.0, "走满一块回到原点");
        // 跨笔迹方向不滚动：只平移 local 不影响 v。
        assert_eq!(at(0.0), at(0.0));
    }

    #[test]
    fn depth_zero_switches_the_grain_off_and_min_depth_lifts_the_dark_end() {
        let grain = Grain::new(half_texture());
        let dab = frame(Vec2::new(0.0, 0.0), 16.0);
        // depth 0 = 纯底色（没有纹理）。
        let flat = grain.clone().with_depth(0.0);
        assert_eq!(flat.coverage(dab, Vec2::new(-3.0, 0.0), true), 1.0);
        assert_eq!(flat.coverage(dab, Vec2::new(3.0, 0.0), true), 1.0);
        // 半深：暗处不再是 0。
        let half = grain.clone().with_depth(0.5);
        assert!((half.coverage(dab, Vec2::new(-3.0, 0.0), true) - 0.5).abs() < 1e-6);
        // min_depth 抬最暗处。
        let floored = grain.with_min_depth(0.4);
        assert!((floored.coverage(dab, Vec2::new(-3.0, 0.0), true) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn contrast_and_brightness_rescue_a_washed_out_texture() {
        // 一张"几乎全白"的素材（0.8）当颗粒：原样几乎看不出纹理。
        let mut pixels = PixelBuffer::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                let value = if (x + y) % 2 == 0 { 230 } else { 205 };
                pixels.set_pixel(x, y, Color::new(255, 255, 255, value));
            }
        }
        let texture = Rc::new(BrushTexture::from_pixels(&pixels).expect("8×8"));
        let grain = Grain::new(texture).with_mapping(GrainMapping::CanvasLocked);
        let dab = frame(Vec2::new(0.0, 0.0), 16.0);
        let spread = |grain: &Grain| {
            let mut low = 1.0f32;
            let mut high = 0.0f32;
            for step in 0..32 {
                let x = step as f32 * 0.5;
                let value = grain.coverage(dab, Vec2::new(x, 0.0), false);
                low = low.min(value);
                high = high.max(value);
            }
            high - low
        };
        let flat = spread(&grain);
        let pushed = spread(&grain.clone().with_contrast(8.0));
        assert!(flat < 0.15, "原样应该几乎没差：{flat}");
        assert!(pushed > 0.5, "拉对比度之后应该看得出纹理：{pushed}");
        // 亮度偏移是整体抬 / 压。
        let darker = grain.clone().with_brightness(-0.2);
        assert!(
            darker.coverage(dab, Vec2::new(0.0, 0.0), false)
                < grain.coverage(dab, Vec2::new(0.0, 0.0), false)
        );
    }

    #[test]
    fn a_smaller_scale_tiles_the_texture_more_often() {
        // 沿一条横线数「亮 -> 暗」跳变：scale 越小，一块纹理越窄，跳变越多。
        let grain = Grain::new(dots_texture()).with_mapping(GrainMapping::CanvasLocked);
        let dab = frame(Vec2::new(0.0, 0.0), 64.0);
        let transitions = |grain: &Grain| {
            let mut last = grain.coverage(dab, Vec2::new(-40.0, 0.0), false);
            let mut count = 0;
            for step in 1..=400 {
                let x = -40.0 + step as f32 * 0.2;
                let value = grain.coverage(dab, Vec2::new(x, 0.0), false);
                if (value - last).abs() > 0.5 {
                    count += 1;
                }
                last = value;
            }
            count
        };
        let coarse = transitions(&grain.clone().with_scale(1.0));
        let fine = transitions(&grain.clone().with_scale(0.25));
        assert!(
            fine > coarse * 2,
            "scale 0.25 应该密得多：{fine} vs {coarse}"
        );
    }
}
