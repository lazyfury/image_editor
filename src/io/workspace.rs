//! 工作区文件格式：把一个 [`Document`]（含图层像素）序列化成自包含二进制文件。
//!
//! 格式（小端，version 1）：
//!
//! ```text
//! magic  "IGUIWS01"          # 8 字节（magic + 版本号）
//! u32    width, height
//! [u8;4] background
//! string document_name       # u32 长度 + UTF-8 字节
//! i32    active_layer_index  # -1 = 无
//! u32    layer_count
//! 每个图层：
//!   string name
//!   u8     visible
//!   f32    opacity
//!   u8     blend_mode
//!   i32    position.x, position.y
//!   u32    pixel_width, pixel_height
//!   u32    png_len; [u8] png   # RGBA8，复用 PNG 编解码
//! ```
//!
//! 图层像素用 PNG 编码，文件紧凑且复用现成的 [`encode_png`] / [`decode_png`]。
//! 解码遇到坏 magic / 版本 / 截断 / 非法字段都返回 [`IoError::Invalid`]。

use std::path::Path;

use crate::document::{Color, Document, Layer};

use super::{decode_png, encode_png, IoError};

/// 文件扩展名（不带点）。
pub const EXTENSION: &str = "igui";
const MAGIC: &[u8; 8] = b"IGUIWS01";

/// 只读文件头得到的摘要（画廊用，不解析图层像素）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSummary {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub layers: u32,
}

/// 把文档编码成工作区字节。
pub fn encode_workspace(document: &Document) -> Result<Vec<u8>, IoError> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&document.width.to_le_bytes());
    out.extend_from_slice(&document.height.to_le_bytes());
    out.extend_from_slice(&document.background.to_rgba8());
    write_string(&mut out, &document.name);
    let active = document
        .active_layer
        .and_then(|id| document.layers.iter().position(|layer| layer.id == id))
        .map_or(-1i32, |index| index as i32);
    out.extend_from_slice(&active.to_le_bytes());
    out.extend_from_slice(&(document.layers.len() as u32).to_le_bytes());
    for layer in &document.layers {
        write_string(&mut out, &layer.name);
        out.push(u8::from(layer.visible));
        out.extend_from_slice(&layer.opacity.to_le_bytes());
        out.push(blend_code(layer.blend_mode));
        out.extend_from_slice(&layer.position.x.to_le_bytes());
        out.extend_from_slice(&layer.position.y.to_le_bytes());
        out.extend_from_slice(&layer.pixels.width.to_le_bytes());
        out.extend_from_slice(&layer.pixels.height.to_le_bytes());
        let png = encode_png(&layer.pixels)?;
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&png);
    }
    Ok(out)
}

/// 从工作区字节解出文档。
pub fn decode_workspace(bytes: &[u8]) -> Result<Document, IoError> {
    let mut reader = Reader::new(bytes);
    if reader.take(8)? != MAGIC {
        return Err(IoError::Invalid("不是工作区文件（magic 不匹配）".into()));
    }
    let width = reader.u32()?;
    let height = reader.u32()?;
    let background = Color::from_rgba8(reader.take(4)?.try_into().expect("4 bytes"));
    let name = reader.string()?;
    let active_index = reader.i32()?;
    let layer_count = reader.u32()?;

    let mut layers = Vec::with_capacity(layer_count as usize);
    for _ in 0..layer_count {
        let layer_name = reader.string()?;
        let visible = reader.u8()? != 0;
        let opacity = f32::from_le_bytes(reader.take(4)?.try_into().expect("4 bytes"));
        let blend_mode = decode_blend(reader.u8()?)?;
        let position = crate::document::Point::new(reader.i32()?, reader.i32()?);
        let pixel_width = reader.u32()?;
        let pixel_height = reader.u32()?;
        let png_len = reader.u32()? as usize;
        let png = reader.take(png_len)?;
        let pixels = decode_png(png)?;
        if pixels.width != pixel_width || pixels.height != pixel_height {
            return Err(IoError::Invalid("图层像素尺寸与头不一致".into()));
        }
        let mut layer = Layer::new(layer_name, pixels);
        layer.visible = visible;
        layer.opacity = opacity.clamp(0.0, 1.0);
        layer.blend_mode = blend_mode;
        layer.position = position;
        layers.push(layer);
    }

    let mut document = Document::with_background(name, width, height, background);
    document.active_layer = (active_index >= 0)
        .then(|| layers.get(active_index as usize).map(|layer| layer.id))
        .flatten();
    document.layers = layers;
    Ok(document)
}

/// 只读文件头，得到名字 / 尺寸 / 图层数（画廊用）。
pub fn read_workspace_summary(path: &Path) -> Result<WorkspaceSummary, IoError> {
    let bytes = std::fs::read(path).map_err(IoError::File)?;
    let mut reader = Reader::new(&bytes);
    if reader.take(8)? != MAGIC {
        return Err(IoError::Invalid("不是工作区文件（magic 不匹配）".into()));
    }
    let width = reader.u32()?;
    let height = reader.u32()?;
    let _background = reader.take(4)?;
    let name = reader.string()?;
    let _active = reader.i32()?;
    let layers = reader.u32()?;
    Ok(WorkspaceSummary {
        name,
        width,
        height,
        layers,
    })
}

/// 把文档写成工作区文件。
pub fn write_workspace(path: &Path, document: &Document) -> Result<(), IoError> {
    let bytes = encode_workspace(document)?;
    std::fs::write(path, bytes).map_err(IoError::File)
}

/// 从工作区文件读出一个文档。
pub fn read_workspace(path: &Path) -> Result<Document, IoError> {
    let bytes = std::fs::read(path).map_err(IoError::File)?;
    decode_workspace(&bytes)
}

fn write_string(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

fn blend_code(mode: crate::document::BlendMode) -> u8 {
    match mode {
        crate::document::BlendMode::Normal => 0,
    }
}

fn decode_blend(code: u8) -> Result<crate::document::BlendMode, IoError> {
    match code {
        0 => Ok(crate::document::BlendMode::Normal),
        other => Err(IoError::Invalid(format!("未知的混合模式 {other}"))),
    }
}

/// 一个带边界检查的小端读取器。
struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], IoError> {
        let end = self
            .cursor
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| IoError::Invalid("工作区文件被截断".into()))?;
        let slice = &self.bytes[self.cursor..end];
        self.cursor = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, IoError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, IoError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    fn i32(&mut self) -> Result<i32, IoError> {
        Ok(i32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    fn string(&mut self) -> Result<String, IoError> {
        let len = self.u32()? as usize;
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| IoError::Invalid("文本不是 UTF-8".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{BlendMode, PixelBuffer, Point};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_path(tag: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "image_editor_ws_{}_{n}_{tag}.{}",
            std::process::id(),
            EXTENSION
        ))
    }

    /// 两层、不同属性 / 位置的文档。
    fn sample() -> Document {
        let mut document = Document::with_background("我的工作区", 8, 6, Color::WHITE);
        let mut top = Layer::new("顶层", PixelBuffer::filled(3, 2, Color::RED));
        top.visible = false;
        top.opacity = 0.5;
        top.position = Point::new(-2, 3);
        top.pixels.set_pixel(1, 1, Color::new(1, 2, 3, 4));
        document.layers.push(top);
        document.active_layer = Some(document.layers[1].id);
        document
    }

    #[test]
    fn a_document_round_trips_through_the_format() {
        let original = sample();
        let bytes = encode_workspace(&original).expect("encode");
        let decoded = decode_workspace(&bytes).expect("decode");

        assert_eq!(decoded.name, original.name);
        assert_eq!((decoded.width, decoded.height), (8, 6));
        assert_eq!(decoded.background, Color::WHITE);
        assert_eq!(decoded.layers.len(), 2);
        assert_eq!(
            decoded
                .active_layer
                .and_then(|id| decoded.layers.iter().position(|l| l.id == id)),
            Some(1)
        );

        let top = &decoded.layers[1];
        assert_eq!(top.name, "顶层");
        assert!(!top.visible);
        assert!((top.opacity - 0.5).abs() < 1e-6);
        assert_eq!(top.blend_mode, BlendMode::Normal);
        assert_eq!(top.position, Point::new(-2, 3));
        assert_eq!((top.pixels.width, top.pixels.height), (3, 2));
        assert_eq!(top.pixels.get_pixel(1, 1), Color::new(1, 2, 3, 4));
    }

    #[test]
    fn the_format_round_trips_through_a_file() {
        let path = temp_path("roundtrip");
        let original = sample();
        write_workspace(&path, &original).expect("write");
        let decoded = read_workspace(&path).expect("read");
        assert_eq!(decoded.name, "我的工作区");
        assert_eq!(decoded.layers.len(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_summary_reads_only_the_header() {
        let path = temp_path("summary");
        write_workspace(&path, &sample()).expect("write");
        let summary = read_workspace_summary(&path).expect("summary");
        assert_eq!(summary.name, "我的工作区");
        assert_eq!((summary.width, summary.height), (8, 6));
        assert_eq!(summary.layers, 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_bad_magic_is_rejected() {
        assert!(matches!(
            decode_workspace(b"NOTAWORKSPACEFILE"),
            Err(IoError::Invalid(_))
        ));
    }

    #[test]
    fn a_truncated_file_is_rejected() {
        let bytes = encode_workspace(&sample()).expect("encode");
        assert!(matches!(
            decode_workspace(&bytes[..bytes.len() / 2]),
            Err(IoError::Invalid(_))
        ));
    }
}
