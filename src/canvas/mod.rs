//! 画布：相机（缩放 / 平移）与坐标转换。
//!
//! [`EditorView::paint`](crate::ui::EditorView) 按 [`CanvasCamera::document_to_screen`]
//! 把文档图像、像素网格和选区画到屏幕空间（并裁到画布区域），所以
//! **文档像素坐标 -> 屏幕逻辑坐标** 只有一个定义，渲染和命中测试不会各算一套。
//! 这里只放纯数学，方便无头测试。

mod camera;
mod checkerboard;
mod coordinate;

pub use camera::CanvasCamera;
#[cfg(test)]
pub use checkerboard::color_at;
pub use checkerboard::paint_backdrop;
/// 棋盘格的两种格色（透明预览复用，保证跟画布一致）。
pub use checkerboard::{DARK as CHECKER_DARK, LIGHT as CHECKER_LIGHT};
pub use coordinate::{document_to_pixel, pixel_selection, screen_to_document};

use igui_render::TextureId;

/// 文档合成结果在渲染后端里的纹理句柄。
///
/// 视图用它在 [`EditorView::paint`](crate::ui::EditorView) 里画文档图像，宿主用
/// 同一个 id 把合成结果注册进后端（`WgpuBackend::register_texture`）。约定一个
/// 固定值，两边不需要再互相传递句柄。
pub const DOCUMENT_TEXTURE: TextureId = TextureId::new(1);
