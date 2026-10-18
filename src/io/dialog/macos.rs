//! macOS 原生文件对话框：AppKit 的 `NSOpenPanel` / `NSSavePanel`。
//!
//! `runModal` 会跑一个嵌套的模态事件循环，**必须在主线程**调用（宿主在
//! `AppLogic::update` 里就是主线程）。用 `MainThreadMarker` 拿到「我在主线程」
//! 的证明，拿不到就退回 `None`（不弹窗），避免在错误线程上崩。
//!
//! 文件类型过滤用 `setAllowedFileTypes`（扩展名字符串数组）。它是 AppKit 里
//! 被 `allowedContentTypes`（`UTType`）取代的旧 API，但仍在所有目标系统上可用，
//! 且不需要多引一个 `UTType` crate，所以这里显式 `allow(deprecated)`。

use std::path::PathBuf;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSModalResponseOK, NSOpenPanel, NSSavePanel};
use objc2_foundation::{NSArray, NSString, NSURL};

use super::{DialogOptions, FileDialog};

/// macOS 实现（无状态）。
#[derive(Default)]
pub struct NativeFileDialog;

impl FileDialog for NativeFileDialog {
    fn pick_open(&self, options: &DialogOptions) -> Option<PathBuf> {
        let mtm = MainThreadMarker::new()?;
        let panel = NSOpenPanel::openPanel(mtm);
        configure(&panel, options);
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowsMultipleSelection(false);
        run(&panel)
    }

    fn pick_save(&self, options: &DialogOptions) -> Option<PathBuf> {
        let mtm = MainThreadMarker::new()?;
        let panel = NSSavePanel::savePanel(mtm);
        configure(&panel, options);
        panel.setCanCreateDirectories(true);
        if let Some(name) = options.default_name.as_deref() {
            panel.setNameFieldStringValue(&NSString::from_str(name));
        }
        run(&panel)
    }
}

/// 弹模态面板；确认则返回选中的路径。
fn run(panel: &NSSavePanel) -> Option<PathBuf> {
    if panel.runModal() != NSModalResponseOK {
        return None;
    }
    let url = panel.URL()?;
    let path = url.path()?;
    Some(PathBuf::from(path.to_string()))
}

/// 把标题 / 起始目录 / 文件类型过滤写进面板（`NSOpenPanel` 继承 `NSSavePanel`）。
fn configure(panel: &NSSavePanel, options: &DialogOptions) {
    if !options.title.is_empty() {
        panel.setTitle(Some(&NSString::from_str(&options.title)));
    }
    if let Some(directory) = options.start_dir.as_deref() {
        let path = NSString::from_str(&directory.to_string_lossy());
        panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&path)));
    }
    let extensions: Vec<_> = options
        .filters
        .iter()
        .flat_map(|filter| filter.extensions.iter())
        .map(|extension| NSString::from_str(extension))
        .collect();
    if !extensions.is_empty() {
        #[allow(deprecated)]
        panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&extensions)));
    }
}
