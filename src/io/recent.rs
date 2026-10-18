//! 最近工作区：配置目录里的一个纯文本列表（每行一个绝对路径，最新在前）。
//!
//! 不引 `dirs`：配置目录按平台手算（macOS `Application Support`、Windows
//! `%APPDATA%`、其余走 `XDG_CONFIG_HOME` / `~/.config`）。读取时顺手滤掉已经
//! 不存在的路径，画廊就不会列已删除的文件。

use std::path::{Path, PathBuf};

use super::IoError;

/// 列表上限。
const MAX_RECENT: usize = 20;

/// 配置目录（不存在也不报错，写入时再 `create_dir_all`）。
pub fn config_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/igui-image-editor")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(home)
            .join("igui-image-editor")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".config"))
            .join("igui-image-editor")
    }
}

/// 最近工作区列表文件。
pub fn recent_path() -> PathBuf {
    config_dir().join("recent")
}

/// 读最近列表（滤掉不存在的路径）。
pub fn load_recent() -> Vec<PathBuf> {
    load_recent_at(&recent_path())
}

/// 把一个工作区路径记到列表最前（去重 + 截断 + 落盘）。
pub fn record_recent(path: &Path) -> Result<(), IoError> {
    record_recent_at(&recent_path(), path)
}

/// [`load_recent`] 的显式路径版本（测试注入用）。
pub fn load_recent_at(path: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|candidate| candidate.exists())
        .take(MAX_RECENT)
        .collect()
}

/// [`record_recent`] 的显式路径版本（测试注入用）。
pub fn record_recent_at(path: &Path, workspace: &Path) -> Result<(), IoError> {
    let mut list = load_recent_at(path);
    list.retain(|existing| existing != workspace);
    list.insert(0, workspace.to_path_buf());
    list.truncate(MAX_RECENT);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(IoError::File)?;
    }
    let text: String = list
        .iter()
        .map(|entry| format!("{}\n", entry.display()))
        .collect();
    std::fs::write(path, text).map_err(IoError::File)
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// 一个临时目录 + 里面的若干假工作区文件。
    fn temp_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "image_editor_recent_{}_{n}_{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, b"x").expect("touch");
        path
    }

    #[test]
    fn recording_puts_the_newest_first_and_dedups() {
        let dir = temp_dir("record");
        let list = dir.join("recent");
        let a = touch(&dir, "a.igui");
        let b = touch(&dir, "b.igui");

        record_recent_at(&list, &a).expect("record a");
        record_recent_at(&list, &b).expect("record b");
        assert_eq!(load_recent_at(&list), vec![b.clone(), a.clone()]);

        // 再记 a：去重并置顶。
        record_recent_at(&list, &a).expect("record a again");
        assert_eq!(load_recent_at(&list), vec![a.clone(), b.clone()]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_files_are_filtered_out_on_load() {
        let dir = temp_dir("missing");
        let list = dir.join("recent");
        let gone = dir.join("gone.igui");
        let kept = touch(&dir, "kept.igui");
        std::fs::write(&list, format!("{}\n{}\n", gone.display(), kept.display())).unwrap();
        assert_eq!(load_recent_at(&list), vec![kept]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_list_is_capped() {
        let dir = temp_dir("cap");
        let list = dir.join("recent");
        for index in 0..(MAX_RECENT + 5) {
            let path = touch(&dir, &format!("w{index}.igui"));
            record_recent_at(&list, &path).expect("record");
        }
        assert_eq!(load_recent_at(&list).len(), MAX_RECENT);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
