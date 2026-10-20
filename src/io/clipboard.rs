//! 系统剪贴板：把一张图写进去 / 从里面读一张图（跨 App 复制粘贴）。
//!
//! macOS 走 `NSPasteboard` 的 `public.png` 类型；其它平台暂时是空实现
//! （写返回 [`IoError::Invalid`]，读返回 `None`），跨平台实现以后按
//! `io/dialog.rs` 的手写 FFI 方式补。
//!
//! 进程内的「复制 / 粘贴」不依赖它（[`crate::tools::PixelClipboard`]），这里只负责
//! 和其它 App 交换：复制时顺手写一份 PNG、粘贴时进程内没有就去系统剪贴板找。
//!
//! 注意：这些调用会**动用户的系统剪贴板**，所以单元测试里只测纯函数；
//! macOS 的往返验证是一个 `--ignored` 的手动探针。

use crate::document::PixelBuffer;

use super::IoError;

/// 把一块像素写进系统剪贴板（macOS：`public.png`）。
pub fn write_image(pixels: &PixelBuffer) -> Result<(), IoError> {
    platform::write_image(pixels)
}

/// 从系统剪贴板读一张图；里面没有 PNG 图片时返回 `None`。
pub fn read_image() -> Option<PixelBuffer> {
    platform::read_image()
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG};
    use objc2_foundation::NSData;

    use super::{IoError, PixelBuffer};
    use crate::io::{decode_png, encode_png};

    pub(super) fn write_image(pixels: &PixelBuffer) -> Result<(), IoError> {
        let png = encode_png(pixels)?;
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        let data = NSData::with_bytes(&png);
        // SAFETY: `NSPasteboardTypePNG` 是 AppKit 提供的静态 UTI 字符串常量。
        let written = pasteboard.setData_forType(Some(&data), unsafe { NSPasteboardTypePNG });
        if written {
            Ok(())
        } else {
            Err(IoError::Invalid("系统剪贴板写入失败".to_string()))
        }
    }

    pub(super) fn read_image() -> Option<PixelBuffer> {
        let pasteboard = NSPasteboard::generalPasteboard();
        // SAFETY: 同上，静态 UTI 常量。
        let data = pasteboard.dataForType(unsafe { NSPasteboardTypePNG })?;
        let bytes = data.to_vec();
        decode_png(&bytes).ok()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::document::{Color, PixelBuffer};

        /// 手动跑：`cargo test -- --ignored clipboard_round_trip`。
        /// 它会**覆盖**系统剪贴板，所以默认不跑。
        #[test]
        #[ignore = "动系统剪贴板，手动跑"]
        fn clipboard_round_trip() {
            let mut pixels = PixelBuffer::filled(4, 3, Color::RED);
            pixels.set_pixel(1, 1, Color::rgb(0, 0, 255));
            write_image(&pixels).expect("写剪贴板");
            let read = read_image().expect("读回剪贴板");
            assert_eq!((read.width, read.height), (4, 3));
            assert_eq!(read.get_pixel(1, 1), Color::rgb(0, 0, 255));
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::{IoError, PixelBuffer};

    pub(super) fn write_image(_pixels: &PixelBuffer) -> Result<(), IoError> {
        Err(IoError::Invalid("这个平台还不支持写系统剪贴板".to_string()))
    }

    pub(super) fn read_image() -> Option<PixelBuffer> {
        None
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::document::Color;

        #[test]
        fn an_unsupported_platform_reports_it_instead_of_panicking() {
            assert!(write_image(&PixelBuffer::filled(2, 2, Color::RED)).is_err());
            assert!(read_image().is_none());
        }
    }
}
