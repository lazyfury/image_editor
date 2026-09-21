//! 随包字体：把 `assets/fonts/` 里的字体指给 quill 的后端。
//!
//! quill 的 `SystemFont` 按 `QUILL_FONT` 环境变量 + 各平台候选列表找字体
//! （见 `draw_backend_wgpu::font::system`）。项目把随包的字体放在
//! `assets/fonts/`，启动时若用户没有显式设置 `QUILL_FONT`，就在这里指向它；
//! 找不到则什么都不做，后端回落到系统字体 / 点阵字体。
//!
//! 字体文件本身**不提交 git**（见 `.gitignore` 与 `AGENTS.md`）。

use std::path::PathBuf;

/// 随包字体相对仓库根的路径。改字体只需替换这个文件。
pub const BUNDLED_FONT: &str = "assets/fonts/jinghua-laosong-v3.0.ttf";

/// 设置 `QUILL_FONT`（仅当用户未设置且字体存在）。返回实际使用的路径。
pub fn install() -> Option<PathBuf> {
    if std::env::var_os("QUILL_FONT").is_some() {
        return None;
    }
    let path = locate()?;
    std::env::set_var("QUILL_FONT", &path);
    Some(path)
}

/// 依次在 crate 根（开发时）、macOS `.app` 的 `Resources/`、可执行文件旁、当前
/// 目录查找字体。
fn locate() -> Option<PathBuf> {
    let mut roots = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR"))];
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
    {
        // macOS `.app`：二进制在 `Contents/MacOS/`，随包资源在
        // `Contents/Resources/`（`package-macos.sh` 把字体拷到这里）。
        roots.push(dir.join("..").join("Resources"));
        roots.push(dir);
    }
    roots.push(PathBuf::from("."));
    roots
        .into_iter()
        .map(|root| root.join(BUNDLED_FONT))
        .find(|path| path.is_file())
}
