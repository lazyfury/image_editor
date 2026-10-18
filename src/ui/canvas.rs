//! 画布区域。
//!
//! 这里**不画任何东西**：文档图像、像素网格和选区由
//! [`EditorView::paint`](crate::ui::EditorView) 在画布区域的裁剪里绘制。这一段
//! 只是一块透明区域，用来取 fit 区域、做裁剪和命中测试（滚轮缩放 / 中键平移）。
//! 用 `MouseFilter::Ignore` 把它从 UI 命中里摘出去，否则那块矩形会把指针事件
//! 全部吃掉。

use igui_components::{Component, Flex};
use igui_ui::MouseFilter;

/// 一块透明的画布区域。
pub fn canvas_area() -> impl Component {
    Flex::new().grow(1.0).mouse_filter(MouseFilter::Ignore)
}
