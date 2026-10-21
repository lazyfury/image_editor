//! 画廊缩略图缓存：配置目录 `thumbnails/<哈希>.png`，工作区文件更新过就作废。
//!
//! 生成缩略图要解码整个工作区（每层一张 PNG）再合成，4K 多图层文件不便宜，
//! 所以主页每次列出最近工作区都重算是不可接受的：保存工作区时顺手写一份缓存，
//! 之后主页只读这张小 PNG。
//!
//! 缓存文件名是**工作区路径的哈希**（路径本身不进文件名：有分隔符和长度限制），
//! 新鲜度用 mtime 判断 —— 工作区文件比缓存新就当作没有缓存、重新生成。这也顺带
//! 处理了外部工具改过文件的情况。缓存的清理不做：条目数上限是最近列表的
//! `MAX_RECENT`，每个几十 KB。

use std::path::{Path, PathBuf};

use crate::document::PixelBuffer;

use super::IoError;

/// 一个工作区的缩略图缓存路径。
pub fn cache_path(workspace: &Path) -> PathBuf {
    super::recent::config_dir()
        .join("thumbnails")
        .join(format!("{:016x}.png", hash_path(workspace)))
}

/// 读缓存；缓存不存在 / 比工作区旧 / 解不出来都返回 `None`。
pub fn load(workspace: &Path) -> Option<PixelBuffer> {
    load_at(&cache_path(workspace), workspace)
}

/// 写缓存（先建目录再落盘）。
pub fn store(workspace: &Path, pixels: &PixelBuffer) -> Result<(), IoError> {
    store_at(&cache_path(workspace), pixels)
}

/// [`load`] 的显式路径版本（测试注入用）。
pub fn load_at(cache: &Path, workspace: &Path) -> Option<PixelBuffer> {
    if !is_fresh(cache, workspace) {
        return None;
    }
    let bytes = std::fs::read(cache).ok()?;
    super::decode_png(&bytes).ok()
}

/// [`store`] 的显式路径版本（测试注入用）。
pub fn store_at(cache: &Path, pixels: &PixelBuffer) -> Result<(), IoError> {
    let bytes = super::encode_png(pixels)?;
    if let Some(parent) = cache.parent() {
        std::fs::create_dir_all(parent).map_err(IoError::File)?;
    }
    std::fs::write(cache, bytes).map_err(IoError::File)
}

/// 缓存不比源文件旧（保存流程是先写工作区、再写缩略图，所以它应该更新）。
fn is_fresh(cache: &Path, workspace: &Path) -> bool {
    let (Ok(cache_meta), Ok(source_meta)) =
        (std::fs::metadata(cache), std::fs::metadata(workspace))
    else {
        return false;
    };
    match (cache_meta.modified(), source_meta.modified()) {
        (Ok(cache_time), Ok(source_time)) => cache_time >= source_time,
        // 拿不到时间戳时宁可用缓存，也好过每次重算。
        _ => true,
    }
}

/// FNV-1a 64：只要跨进程稳定，不需要抗碰撞（碰撞最坏是预览图错，且路径哈希
/// 之后缓存会被更大 mtime 的工作区覆盖）。
fn hash_path(path: &Path) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Color;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "image_editor_thumb_{}_{n}_{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn a_stored_thumbnail_round_trips() {
        let dir = temp_dir("round_trip");
        let workspace = dir.join("w.igui");
        std::fs::write(&workspace, b"x").unwrap();
        let cache = dir.join("thumb.png");
        let pixels = PixelBuffer::filled(3, 2, Color::RED);

        store_at(&cache, &pixels).expect("store");
        assert_eq!(load_at(&cache, &workspace), Some(pixels));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 工作区被重新保存（mtime 更新）后缓存失效，避免画廊显示旧内容。
    #[test]
    fn a_newer_workspace_invalidates_the_cache() {
        let dir = temp_dir("stale");
        let workspace = dir.join("w.igui");
        std::fs::write(&workspace, b"old").unwrap();
        let cache = dir.join("thumb.png");
        store_at(&cache, &PixelBuffer::filled(2, 2, Color::RED)).expect("store");
        assert!(load_at(&cache, &workspace).is_some());

        // 明确把工作区写成更新的时间戳（同秒内两次写的时间戳可能相等）。
        let bytes = std::fs::read(&workspace).unwrap();
        std::fs::write(&workspace, bytes).unwrap();
        let future = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
        std::fs::File::options()
            .write(true)
            .open(&workspace)
            .unwrap()
            .set_modified(future)
            .unwrap();
        assert_eq!(load_at(&cache, &workspace), None, "工作区更新后不吃旧缓存");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_cache_reads_as_none() {
        let dir = temp_dir("missing");
        let workspace = dir.join("w.igui");
        std::fs::write(&workspace, b"x").unwrap();
        assert_eq!(load_at(&dir.join("nope.png"), &workspace), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 缓存文件名只由路径决定：同一路径两次算出的文件相同，不同路径不同。
    #[test]
    fn the_cache_name_is_derived_from_the_path() {
        let a = cache_path(Path::new("/tmp/a.igui"));
        assert_eq!(a, cache_path(Path::new("/tmp/a.igui")));
        assert_ne!(a, cache_path(Path::new("/tmp/b.igui")));
        assert_eq!(a.extension().and_then(|e| e.to_str()), Some("png"));
    }
}
