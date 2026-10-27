//! 画布相机：文档像素 -> 屏幕逻辑坐标。

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use igui_core::{Rect, Size, Transform2D, Vec2};

/// 相机的放大上限 / 下限。太小的缩放会让图像退化成一个点，太大则失去意义。
pub const MIN_ZOOM: f32 = 0.05;
pub const MAX_ZOOM: f32 = 64.0;

/// 直角判定容差（弧度）。吸附 / 测试给的角度是 `90f32.to_radians()` 这种值，
/// 浮点误差在 1e-7 量级，1e-4 足够宽松又不会把 1° 当成直角。
const ANGLE_EPSILON: f32 = 1e-4;

/// 画布相机。
///
/// 模型很简单：`offset` 是文档原点 `(0,0)` 在屏幕上的位置，`zoom` 是缩放，
/// `angle` 是**视图**旋转（文档像素不动）。因此
///
/// ```text
/// screen   = offset + R(angle) * (document * zoom)
/// document = R(-angle) * (screen - offset) / zoom
/// ```
///
/// 旋转只影响显示与命中测试：`document_to_screen` / `screen_to_document` 是唯一
/// 的两个方向，工具、选区、叠加层都走它们。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasCamera {
    pub zoom: f32,
    /// 视图旋转，弧度，顺时针为正，归一化到 `(-π, π]`。
    pub angle: f32,
    /// 文档原点在屏幕逻辑坐标里的位置。
    pub offset: Vec2,
}

impl Default for CanvasCamera {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            angle: 0.0,
            offset: Vec2::ZERO,
        }
    }
}

impl CanvasCamera {
    /// 便捷构造（钳制缩放、不旋转）。Phase 5 的工具与自检会用到。
    #[allow(dead_code)]
    pub fn new(zoom: f32, offset: Vec2) -> Self {
        Self {
            zoom: zoom.clamp(MIN_ZOOM, MAX_ZOOM),
            angle: 0.0,
            offset,
        }
    }

    /// 把角度归一化到 `(-π, π]`：连续旋转不会把 `f32` 精度耗光，
    /// 也让「是不是直角」这类判断不用管绕了几圈。
    pub fn normalize_angle(angle: f32) -> f32 {
        let mut angle = angle % TAU;
        if angle <= -PI {
            angle += TAU;
        }
        if angle > PI {
            angle -= TAU;
        }
        angle
    }

    /// 角度是不是 90° 的整数倍（直角旋转采样仍然精确，可以用最近邻）。
    pub fn is_right_angle(angle: f32) -> bool {
        let angle = Self::normalize_angle(angle);
        let quarter = (angle / FRAC_PI_2).round();
        (angle - quarter * FRAC_PI_2).abs() <= ANGLE_EPSILON
    }

    /// 相机当前是不是「正着看」（0°）。只有这种情况才走轴对齐快路径：
    /// 设备像素对齐 + 轴对齐矩形在旋转后没有定义。
    pub fn is_upright(self) -> bool {
        Self::normalize_angle(self.angle).abs() <= ANGLE_EPSILON
    }

    /// 屏幕空间旋转（文档 -> 屏幕方向）。
    fn rotate(self, vector: Vec2) -> Vec2 {
        let (sin, cos) = self.angle.sin_cos();
        Vec2::new(
            cos * vector.x - sin * vector.y,
            sin * vector.x + cos * vector.y,
        )
    }

    /// 屏幕空间逆旋转（屏幕 -> 文档方向）。
    fn unrotate(self, vector: Vec2) -> Vec2 {
        let (sin, cos) = self.angle.sin_cos();
        Vec2::new(
            cos * vector.x + sin * vector.y,
            -sin * vector.x + cos * vector.y,
        )
    }

    /// 文档坐标 -> 屏幕坐标。渲染与命中测试都走它，
    /// **文档像素坐标 -> 屏幕逻辑坐标** 只有一个定义。
    pub fn document_to_screen(self, document: Vec2) -> Vec2 {
        self.offset + self.rotate(document * self.zoom)
    }

    /// 屏幕坐标 -> 文档坐标（浮点；需要像素下标时再走
    /// [`document_to_pixel`](super::document_to_pixel)）。
    pub fn screen_to_document(self, screen: Vec2) -> Vec2 {
        self.unrotate(screen - self.offset) / self.zoom
    }

    /// 文档 -> 屏幕的仿射矩阵（`paint_document` 用它画旋转后的文档）。
    ///
    /// 和 [`document_to_screen`](Self::document_to_screen) 是同一个定义：
    /// 测试对着 `transform_point` 核对。
    pub fn to_transform(self) -> Transform2D {
        Transform2D::from_scale_rotation_origin(Vec2::splat(self.zoom), self.angle, self.offset)
    }

    /// 以 `screen` 为锚点缩放：锚点下的文档位置保持不动。
    pub fn zoom_at(&mut self, screen: Vec2, factor: f32) {
        let anchor = self.screen_to_document(screen);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.offset = screen - self.rotate(anchor * self.zoom);
    }

    /// 以 `screen` 为锚点旋转：锚点下的文档位置保持不动。
    pub fn rotate_by(&mut self, delta: f32, screen: Vec2) {
        self.rotate_to(self.angle + delta, screen);
    }

    /// 转到指定角度（弧度），锚点下的文档位置保持不动。
    pub fn rotate_to(&mut self, angle: f32, screen: Vec2) {
        let anchor = self.screen_to_document(screen);
        self.angle = Self::normalize_angle(angle);
        self.offset = screen - self.rotate(anchor * self.zoom);
    }

    /// 按屏幕像素平移。
    pub fn pan_by(&mut self, delta: Vec2) {
        self.offset += delta;
    }

    /// 给定缩放下，旋转后的文档在屏幕上的包围盒尺寸（不含 `offset`）。
    pub fn rotated_size(document: Size, zoom: f32, angle: f32) -> Vec2 {
        let (sin, cos) = angle.sin_cos();
        let (sin, cos) = (sin.abs(), cos.abs());
        Vec2::new(
            (cos * document.width + sin * document.height) * zoom,
            (sin * document.width + cos * document.height) * zoom,
        )
    }

    /// 以给定缩放把文档居中放到 `area` 里（不旋转）。
    pub fn centered(area: Rect, document: Size, zoom: f32) -> Self {
        Self::centered_rotated(area, document, zoom, 0.0)
    }

    /// 同上，但带着视图角度：文档**中心**落在区域中心，旋转不会把它甩出去。
    pub fn centered_rotated(area: Rect, document: Size, zoom: f32, angle: f32) -> Self {
        let mut camera = Self {
            zoom: zoom.clamp(MIN_ZOOM, MAX_ZOOM),
            angle: Self::normalize_angle(angle),
            offset: Vec2::ZERO,
        };
        let center = Vec2::new(document.width, document.height) * 0.5;
        camera.offset = area.center() - camera.rotate(center * camera.zoom);
        camera
    }

    /// 适配：取能放进 `area`（留 `padding`）的最大缩放，并居中（不旋转）。
    ///
    /// 正着看时等价于 `fit_rotated(.., 0.0)`；测试与「没有视图角度」的调用方用。
    #[allow(dead_code)]
    pub fn fit(area: Rect, document: Size, padding: f32) -> Self {
        Self::fit_rotated(area, document, padding, 0.0)
    }

    /// 同上，但按**旋转后的包围盒**算缩放：正方形转 45° 要多留 √2 的余量。
    pub fn fit_rotated(area: Rect, document: Size, padding: f32, angle: f32) -> Self {
        let available = Vec2::new(
            (area.size.width - 2.0 * padding).max(1.0),
            (area.size.height - 2.0 * padding).max(1.0),
        );
        let document = Size::new(document.width.max(1.0), document.height.max(1.0));
        let bounds = Self::rotated_size(document, 1.0, angle);
        let zoom = (available.x / bounds.x).min(available.y / bounds.y);
        Self::centered_rotated(area, document, zoom, angle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::from_min_size(Vec2::new(100.0, 50.0), Size::new(400.0, 300.0))
    }

    #[test]
    fn default_is_identity() {
        let camera = CanvasCamera::default();
        assert_eq!(
            camera.document_to_screen(Vec2::new(7.0, 9.0)),
            Vec2::new(7.0, 9.0)
        );
    }

    #[test]
    fn document_and_screen_round_trip() {
        let camera = CanvasCamera::new(2.0, Vec2::new(30.0, 40.0));
        let document = Vec2::new(5.0, 6.0);
        let screen = camera.document_to_screen(document);
        assert_eq!(screen, Vec2::new(40.0, 52.0));
        assert_eq!(camera.screen_to_document(screen), document);
    }

    #[test]
    fn smoothing_is_nearest_when_magnifying_and_linear_when_minifying() {
        use super::super::ImageSmoothing;
        // 4K 适配窗口 ~0.18 倍：必须线性，最近邻会把细笔迹采样成断线。
        assert_eq!(ImageSmoothing::for_zoom(0.18), ImageSmoothing::Linear);
        assert_eq!(ImageSmoothing::for_zoom(0.5), ImageSmoothing::Linear);
        // 100% 及放大：最近邻，像素画硬边不被插值糊掉。
        assert_eq!(ImageSmoothing::for_zoom(1.0), ImageSmoothing::Nearest);
        assert_eq!(ImageSmoothing::for_zoom(4.0), ImageSmoothing::Nearest);
        // 非直角旋转时最近邻会抖，只能线性；直角旋转采样仍然精确。
        assert_eq!(ImageSmoothing::for_view(4.0, 0.0), ImageSmoothing::Nearest);
        assert_eq!(
            ImageSmoothing::for_view(4.0, 30f32.to_radians()),
            ImageSmoothing::Linear
        );
        assert_eq!(
            ImageSmoothing::for_view(4.0, 90f32.to_radians()),
            ImageSmoothing::Nearest
        );
    }

    #[test]
    fn zooming_keeps_the_anchor_fixed() {
        let mut camera = CanvasCamera::new(1.0, Vec2::ZERO);
        let anchor = Vec2::new(120.0, 80.0);
        let before = camera.screen_to_document(anchor);
        camera.zoom_at(anchor, 2.0);
        assert_eq!(camera.zoom, 2.0);
        assert_eq!(camera.screen_to_document(anchor), before);
    }

    #[test]
    fn zoom_is_clamped() {
        let mut camera = CanvasCamera::default();
        camera.zoom_at(Vec2::ZERO, 1e9);
        assert_eq!(camera.zoom, MAX_ZOOM);
        camera.zoom_at(Vec2::ZERO, 1e-9);
        assert_eq!(camera.zoom, MIN_ZOOM);
    }

    #[test]
    fn pan_moves_the_origin() {
        let mut camera = CanvasCamera::default();
        camera.pan_by(Vec2::new(10.0, -4.0));
        assert_eq!(camera.offset, Vec2::new(10.0, -4.0));
    }

    #[test]
    fn fit_centers_the_document_in_the_area() {
        let camera = CanvasCamera::fit(area(), Size::new(200.0, 100.0), 20.0);
        // 可用 360×260，文档 200×100 -> zoom = min(1.8, 2.6) = 1.8
        assert!((camera.zoom - 1.8).abs() < 1e-4, "zoom = {}", camera.zoom);
        let center = camera.document_to_screen(Vec2::new(100.0, 50.0));
        assert!((center.x - 300.0).abs() < 1e-3, "center = {center:?}");
        assert!((center.y - 200.0).abs() < 1e-3, "center = {center:?}");
    }

    #[test]
    fn fit_to_a_tiny_area_still_has_a_usable_zoom() {
        let camera = CanvasCamera::fit(
            Rect::from_min_size(Vec2::ZERO, Size::new(4.0, 4.0)),
            Size::new(800.0, 600.0),
            16.0,
        );
        assert!(camera.zoom >= MIN_ZOOM);
        assert!(camera.zoom > 0.0);
    }

    // -- 视图旋转 --------------------------------------------------------

    fn rotated(angle: f32) -> CanvasCamera {
        CanvasCamera {
            zoom: 2.0,
            angle,
            offset: Vec2::new(30.0, 40.0),
        }
    }

    #[test]
    fn rotation_round_trips_and_matches_the_draw_transform() {
        let camera = rotated(30f32.to_radians());
        let document = Vec2::new(17.0, 9.0);
        let screen = camera.document_to_screen(document);
        // 绘制走 `to_transform`，命中测试走 `screen_to_document`：三者必须同源。
        let matrix = camera.to_transform().transform_point(document);
        assert!(
            (screen - matrix).length() < 1e-4,
            "{screen:?} vs {matrix:?}"
        );
        assert!((camera.screen_to_document(screen) - document).length() < 1e-4);
    }

    #[test]
    fn zooming_keeps_the_anchor_fixed_at_an_angle() {
        let mut camera = rotated(30f32.to_radians());
        let anchor = Vec2::new(120.0, 80.0);
        let before = camera.screen_to_document(anchor);
        camera.zoom_at(anchor, 1.5);
        assert!((camera.zoom - 3.0).abs() < 1e-4);
        assert!((camera.screen_to_document(anchor) - before).length() < 1e-4);
    }

    #[test]
    fn rotating_keeps_the_anchor_fixed() {
        let mut camera = rotated(10f32.to_radians());
        let anchor = Vec2::new(200.0, 150.0);
        let before = camera.screen_to_document(anchor);
        camera.rotate_by(20f32.to_radians(), anchor);
        assert!((camera.angle.to_degrees() - 30.0).abs() < 1e-3);
        assert!((camera.screen_to_document(anchor) - before).length() < 1e-4);
    }

    #[test]
    fn fit_accounts_for_the_rotated_bounds() {
        let document = Size::new(100.0, 100.0);
        let upright = CanvasCamera::fit(area(), document, 20.0);
        let turned = CanvasCamera::fit_rotated(area(), document, 20.0, 45f32.to_radians());
        // 正方形转 45°：包围盒边长 ×√2，所以缩放该是 1/√2。
        assert!(
            (upright.zoom / turned.zoom - std::f32::consts::SQRT_2).abs() < 1e-3,
            "upright = {}, turned = {}",
            upright.zoom,
            turned.zoom
        );
        // 中心仍然落在区域中心。
        let center = turned.document_to_screen(Vec2::new(50.0, 50.0));
        assert!(
            (center - area().center()).length() < 1e-3,
            "center = {center:?}"
        );
    }

    #[test]
    fn right_angles_are_axis_aligned_but_free_angles_are_not() {
        for degrees in [0.0f32, 90.0, 180.0, 270.0, -90.0, 360.0] {
            let camera = rotated(degrees.to_radians());
            assert!(
                CanvasCamera::is_right_angle(camera.angle),
                "{degrees}° 应是直角"
            );
            assert_eq!(camera.is_upright(), degrees == 0.0 || degrees == 360.0);
        }
        let free = rotated(30f32.to_radians());
        assert!(!CanvasCamera::is_right_angle(free.angle));
        assert!(!free.is_upright());
    }

    #[test]
    fn angle_is_normalized() {
        let mut camera = CanvasCamera::default();
        camera.rotate_by(std::f32::consts::TAU, Vec2::ZERO);
        assert_eq!(camera.angle, 0.0);
        let mut camera = CanvasCamera::default();
        camera.rotate_by(std::f32::consts::PI * 1.5, Vec2::ZERO);
        assert!((camera.angle + std::f32::consts::FRAC_PI_2).abs() < 1e-4);
    }

    #[test]
    fn rotated_centering_puts_the_document_center_on_the_area_center() {
        let camera = CanvasCamera::centered_rotated(
            area(),
            Size::new(200.0, 100.0),
            1.0,
            30f32.to_radians(),
        );
        let center = camera.document_to_screen(Vec2::new(100.0, 50.0));
        assert!(
            (center - area().center()).length() < 1e-3,
            "center = {center:?}"
        );
    }
}
