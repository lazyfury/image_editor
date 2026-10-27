//! 自检的各项检查，按功能分块。
//!
//! 每个文件只管一类功能，所有检查都读同一份录制帧 / 视图状态（不截图）；
//! [`super::check`] 只负责按顺序调用它们。

mod brush;
mod chrome;
mod clipboard;
mod fill;
mod layer;
mod pen;
mod pipeline;
mod rotation;
mod selection;
mod timeline;

pub(crate) use brush::{
    check_brush_cursor, check_brush_presets, check_brush_smooth, check_pixel_mode_edges,
    check_symmetry,
};
pub(crate) use chrome::{
    check_chrome_order, check_dialogs, check_native_chrome_order, check_options_bar_height,
    options_bar, palette, resizable_panels, view_menu,
};
pub(crate) use clipboard::check_clipboard;
pub(crate) use fill::{check_bucket, check_gradient};
pub(crate) use layer::check_rename;
pub(crate) use pen::check_pen;
pub(crate) use pipeline::{first_stroke, png_io, pointer_tools, undo_redo};
pub(crate) use rotation::check_rotation;
pub(crate) use selection::{check_magic_wand, check_selection_menu, check_selection_merge};
pub(crate) use timeline::check_timeline;
