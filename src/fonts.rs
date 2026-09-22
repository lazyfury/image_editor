//! 随包字体：把 `assets/fonts/` 里的字体作为 quill 后端的默认 face。
//!
//! quill 的 [`FontConfig::default_face`](draw_font::FontConfig)（一个
//! [`FaceRef`]）让调用方直接把一个字体文件指给 `FontServer`：只读该文件的元数据、
//! **不扫描系统字体**；系统扫描推迟到真正需要 fallback（缺字 / 字体选择器）时。
//! 项目启动时用 [`bundled_face`] 拿到它，随包字体不需要再借道环境变量。
//!
//! 用户显式设置的 `QUILL_FONT` 仍然优先：`FontServer::load_with` 先看该环境
//! 变量，命中就不用 `default_face`。
//!
//! 字体文件本身**不提交 git**（见 `.gitignore` 与 `AGENTS.md`）。

use std::path::PathBuf;

use draw_font::FaceRef;

/// 随包字体相对仓库根的路径。改字体只需替换这个文件。
pub const BUNDLED_FONT: &str = "assets/fonts/jinghua-laosong-v3.0.ttf";

/// 定位随包字体并作为默认 face 返回；找不到则 `None`（后端回落到系统 /
/// 点阵字体）。
pub fn bundled_face() -> Option<FaceRef> {
    locate().map(|file| FaceRef::new(file, 0))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_face_points_at_the_bundled_font() {
        let Some(face) = bundled_face() else {
            // 字体未随仓库提供（见模块文档），没有可断言的对象。
            return;
        };
        assert!(face.file.ends_with(BUNDLED_FONT));
        assert_eq!(face.index, 0);
    }
}
