//! 小端字节读写的小工具：`.igui`（工作区）与 `.igtl`（编辑时间线）共用。
//!
//! 只做边界检查，不解释格式；格式本身在各自的模块文档里。

use super::IoError;

/// 写一个带 `u32` 长度前缀的 UTF-8 字符串。
pub(super) fn write_string(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

/// 一个带边界检查的小端读取器。
pub(super) struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    pub(super) fn take(&mut self, len: usize) -> Result<&'a [u8], IoError> {
        let end = self
            .cursor
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| IoError::Invalid("文件被截断".into()))?;
        let slice = &self.bytes[self.cursor..end];
        self.cursor = end;
        Ok(slice)
    }

    pub(super) fn u8(&mut self) -> Result<u8, IoError> {
        Ok(self.take(1)?[0])
    }

    pub(super) fn u32(&mut self) -> Result<u32, IoError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    pub(super) fn u64(&mut self) -> Result<u64, IoError> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("8 bytes"),
        ))
    }

    pub(super) fn i32(&mut self) -> Result<i32, IoError> {
        Ok(i32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    pub(super) fn string(&mut self) -> Result<String, IoError> {
        let len = self.u32()? as usize;
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| IoError::Invalid("文本不是 UTF-8".into()))
    }
}
