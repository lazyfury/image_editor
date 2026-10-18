//! 其它 Unix（Linux / BSD）：调 `zenity`，退回 `kdialog` 子进程。
//!
//! 子进程方案不碰 D-Bus / GTK，最省事；两个都装不上就返回 `None`（不弹窗）。
//! 本仓库只在 macOS 上验证过，这份实现**未在本机验证**。

use std::path::PathBuf;
use std::process::Command;

use super::{DialogOptions, FileDialog};

/// Unix 实现（无状态）。
#[derive(Default)]
pub struct NativeFileDialog;

impl FileDialog for NativeFileDialog {
    fn pick_open(&self, options: &DialogOptions) -> Option<PathBuf> {
        run(options, false)
    }

    fn pick_save(&self, options: &DialogOptions) -> Option<PathBuf> {
        run(options, true)
    }
}

fn run(options: &DialogOptions, save: bool) -> Option<PathBuf> {
    zenity(options, save).or_else(|| kdialog(options, save))
}

/// `名称 | *.ext *.ext` 形式的过滤器（zenity / kdialog 都吃这个）。
fn filters(options: &DialogOptions) -> Vec<String> {
    options
        .filters
        .iter()
        .map(|filter| {
            let patterns = filter
                .extensions
                .iter()
                .map(|extension| format!("*.{extension}"))
                .collect::<Vec<_>>()
                .join(" ");
            format!("{} | {}", filter.name, patterns)
        })
        .collect()
}

/// `--filename` / 起始路径参数（另存为带上默认文件名）。
fn start_arg(options: &DialogOptions, save: bool) -> Option<String> {
    let directory = options.start_dir.as_ref()?.display().to_string();
    let directory = format!("{}/", directory.trim_end_matches('/'));
    Some(match (save, options.default_name.as_deref()) {
        (true, Some(name)) => format!("{directory}{name}"),
        _ => directory,
    })
}

fn zenity(options: &DialogOptions, save: bool) -> Option<PathBuf> {
    let mut command = Command::new("zenity");
    command.arg("--file-selection");
    if save {
        command.arg("--save").arg("--confirm-overwrite");
    }
    if !options.title.is_empty() {
        command.arg(format!("--title={}", options.title));
    }
    for filter in filters(options) {
        command.arg(format!("--file-filter={filter}"));
    }
    if let Some(start) = start_arg(options, save) {
        command.arg(format!("--filename={start}"));
    }
    output(command)
}

fn kdialog(options: &DialogOptions, save: bool) -> Option<PathBuf> {
    let mut command = Command::new("kdialog");
    command.arg(if save {
        "--getsavefilename"
    } else {
        "--getopenfilename"
    });
    let start = options
        .start_dir
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| ".".to_string());
    command.arg(start);
    let filter = filters(options).join("\n");
    if !filter.is_empty() {
        command.arg(filter);
    }
    if !options.title.is_empty() {
        command.arg("--title").arg(&options.title);
    }
    output(command)
}

/// 跑命令，把 stdout 当路径（空 / 非零退出都算取消）。
fn output(mut command: Command) -> Option<PathBuf> {
    let result = command.output().ok()?;
    if !result.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&result.stdout);
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(PathBuf::from(text))
    }
}
