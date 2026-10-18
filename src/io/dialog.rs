//! 原生文件对话框（**手写平台 FFI**，不引第三方对话框 crate）。
//!
//! 宿主 [`crate::app::application`] 持有一个 [`FileDialog`]；视图只发「要打开 /
//! 另存为」的意图，路径由这里选出来。按平台分三份实现：
//!
//! - macOS：AppKit 的 `NSOpenPanel` / `NSSavePanel`（`objc2` 已在依赖树里）；
//! - Windows：`comdlg32` 的 `GetOpenFileNameW` / `GetSaveFileNameW`（裸 FFI）；
//! - 其它 Unix（Linux/BSD）：`zenity` / `kdialog` 子进程。
//!
//! 对话框必须在**主线程**同步弹（AppKit 的 `runModal` 会跑嵌套模态循环），
//! 所以宿主在 `AppLogic::update` 里直接调用。
//!
//! 测试注入假对话框，永不真弹窗；[`DialogOptions`] 的构造与「保存还是另存为」
//! 的判断是纯逻辑，单独单测。

use std::path::PathBuf;

/// 一个文件类型过滤器：显示名 + 扩展名列表（不带点）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

impl FileFilter {
    /// 新建一个过滤器，例如 `FileFilter::new("工作区", &["igui"])`。
    pub fn new(name: impl Into<String>, extensions: &[&str]) -> Self {
        Self {
            name: name.into(),
            extensions: extensions.iter().map(|ext| (*ext).to_string()).collect(),
        }
    }
}

/// 一次对话框的参数。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DialogOptions {
    /// 标题栏文字。
    pub title: String,
    /// 文件类型过滤；空表示不过滤。
    pub filters: Vec<FileFilter>,
    /// 另存为时的默认文件名（含扩展名）。
    pub default_name: Option<String>,
    /// 起始目录；`None` 交给系统记住的上次位置。
    pub start_dir: Option<PathBuf>,
}

/// 原生文件对话框。宿主在启动时注入一个实现。
pub trait FileDialog {
    /// 选一个已存在的文件；取消返回 `None`。
    fn pick_open(&self, options: &DialogOptions) -> Option<PathBuf>;
    /// 选一个保存路径（可能覆盖已有文件）；取消返回 `None`。
    fn pick_save(&self, options: &DialogOptions) -> Option<PathBuf>;
}

#[cfg(all(unix, not(target_os = "macos")))]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(all(unix, not(target_os = "macos")))]
pub use linux::NativeFileDialog;
#[cfg(target_os = "macos")]
pub use macos::NativeFileDialog;
#[cfg(target_os = "windows")]
pub use windows::NativeFileDialog;

/// 其它平台（wasm 等）没有实现：永远返回 `None`。
#[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
#[derive(Default)]
pub struct NativeFileDialog;

#[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
impl FileDialog for NativeFileDialog {
    fn pick_open(&self, _options: &DialogOptions) -> Option<PathBuf> {
        None
    }

    fn pick_save(&self, _options: &DialogOptions) -> Option<PathBuf> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filter_lowercases_nothing_and_keeps_order() {
        let filter = FileFilter::new("图片", &["png", "jpg"]);
        assert_eq!(filter.extensions, vec!["png", "jpg"]);
    }
}
