//! 工作区文件格式：把一个 [`Document`]（含图层像素）序列化成自包含二进制文件。
//!
//! 格式（小端，version 2；`IGUIWS01` 仍然可读）：
//!
//! ```text
//! magic  "IGUIWS02"          # 8 字节（magic + 版本号）
//! u32    width, height
//! [u8;4] background
//! string document_name       # u32 长度 + UTF-8 字节
//! i32    active_index        # -1 = 无
//! u8     active_mask         # 1 = 当前编辑的是蒙版
//! u32    node_count
//! 每个节点（前序 DFS，从下到上）：
//!   string name
//!   u8     kind              # 0 = 栅格，1 = 组
//!   i32    parent_index      # -1 = 根
//!   u8     visible
//!   f32    opacity
//!   u8     blend_mode
//!   i32    position.x, position.y
//!   u8     expanded
//!   u8     has_mask; [u32 len + png]   # 灰度 RGBA8
//!   栅格层才有的像素：u32 w, h; u32 png_len; [u8] png
//! ```
//!
//! 组没有像素；蒙版尺寸是图层缓冲（组是文档）大小。v1 是扁平无蒙版格式，
//! 解码时每个节点 `parent = None`、`expanded = true`。
//!
//! 图层像素用 PNG 编码，文件紧凑且复用现成的 [`encode_png`] / [`decode_png`]。
//! 解码遇到坏 magic / 版本 / 截断 / 非法字段都返回 [`IoError::Invalid`]。

use std::path::Path;

use crate::document::{Color, Document, Layer};

use super::bytes::{write_string, Reader};
use super::{decode_png, encode_png, IoError};

/// 文件扩展名（不带点）。
pub const EXTENSION: &str = "igui";
const MAGIC: &[u8; 8] = b"IGUIWS02";
const MAGIC_V1: &[u8; 8] = b"IGUIWS01";

/// 只读文件头得到的摘要（画廊用，不解析图层像素）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSummary {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub layers: u32,
}

/// 把文档编码成工作区字节（v2）。
pub fn encode_workspace(document: &Document) -> Result<Vec<u8>, IoError> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&document.width.to_le_bytes());
    out.extend_from_slice(&document.height.to_le_bytes());
    out.extend_from_slice(&document.background.to_rgba8());
    write_string(&mut out, &document.name);
    let active = document
        .active_layer
        .and_then(|id| document.layer_index(id))
        .map_or(-1i32, |index| index as i32);
    out.extend_from_slice(&active.to_le_bytes());
    out.push(u8::from(document.active_mask));
    out.extend_from_slice(&(document.layers.len() as u32).to_le_bytes());
    for layer in &document.layers {
        write_string(&mut out, &layer.name);
        out.push(match layer.kind {
            crate::document::LayerKind::Raster => 0,
            crate::document::LayerKind::Group => 1,
        });
        let parent = layer
            .parent
            .and_then(|id| document.layer_index(id))
            .map_or(-1i32, |index| index as i32);
        out.extend_from_slice(&parent.to_le_bytes());
        out.push(u8::from(layer.visible));
        out.extend_from_slice(&layer.opacity.to_le_bytes());
        out.push(blend_code(layer.blend_mode));
        out.extend_from_slice(&layer.position.x.to_le_bytes());
        out.extend_from_slice(&layer.position.y.to_le_bytes());
        out.push(u8::from(layer.expanded));
        match &layer.mask {
            Some(mask) => {
                out.push(1);
                let png = encode_png(&mask.pixels)?;
                out.push(u8::from(mask.enabled));
                out.extend_from_slice(&(png.len() as u32).to_le_bytes());
                out.extend_from_slice(&png);
            }
            None => out.push(0),
        }
        if layer.is_group() {
            continue;
        }
        out.extend_from_slice(&layer.pixels.width.to_le_bytes());
        out.extend_from_slice(&layer.pixels.height.to_le_bytes());
        let png = encode_png(&layer.pixels)?;
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&png);
    }
    Ok(out)
}

/// 从工作区字节解出文档（v2；`IGUIWS01` 走兼容路径）。
pub fn decode_workspace(bytes: &[u8]) -> Result<Document, IoError> {
    let mut reader = Reader::new(bytes);
    let magic = reader.take(8)?;
    if magic == MAGIC_V1 {
        return decode_v1(&mut reader);
    }
    if magic != MAGIC {
        return Err(IoError::Invalid("不是工作区文件（magic 不匹配）".into()));
    }
    let width = reader.u32()?;
    let height = reader.u32()?;
    let background = Color::from_rgba8(reader.take(4)?.try_into().expect("4 bytes"));
    let name = reader.string()?;
    let active_index = reader.i32()?;
    let active_mask = reader.u8()? != 0;
    let node_count = reader.u32()?;

    let mut layers = Vec::with_capacity(node_count as usize);
    let mut parents = Vec::with_capacity(node_count as usize);
    for _ in 0..node_count {
        let layer_name = reader.string()?;
        let kind = reader.u8()?;
        let parent_index = reader.i32()?;
        let visible = reader.u8()? != 0;
        let opacity = f32::from_le_bytes(reader.take(4)?.try_into().expect("4 bytes"));
        let blend_mode = decode_blend(reader.u8()?)?;
        let position = crate::document::Point::new(reader.i32()?, reader.i32()?);
        let expanded = reader.u8()? != 0;
        let has_mask = reader.u8()? != 0;
        let mask = if has_mask {
            let enabled = reader.u8()? != 0;
            let png_len = reader.u32()? as usize;
            let png = reader.take(png_len)?;
            Some(crate::document::LayerMask {
                pixels: decode_png(png)?,
                enabled,
            })
        } else {
            None
        };
        let mut layer = match kind {
            0 => {
                let pixel_width = reader.u32()?;
                let pixel_height = reader.u32()?;
                let png_len = reader.u32()? as usize;
                let png = reader.take(png_len)?;
                let pixels = decode_png(png)?;
                if pixels.width != pixel_width || pixels.height != pixel_height {
                    return Err(IoError::Invalid("图层像素尺寸与头不一致".into()));
                }
                Layer::new(layer_name, pixels)
            }
            1 => Layer::group(layer_name),
            other => return Err(IoError::Invalid(format!("未知的节点类型 {other}"))),
        };
        if let Some(mask) = mask {
            if !layer.is_group()
                && (mask.pixels.width != layer.pixels.width
                    || mask.pixels.height != layer.pixels.height)
            {
                return Err(IoError::Invalid("蒙版尺寸与图层缓冲不一致".into()));
            }
            layer.mask = Some(mask);
        }
        layer.visible = visible;
        layer.opacity = opacity.clamp(0.0, 1.0);
        layer.blend_mode = blend_mode;
        layer.position = if layer.is_group() {
            crate::document::Point::ZERO
        } else {
            position
        };
        layer.expanded = expanded;
        layers.push(layer);
        parents.push(parent_index);
    }

    // 第二遍：校验 parent 并挂上（parent 必须指向前面出现过的组）。
    let mut document = Document::with_background(name, width, height, background);
    for index in 0..layers.len() {
        let parent_index = parents[index];
        if parent_index >= 0 {
            let parent_index = parent_index as usize;
            let parent = layers
                .get(parent_index)
                .ok_or_else(|| IoError::Invalid("parent 下标越界".into()))?;
            if !parent.is_group() {
                return Err(IoError::Invalid("parent 不是组".into()));
            }
            if parent_index >= index {
                return Err(IoError::Invalid("parent 必须在子节点之前".into()));
            }
            layers[index].parent = Some(layers[parent_index].id);
        }
    }
    document.active_layer = (active_index >= 0)
        .then(|| layers.get(active_index as usize).map(|layer| layer.id))
        .flatten();
    document.layers = layers;
    let active_has_mask = document
        .active_layer
        .and_then(|id| document.layer(id))
        .is_some_and(|layer| !layer.is_group() && layer.mask.is_some());
    document.active_mask = active_mask && active_has_mask;
    document.validate().map_err(IoError::Invalid)?;
    Ok(document)
}

/// v1（扁平、无分组 / 蒙版）的兼容解码。
fn decode_v1(reader: &mut Reader<'_>) -> Result<Document, IoError> {
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
    let magic = reader.take(8)?;
    let v2 = magic == MAGIC;
    if !v2 && magic != MAGIC_V1 {
        return Err(IoError::Invalid("不是工作区文件（magic 不匹配）".into()));
    }
    let width = reader.u32()?;
    let height = reader.u32()?;
    let _background = reader.take(4)?;
    let name = reader.string()?;
    let _active = reader.i32()?;
    if v2 {
        let _active_mask = reader.u8()?;
    }
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
    fn a_tree_with_groups_masks_and_collapse_round_trips() {
        let mut original = Document::with_background("树", 8, 6, Color::WHITE);
        let group = original.add_group("组");
        let inner = original.add_layer("组内");
        original.set_layer_visible(inner, false);
        original.set_expanded(group, false);
        let mut mask = crate::document::LayerMask::opaque(8, 6);
        mask.pixels.set_pixel(0, 0, Color::BLACK);
        mask.enabled = false;
        assert!(original.set_layer_mask(group, mask));
        original.set_active_mask(false);
        let active = original.active_layer;

        let bytes = encode_workspace(&original).expect("encode");
        let decoded = decode_workspace(&bytes).expect("decode");
        decoded.validate().expect("树不变量");

        // 解码会换新 id，按名字找。
        let group = decoded
            .layers
            .iter()
            .find(|layer| layer.name == "组")
            .expect("组还在");
        assert!(group.is_group());
        assert!(!group.expanded, "折叠状态写进文件");
        let mask = group.mask.as_ref().expect("组蒙版");
        assert!(!mask.enabled);
        assert_eq!(mask.pixels.get_pixel(0, 0), Color::BLACK);
        let inner = decoded
            .layers
            .iter()
            .find(|layer| layer.name == "组内")
            .expect("子节点");
        assert_eq!(inner.parent, Some(group.id));
        assert!(!inner.visible);
        assert_eq!(
            decoded.layer_index(inner.id),
            Some(active_index_after(&original, active))
        );
        assert!(!decoded.active_mask);

        fn active_index_after(
            document: &Document,
            active: Option<crate::document::LayerId>,
        ) -> usize {
            active.and_then(|id| document.layer_index(id)).unwrap_or(0)
        }
    }

    #[test]
    fn active_mask_only_survives_when_the_active_node_has_one() {
        let mut document = Document::with_background("d", 2, 2, Color::WHITE);
        let id = document.layers[0].id;
        let mut mask = crate::document::LayerMask::opaque(2, 2);
        mask.enabled = true;
        assert!(document.set_layer_mask(id, mask));
        assert!(document.set_active_mask(true));
        let decoded =
            decode_workspace(&encode_workspace(&document).expect("encode")).expect("decode");
        assert!(decoded.active_mask, "有蒙版时编辑目标跟着存");

        // 删掉蒙版后 active_mask 不会留在文件里。
        assert!(document.remove_layer_mask(id).is_some());
        let decoded =
            decode_workspace(&encode_workspace(&document).expect("encode")).expect("decode");
        assert!(!decoded.active_mask);
    }

    #[test]
    fn a_v1_file_still_decodes_as_a_flat_tree() {
        use super::super::bytes::write_string;

        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC_V1);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&Color::WHITE.to_rgba8());
        write_string(&mut bytes, "老文件");
        bytes.extend_from_slice(&0i32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        write_string(&mut bytes, "背景");
        bytes.push(1);
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&0i32.to_le_bytes());
        bytes.extend_from_slice(&0i32.to_le_bytes());
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        let png = encode_png(&PixelBuffer::filled(4, 3, Color::RED)).expect("png");
        bytes.extend_from_slice(&(png.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&png);

        let document = decode_workspace(&bytes).expect("v1 decode");
        assert_eq!(document.name, "老文件");
        assert_eq!(document.layers.len(), 1);
        assert_eq!(document.layers[0].parent, None);
        assert!(document.layers[0].expanded);
        assert!(document.layers[0].mask.is_none());
        assert_eq!(document.layers[0].pixels.get_pixel(0, 0), Color::RED);
        assert_eq!(document.active_layer, Some(document.layers[0].id));
    }

    #[test]
    fn a_bad_parent_is_rejected() {
        let mut document = Document::with_background("d", 2, 2, Color::WHITE);
        let id = document.layers[0].id;
        let group = document.add_group("组");
        document.move_into(id, group);
        let mut bytes = encode_workspace(&document).expect("encode");
        // 把子节点的 parent 改成它自己（前序里 parent 必须在前面）。
        let child_index = document.layer_index(id).unwrap();
        let parent_offset = node_field_offset(&bytes, child_index).expect("父字段位置");
        bytes[parent_offset..parent_offset + 4]
            .copy_from_slice(&(child_index as i32).to_le_bytes());
        assert!(matches!(decode_workspace(&bytes), Err(IoError::Invalid(_))));
    }

    fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
        bytes
            .get(at..at + 4)
            .and_then(|slice| slice.try_into().ok())
            .map(u32::from_le_bytes)
    }

    /// 第 `node` 个节点的 `parent_index` 字段在文件里的字节偏移（测试用）。
    fn node_field_offset(bytes: &[u8], node: usize) -> Option<usize> {
        let mut cursor = 8;
        cursor += 8; // width, height
        cursor += 4; // background
        let name_len = read_u32(bytes, cursor)? as usize;
        cursor += 4 + name_len;
        cursor += 4; // active index
        cursor += 1; // active mask
        let count = read_u32(bytes, cursor)? as usize;
        cursor += 4;
        if node >= count {
            return None;
        }
        for index in 0..count {
            let name_len = read_u32(bytes, cursor)? as usize;
            cursor += 4 + name_len;
            let kind = *bytes.get(cursor)?;
            cursor += 1;
            if index == node {
                return Some(cursor);
            }
            cursor += 4; // parent
            cursor += 1; // visible
            cursor += 4; // opacity
            cursor += 1; // blend
            cursor += 8; // position
            cursor += 1; // expanded
            let has_mask = *bytes.get(cursor)?;
            cursor += 1;
            if has_mask == 1 {
                cursor += 1; // enabled
                let len = read_u32(bytes, cursor)? as usize;
                cursor += 4 + len;
            }
            if kind == 0 {
                cursor += 8; // pixel w, h
                let len = read_u32(bytes, cursor)? as usize;
                cursor += 4 + len;
            }
        }
        None
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
