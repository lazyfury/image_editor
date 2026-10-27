//! 系统光标映射：画布工具 / igui 控件 -> winit `CursorIcon`。
//!
//! 这一层只做纯映射（可单测），不碰窗口：宿主每帧把结果通过
//! `SharedWindow` 设给 winit。`igui_core::Cursor` 目前只有 7 种（没有
//! Crosshair / ZoomIn / ZoomOut），所以画布上的工具光标直接走 winit。
//!
//! 任意位图光标（自定义 PNG）不在这一层：那需要 winit 的 `CustomCursor` +
//! `ActiveEventLoop`，留给上游 igui。

use igui_core::Cursor as IguiCursor;
use winit::window::CursorIcon;

use crate::app::state::ActiveTool;

/// 指针在画布上时按当前工具/拖动状态选光标。
///
/// - 平移中（中键 / 抓手 / 空格）→ `Grabbing`；按住空格或抓手待命 → `Grab`
/// - 缩放工具 → `ZoomIn`，按住 `Option/Alt` → `ZoomOut`
/// - 画笔 / 橡皮 / 选区 / 钢笔 / 填充 / 吸管 → `Crosshair`
/// - 移动工具 → `Move`
pub fn canvas(tool: ActiveTool, panning: bool, space_held: bool, alt: bool) -> CursorIcon {
    if panning {
        return CursorIcon::Grabbing;
    }
    if space_held || tool == ActiveTool::Hand {
        return CursorIcon::Grab;
    }
    match tool {
        ActiveTool::Zoom if alt => CursorIcon::ZoomOut,
        ActiveTool::Zoom => CursorIcon::ZoomIn,
        ActiveTool::Move => CursorIcon::Move,
        ActiveTool::Hand => CursorIcon::Grab,
        ActiveTool::Brush
        | ActiveTool::Eraser
        | ActiveTool::RectangleSelect
        | ActiveTool::LassoSelect
        | ActiveTool::Pen
        | ActiveTool::PaintBucket
        | ActiveTool::MagicWand
        | ActiveTool::Gradient
        | ActiveTool::RectangleFill
        | ActiveTool::FreeFill
        | ActiveTool::Eyedropper => CursorIcon::Crosshair,
    }
}

/// igui 控件自带的 hover 光标 -> winit。
pub fn igui(cursor: IguiCursor) -> CursorIcon {
    match cursor {
        IguiCursor::Default => CursorIcon::Default,
        IguiCursor::Pointer => CursorIcon::Pointer,
        IguiCursor::Text => CursorIcon::Text,
        IguiCursor::ColResize => CursorIcon::ColResize,
        IguiCursor::RowResize => CursorIcon::RowResize,
        IguiCursor::Grab => CursorIcon::Grab,
        IguiCursor::Grabbing => CursorIcon::Grabbing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panning_always_shows_the_closed_hand() {
        for tool in ActiveTool::ALL {
            assert_eq!(
                canvas(tool, true, false, false),
                CursorIcon::Grabbing,
                "{tool:?}"
            );
        }
    }

    #[test]
    fn space_and_the_hand_tool_show_the_open_hand() {
        assert_eq!(
            canvas(ActiveTool::Brush, false, true, false),
            CursorIcon::Grab
        );
        assert_eq!(
            canvas(ActiveTool::Hand, false, false, false),
            CursorIcon::Grab
        );
    }

    #[test]
    fn zoom_uses_the_magnifier_and_option_reverses_it() {
        assert_eq!(
            canvas(ActiveTool::Zoom, false, false, false),
            CursorIcon::ZoomIn
        );
        assert_eq!(
            canvas(ActiveTool::Zoom, false, false, true),
            CursorIcon::ZoomOut
        );
    }

    #[test]
    fn pixel_tools_are_a_crosshair_and_move_is_move() {
        for tool in [
            ActiveTool::Brush,
            ActiveTool::Eraser,
            ActiveTool::RectangleSelect,
            ActiveTool::Pen,
            ActiveTool::Eyedropper,
        ] {
            assert_eq!(canvas(tool, false, false, false), CursorIcon::Crosshair);
        }
        assert_eq!(
            canvas(ActiveTool::Move, false, false, false),
            CursorIcon::Move
        );
    }

    #[test]
    fn every_igui_cursor_has_a_winit_counterpart() {
        assert_eq!(igui(IguiCursor::Default), CursorIcon::Default);
        assert_eq!(igui(IguiCursor::Pointer), CursorIcon::Pointer);
        assert_eq!(igui(IguiCursor::Text), CursorIcon::Text);
        assert_eq!(igui(IguiCursor::ColResize), CursorIcon::ColResize);
        assert_eq!(igui(IguiCursor::RowResize), CursorIcon::RowResize);
        assert_eq!(igui(IguiCursor::Grab), CursorIcon::Grab);
        assert_eq!(igui(IguiCursor::Grabbing), CursorIcon::Grabbing);
    }
}
