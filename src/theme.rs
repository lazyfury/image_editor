//! 编辑器自己的主题：实现 quill 的 [`Theme`] trait，在项目里自由覆盖任意 token。
//!
//! [`EditorTheme`] 包住设计系统的 [`DefaultTheme`]，只做两件事：
//! 1. 固定 **紧凑密度**（更小 padding、默认 mini 控件）；
//! 2. 提供一处编辑器专属的覆盖点（下面 `impl Theme` 里按需改 `surface` /
//!    `spacing` / `font_size` / 任意颜色）。
//!
//! 组件库读的是 trait，不认具体类型，所以这里加覆盖不用动 quill。主题以
//! `&'static dyn Theme` 形式交给视图，进程内只构造一次。

use std::sync::LazyLock;

use draw_core::Color;
use draw_theme::{DefaultTheme, Density, Mode, Palette, SurfaceLevel, Theme};

/// 编辑器主题。持有内置主题作为基底，覆盖点集中在 `impl Theme` 中。
pub struct EditorTheme {
    base: DefaultTheme,
}

impl EditorTheme {
    fn light() -> Self {
        Self {
            base: DefaultTheme::light().compact(),
        }
    }

    fn dark() -> Self {
        Self {
            base: DefaultTheme::dark().compact(),
        }
    }
}

impl Theme for EditorTheme {
    fn palette(&self) -> &Palette {
        self.base.palette()
    }

    fn mode(&self) -> Mode {
        self.base.mode()
    }

    fn density(&self) -> Density {
        self.base.density()
    }

    /// 编辑器专属：浮层（菜单 / 模态）沿用 raised 的平色，压低视觉噪音。
    /// 想换成独立配色，改这一处即可。
    fn surface(&self, level: SurfaceLevel) -> Color {
        match level {
            SurfaceLevel::Floating => self.base.surface(SurfaceLevel::Raised),
            other => self.base.surface(other),
        }
    }
}

static EDITOR_LIGHT: LazyLock<EditorTheme> = LazyLock::new(EditorTheme::light);
static EDITOR_DARK: LazyLock<EditorTheme> = LazyLock::new(EditorTheme::dark);

/// 编辑器主题（`light` 选浅色外观），以 `'static` trait 对象返回给视图。
pub fn editor_theme(light: bool) -> &'static dyn Theme {
    if light {
        &*EDITOR_LIGHT
    } else {
        &*EDITOR_DARK
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use draw_theme::{ControlSize, Space};

    #[test]
    fn the_editor_theme_is_compact_and_keeps_the_mode() {
        let dark = editor_theme(false);
        assert!(dark.is_dark());
        assert_eq!(dark.default_control(), ControlSize::Mini);
        // Compact scales spacing down from the documented 12px `MD` step.
        assert!(dark.spacing(Space::MD) < 12.0);
        assert!(editor_theme(true).mode().is_light());
    }
}
