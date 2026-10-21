//! 编辑过程时间线（`.igtl`）：记录每一步「什么时候、哪一层、哪块区域、变成什么」。
//!
//! 给「延时动画 / 过程回放」用。录制本身在 [`crate::document::Recorder`]（默认关，
//! 偏好里的 `record_timeline` 打开）；保存工作区时顺手写到同名 `.igtl`
//! （`x.igui` -> `x.igtl`），所以「全部保存」不需要单独一个按钮。
//!
//! 格式（小端，version 1）：
//!
//! ```text
//! magic  "IGTL0001"          # 8 字节
//! u32    step_count
//! 每步：
//!   u64   at_ms              # 相对录制开始
//!   string label             # u32 长度 + UTF-8
//!   u8    has_patch          # 0 / 1
//!   if has_patch:
//!     u64 layer_bits
//!     u32 x, y, width, height
//!     width*height × 4 bytes # RGBA8，行优先
//! ```
//!
//! 像素补丁不压缩：写盘只在保存工作区时发生一次，换来的是回放时零解码依赖。

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::document::{Color, LayerId, PixelRegion, RecordedStep, StepPatch};

use super::bytes::{write_string, Reader};
use super::IoError;

/// 文件扩展名（不带点）。
pub const EXTENSION: &str = "igtl";
const MAGIC: &[u8; 8] = b"IGTL0001";

/// 和工作区文件同名的 `.igtl`（`x.igui` -> `x.igtl`；没有扩展名就追加）。
pub fn timeline_path_for(workspace: &Path) -> PathBuf {
    workspace.with_extension(EXTENSION)
}

/// 把时间线编码成字节。
pub fn encode_timeline(steps: &[RecordedStep]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(steps.len() as u32).to_le_bytes());
    for step in steps {
        out.extend_from_slice(&(step.at.as_millis() as u64).to_le_bytes());
        write_string(&mut out, &step.label);
        match &step.patch {
            None => out.push(0),
            Some(patch) => {
                out.push(1);
                out.extend_from_slice(&patch.layer.to_bits().to_le_bytes());
                out.extend_from_slice(&patch.region.x.to_le_bytes());
                out.extend_from_slice(&patch.region.y.to_le_bytes());
                out.extend_from_slice(&patch.region.width.to_le_bytes());
                out.extend_from_slice(&patch.region.height.to_le_bytes());
                for color in &patch.pixels {
                    out.extend_from_slice(&color.to_rgba8());
                }
            }
        }
    }
    out
}

/// 解出时间线；坏 magic / 截断 / 像素数与区域不符都返回 [`IoError::Invalid`]。
pub fn decode_timeline(bytes: &[u8]) -> Result<Vec<RecordedStep>, IoError> {
    if bytes.len() < MAGIC.len() || &bytes[..MAGIC.len()] != MAGIC {
        return Err(IoError::Invalid("不是时间线文件".into()));
    }
    let mut reader = Reader::new(&bytes[MAGIC.len()..]);
    let count = reader.u32()? as usize;
    let mut steps = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        let at = Duration::from_millis(reader.u64()?);
        let label = reader.string()?;
        let patch = match reader.u8()? {
            0 => None,
            1 => {
                let layer = LayerId::from_bits(reader.u64()?);
                let region =
                    PixelRegion::new(reader.u32()?, reader.u32()?, reader.u32()?, reader.u32()?);
                let mut pixels = Vec::with_capacity(region.area());
                for _ in 0..region.area() {
                    let bytes = reader.take(4)?;
                    pixels.push(Color::new(bytes[0], bytes[1], bytes[2], bytes[3]));
                }
                Some(StepPatch {
                    layer,
                    region,
                    pixels,
                })
            }
            other => return Err(IoError::Invalid(format!("未知的补丁标记 {other}"))),
        };
        steps.push(RecordedStep { at, label, patch });
    }
    Ok(steps)
}

pub fn write_timeline(path: &Path, steps: &[RecordedStep]) -> Result<(), IoError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, encode_timeline(steps))?;
    Ok(())
}

pub fn read_timeline(path: &Path) -> Result<Vec<RecordedStep>, IoError> {
    decode_timeline(&std::fs::read(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<RecordedStep> {
        vec![
            RecordedStep {
                at: Duration::from_millis(0),
                label: "画笔".to_string(),
                patch: Some(StepPatch {
                    layer: LayerId::from_bits(7),
                    region: PixelRegion::new(2, 3, 2, 1),
                    pixels: vec![Color::RED, Color::WHITE],
                }),
            },
            RecordedStep {
                at: Duration::from_millis(250),
                label: "新建图层".to_string(),
                patch: None,
            },
        ]
    }

    #[test]
    fn a_timeline_round_trips_through_the_format() {
        let steps = sample();
        let decoded = decode_timeline(&encode_timeline(&steps)).expect("能解回来");
        assert_eq!(decoded, steps);
    }

    #[test]
    fn the_file_name_follows_the_workspace() {
        assert_eq!(
            timeline_path_for(Path::new("/tmp/a.igui")),
            PathBuf::from("/tmp/a.igtl")
        );
        assert_eq!(
            timeline_path_for(Path::new("/tmp/untitled")),
            PathBuf::from("/tmp/untitled.igtl")
        );
    }

    #[test]
    fn a_bad_magic_is_rejected() {
        assert!(decode_timeline(b"nope").is_err());
        assert!(decode_timeline(b"NOPE0001").is_err());
    }

    #[test]
    fn a_truncated_file_is_rejected() {
        let mut bytes = encode_timeline(&sample());
        bytes.truncate(bytes.len() - 3);
        assert!(decode_timeline(&bytes).is_err(), "缺像素要报错");
    }
}
