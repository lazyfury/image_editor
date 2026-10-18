//! 文件读写：`路径 <-> PNG 字节`，外加默认命名 / 图层名规则。

use std::path::Path;

use crate::document::PixelBuffer;

use super::{decode_png, encode_png, IoError};

/// 把像素写成 PNG 文件。
pub fn write_png(path: &Path, pixels: &PixelBuffer) -> Result<(), IoError> {
    let bytes = encode_png(pixels)?;
    std::fs::write(path, bytes).map_err(IoError::File)
}

/// 从 PNG 文件读出像素。
pub fn read_png(path: &Path) -> Result<PixelBuffer, IoError> {
    let bytes = std::fs::read(path).map_err(IoError::File)?;
    decode_png(&bytes)
}

/// 导入图层的名字：用文件名（去掉扩展名）；没有名字就用 `导入`。
pub fn file_label(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("导入")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Color;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// 测试用的唯一临时路径（进程 id + 原子计数，避免并行测试互相覆盖）。
    fn temp_path(tag: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "image_editor_{}_{}_{n}.png",
            std::process::id(),
            tag
        ))
    }

    #[test]
    fn pixels_round_trip_through_a_file() {
        let path = temp_path("roundtrip");
        let pixels = PixelBuffer::filled(2, 2, Color::new(4, 5, 6, 7));
        write_png(&path, &pixels).expect("write");
        let read = read_png(&path).expect("read");
        assert_eq!(read.get_pixel(1, 1), Color::new(4, 5, 6, 7));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reading_a_missing_file_is_an_io_error() {
        let path = temp_path("missing");
        assert!(matches!(read_png(&path), Err(IoError::File(_))));
    }

    #[test]
    fn the_layer_label_comes_from_the_file_stem() {
        assert_eq!(file_label(Path::new("/tmp/photo.png")), "photo");
        assert_eq!(file_label(Path::new("/")), "导入");
    }
}
