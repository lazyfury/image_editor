//! 画布：相机（缩放 / 平移）与坐标转换。
//!
//! [`EditorView::paint`](crate::ui::EditorView) 按 [`CanvasCamera::document_to_screen`]
//! 把文档图像、像素网格和选区画到屏幕空间（并裁到画布区域），所以
//! **文档像素坐标 -> 屏幕逻辑坐标** 只有一个定义，渲染和命中测试不会各算一套。
//! 这里只放纯数学，方便无头测试。

mod camera;
pub mod checkerboard;
mod coordinate;
mod path;
mod selection;

pub use camera::CanvasCamera;
#[cfg(test)]
pub use checkerboard::color_at;
pub use checkerboard::{paint_backdrop, paint_backdrop_in};
/// 棋盘格的两种格色（透明预览复用，保证跟画布一致）。
pub use checkerboard::{DARK as CHECKER_DARK, LIGHT as CHECKER_LIGHT};
pub use coordinate::{document_to_pixel, pixel_selection, screen_to_document};
pub use path::{flatten_segment, Anchor, Path};
pub use selection::{MergeMode, Selection};

use igui_render::TextureId;

/// 文档纹理的采样方式。
///
/// **放大（`zoom >= 1`）用最近邻**：像素画的硬边不该被插值糊掉；
/// **缩小用线性**：4K 文档适配到窗口是 0.18 倍，最近邻会直接丢像素，
/// 细笔迹变成断线 / 锯齿（就是「毛刺」）。宿主每帧把它翻成后端的
/// `TextureFilter`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageSmoothing {
    Nearest,
    Linear,
}

impl ImageSmoothing {
    /// 按缩放挑采样方式：倍率 ≥ 1 才用最近邻。
    pub fn for_zoom(zoom: f32) -> Self {
        if zoom >= 1.0 {
            Self::Nearest
        } else {
            Self::Linear
        }
    }
}

/// 文档合成结果在渲染后端里的纹理句柄。
///
/// 视图用它在 [`EditorView::paint`](crate::ui::EditorView) 里画文档图像，宿主用
/// 同一个 id 把合成结果注册进后端（`WgpuBackend::register_texture`）。约定一个
/// 固定值，两边不需要再互相传递句柄。
pub const DOCUMENT_TEXTURE: TextureId = TextureId::new(1);
