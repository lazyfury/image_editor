//! 「关于」对话框：版本 / igui tag / 字体 / 当前文档 / 能力规模。
//!
//! 内容在打开时按 [`AboutSnapshot`] 生成一次：这是一个「看一眼就关」的窗口，
//! 文档之后改了不重画（要再看得重新打开）。数字都现取，不写死 —— `tools` /
//! `menu_items` / `placeholders` 分别来自工具表与 `ui::menu::menu_spec`，
//! 所以占位项减一个，这里就少一个（`todo.md` §1.ae 的能力梳理靠它兜住）。

use std::cell::Cell;
use std::rc::Rc;

use igui_components::{Button, Component, Flex, Text};
use igui_core::{Edges, EventResult, InputEvent, Vec2, ViewportSize};
use igui_render::PaintContext;
use igui_scene::SceneTree;
use igui_theme::{space, Theme, Tone};
use igui_ui::{MouseFilter, TextMeasurer};

use super::super::window::{Window, FOOTER_BUTTON_MIN_WIDTH};

/// 窗口尺寸 / 最小尺寸。
const WINDOW_SIZE: Vec2 = Vec2::new(460.0, 420.0);
const WINDOW_MIN: Vec2 = Vec2::new(320.0, 280.0);

/// 「关于」窗口要显示的内容（打开时取一次）。
#[derive(Debug, Clone, Default)]
pub struct AboutSnapshot {
    /// `CARGO_PKG_VERSION`。
    pub version: String,
    /// 依赖的 igui release tag（`Cargo.toml` 里固定的那个）。
    pub igui: String,
    /// 当前字体（随包老宋体 / 系统字体 / 点阵）。
    pub font: String,
    /// 当前文档（宽, 高）；没有工作区时 `None`。
    pub document: Option<(u32, u32)>,
    /// 图层数。
    pub layers: usize,
    /// 工具数 / 菜单项数 / 还没实现的占位项数。
    pub tools: usize,
    pub menu_items: usize,
    pub placeholders: usize,
}

impl AboutSnapshot {
    /// 一行行文字（单测直接比对这里，不去读绘制命令）。
    pub fn lines(&self) -> Vec<(String, String)> {
        let document = match self.document {
            Some((width, height)) => format!("{width} × {height}"),
            None => "没有打开的文档".to_string(),
        };
        vec![
            ("版本".into(), self.version.clone()),
            ("UI 栈".into(), format!("igui · {}", self.igui)),
            ("字体".into(), self.font.clone()),
            ("文档".into(), document),
            ("图层".into(), format!("{} 个", self.layers)),
            ("工具".into(), format!("{} 个", self.tools)),
            (
                "菜单".into(),
                format!("{} 项（{} 项待实现）", self.menu_items, self.placeholders),
            ),
        ]
    }
}

/// 「关于」窗口。
pub struct AboutWindow {
    window: Window,
    closed: Rc<Cell<bool>>,
}

impl AboutWindow {
    pub fn new(theme: &'static dyn Theme, snapshot: AboutSnapshot) -> Self {
        let closed: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        let footer_close = closed.clone();
        let content = {
            let mut column = Flex::column()
                .gap(space::MD)
                .padding(Edges::ZERO)
                .mouse_filter(MouseFilter::Ignore)
                .child(
                    Text::small("image_editor", theme)
                        .color(theme.palette().foreground)
                        .mouse_filter(MouseFilter::Ignore),
                )
                .child(
                    Text::caption(
                        "用 igui UI 栈自己画的 2D 图像编辑器（没有 egui / DOM）",
                        theme,
                    )
                    .tone(Tone::Muted),
                );
            for (label, value) in snapshot.lines() {
                column = column.child(
                    Flex::row()
                        .gap(space::SM)
                        .padding(Edges::ZERO)
                        .mouse_filter(MouseFilter::Ignore)
                        .child(
                            Text::caption(label, theme)
                                .tone(Tone::Muted)
                                .min_size(64.0, 0.0)
                                .mouse_filter(MouseFilter::Ignore),
                        )
                        .child(Text::small(value, theme).mouse_filter(MouseFilter::Ignore)),
                );
            }
            column
        };
        let window = Window::new(theme, "关于 image_editor", WINDOW_SIZE, WINDOW_MIN, {
            move |tree: &mut SceneTree, content_node, footer| {
                content.build(tree, content_node);
                Button::secondary("关闭", theme)
                    .min_size(FOOTER_BUTTON_MIN_WIDTH, 0.0)
                    .on_click(move |_tree, _id| footer_close.set(true))
                    .build(tree, footer);
            }
        });
        Self { window, closed }
    }

    pub fn set_text_measurer(&mut self, measurer: Rc<dyn TextMeasurer>) {
        self.window.set_text_measurer(measurer);
    }

    pub fn event(&mut self, event: &InputEvent) -> EventResult {
        self.window.event(event)
    }

    pub fn layout(&mut self, viewport: ViewportSize) {
        self.window.layout(viewport);
    }

    pub fn paint(&self, ctx: &mut PaintContext) {
        self.window.paint(ctx);
    }

    pub fn take_close(&self) -> bool {
        self.closed.replace(false) || self.window.take_close_request()
    }
}
