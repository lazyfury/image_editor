//! 模拟窗口的三个对话框：**完整的工具属性**（设置）、关于、快捷键。
//!
//! 都建在 [`crate::ui::window::Window`] 上（和「新建文档」/「未保存确认」同一套外壳：
//! 标题栏可拖、四条边 / 角可缩放、右上关闭；Esc / 点窗口外 = 关闭，输入全吞掉）。
//! 宿主同一时刻只开一个（[`DialogWindow`]），有它时编辑器收不到输入。
//!
//! 为什么要有「完整属性」窗口：工具选项栏只有一行，宽了就塞不下（画笔那些
//! `−` / `+` 已经在抢地方）。选项栏保持**紧凑子集**，末尾一个「全部属性…」打开
//! 这里 —— 同一批共享状态、同一批 [`Intent`](crate::ui::Intent)，只是排成竖列、
//! 每行带说明。
//!
//! 数据方向：
//!
//! - **显示**：宿主每帧把 [`SettingsSnapshot`] 交给 [`DialogWindow::sync`]，窗口只写
//!   文本；开关（像素 / 方形 / 对称 / 连续）直接读共享 `Cell`，和选项栏是同一份；
//! - **修改**：按钮只塞意图（[`Intent::Brush`](crate::ui::Intent) 等），由
//!   `EditorView::update` 统一应用 —— 和选项栏完全一条路。
//!
//! 文件分工：`mod.rs` = 宿主用的外壳枚举 + 快照 + 共享开关；
//! `settings.rs` = 完整属性；`about.rs` = 版本 / 字体 / 文档信息；
//! `shortcuts.rs` = 快捷键表（菜单表 + 工具键 + 其它键，同一批数据）。

use std::cell::Cell;
use std::rc::Rc;

use igui_core::{EventResult, InputEvent, ViewportSize};
use igui_render::PaintContext;
use igui_ui::TextMeasurer;

use crate::tools::Symmetry;

mod about;
mod settings;
mod shortcuts;

pub use about::{AboutSnapshot, AboutWindow};
pub use settings::{SettingsSnapshot, SettingsWindow};
pub use shortcuts::ShortcutsWindow;

/// 快捷键表（测试 / 自检按它核对覆盖面）。
pub fn shortcut_rows() -> Vec<(String, String)> {
    shortcuts::rows()
}

/// 打开哪个对话框（[`Intent::Dialog`](crate::ui::Intent::Dialog) 带的就是它）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    /// 完整的工具属性（选项栏塞不下的那些）。
    Settings,
    /// 版本 / 字体 / 当前文档。
    About,
    /// 一张快捷键表。
    Shortcuts,
}

/// 三个对话框之一；宿主 `Option` 着一个，有它时输入全交给它。
pub enum DialogWindow {
    Settings(SettingsWindow),
    About(AboutWindow),
    Shortcuts(ShortcutsWindow),
}

impl DialogWindow {
    /// 建一个对话框窗口。
    pub fn new(
        kind: DialogKind,
        theme: &'static dyn igui_theme::Theme,
        settings: SettingsWindowInput,
        about: AboutSnapshot,
    ) -> Self {
        match kind {
            DialogKind::Settings => {
                Self::Settings(SettingsWindow::new(theme, settings.cells, settings.intents))
            }
            DialogKind::About => Self::About(AboutWindow::new(theme, about)),
            DialogKind::Shortcuts => Self::Shortcuts(ShortcutsWindow::new(theme)),
        }
    }

    /// 用后端真实字体度量排版。
    pub fn set_text_measurer(&mut self, measurer: Rc<dyn TextMeasurer>) {
        match self {
            Self::Settings(window) => window.set_text_measurer(measurer),
            Self::About(window) => window.set_text_measurer(measurer),
            Self::Shortcuts(window) => window.set_text_measurer(measurer),
        }
    }

    pub fn event(&mut self, event: &InputEvent) -> EventResult {
        match self {
            Self::Settings(window) => window.event(event),
            Self::About(window) => window.event(event),
            Self::Shortcuts(window) => window.event(event),
        }
    }

    pub fn layout(&mut self, viewport: ViewportSize) {
        match self {
            Self::Settings(window) => window.layout(viewport),
            Self::About(window) => window.layout(viewport),
            Self::Shortcuts(window) => window.layout(viewport),
        }
    }

    pub fn paint(&self, ctx: &mut PaintContext) {
        match self {
            Self::Settings(window) => window.paint(ctx),
            Self::About(window) => window.paint(ctx),
            Self::Shortcuts(window) => window.paint(ctx),
        }
    }

    /// 关闭请求（Esc / 关闭按钮 / 点窗口外）；设置窗口的「关闭」按钮也算。
    pub fn take_close(&self) -> bool {
        match self {
            Self::Settings(window) => window.take_close(),
            Self::About(window) => window.take_close(),
            Self::Shortcuts(window) => window.take_close(),
        }
    }

    /// 把最新的属性值写进标签（只有设置窗口有内容要同步）。
    pub fn sync(&mut self, snapshot: &SettingsSnapshot) {
        if let Self::Settings(window) = self {
            window.sync(snapshot);
        }
    }
}

/// 建设置窗口需要的东西：共享开关 + 意图队列（和选项栏同一批）。
pub struct SettingsWindowInput {
    pub cells: OptionCells,
    pub intents: super::Intents,
}

/// 选项栏 / 设置窗口共用的四个开关（同一份状态，两处显示）。
#[derive(Clone)]
pub struct OptionCells {
    pub pixel: Rc<Cell<bool>>,
    pub square: Rc<Cell<bool>>,
    pub symmetry: Rc<Cell<Symmetry>>,
    pub contiguous: Rc<Cell<bool>>,
}

impl super::EditorView {
    /// 选项栏与属性窗口共用的四个开关（克隆 `Rc`，两处读同一份）。
    pub fn option_cells(&self) -> OptionCells {
        OptionCells {
            pixel: self.options.pixel_mode.clone(),
            square: self.options.square_mode.clone(),
            symmetry: self.options.symmetry_mode.clone(),
            contiguous: self.options.bucket_contiguous.clone(),
        }
    }

    /// 属性窗口每帧要显示的值。
    pub fn settings_snapshot(&self) -> SettingsSnapshot {
        SettingsSnapshot {
            preset: crate::tools::brush::preset::label(
                crate::tools::brush::preset::PRESETS,
                &self.brush,
            ),
            size: self.brush.size,
            opacity: self.brush.opacity,
            hardness: self.brush.hardness,
            smooth: self.brush.smooth,
            spacing: self.brush.spacing_ratio,
            tolerance: self.paint_bucket.tolerance,
            symmetry: self.options.symmetry_mode.get(),
            record: self.records_timeline(),
        }
    }

    /// 「记录整段编辑过程」开着没有（属性窗口的开关显示用它）。
    pub fn records_timeline(&self) -> bool {
        self.timeline_len().is_some()
    }
}
