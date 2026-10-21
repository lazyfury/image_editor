//! 「快捷键」对话框：一张两列的表。
//!
//! **不手抄**：菜单项的快捷键来自 `ui::menu::menu_spec`（那张表也是原生菜单栏的
//! 数据源），工具字母来自 `ui::TOOL_KEYS`，只有「宿主层」的键（保存 / 打开 /
//! 笔刷大小 / 删像素……）在这里列一次 —— 它们归 `app/application/shortcuts.rs`
//! 与 `ui::*_shortcut`，等 §1.ab 第 5 批把菜单键收进表里之后，这一列也会自动来。
//!
//! 行数固定（约 34 行），两列排得下，所以窗口里**不用滚动**，一屏全看见。

use std::cell::Cell;
use std::rc::Rc;

use igui_components::{Button, Component, Flex, Text};
use igui_core::{Edges, EventResult, InputEvent, Vec2, ViewportSize};
use igui_render::PaintContext;
use igui_scene::SceneTree;
use igui_theme::{space, Theme, Tone};
use igui_ui::{MouseFilter, TextMeasurer};

use super::super::window::{Window, FOOTER_BUTTON_MIN_WIDTH};
use crate::ui::menu::{menu_spec, MenuEntry, MenuState, MENUS};

/// 窗口尺寸 / 最小尺寸。
const WINDOW_SIZE: Vec2 = Vec2::new(720.0, 560.0);
const WINDOW_MIN: Vec2 = Vec2::new(480.0, 360.0);
/// 快捷键列的宽度（按键对齐）。
const KEY_WIDTH: f32 = 132.0;
/// 一行的高度。
const ROW_HEIGHT: f32 = 22.0;

/// 菜单表里每个菜单的「有文档 / 全都可点」状态：只看快捷键，不看可用性。
fn table_state() -> MenuState {
    MenuState {
        has_document: true,
        can_undo: true,
        can_redo: true,
        has_selection: true,
        can_paste: true,
        checkerboard: false,
    }
}

/// 宿主层的键（不在菜单表里）。
const HOST_KEYS: [(&str, &str); 8] = [
    ("Ctrl/Cmd+S", "保存工作区"),
    ("Shift+Ctrl/Cmd+S", "另存为"),
    ("Ctrl/Cmd+O", "打开工作区"),
    ("Ctrl/Cmd+E", "导出 PNG"),
    ("Ctrl/Cmd+N", "新建文档"),
    ("[  ]", "笔刷大小 −/+"),
    (",  .", "笔刷不透明度 −/+"),
    ("Delete", "删除选区内容"),
];

/// 整张表（`(按键, 动作)`，已去重、按来源排序：工具 → 菜单 → 宿主键）。
pub fn rows() -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = crate::ui::TOOL_KEYS
        .iter()
        .map(|(key, tool)| (key.to_string(), tool.label().to_string()))
        .collect();
    for (index, title) in MENUS.iter().enumerate() {
        for entry in menu_spec(index, table_state()) {
            let MenuEntry::Item {
                label,
                shortcut: Some(shortcut),
                ..
            } = entry
            else {
                continue;
            };
            rows.push((shortcut.to_string(), format!("{title} · {label}")));
        }
    }
    rows.extend(
        HOST_KEYS
            .iter()
            .map(|(key, label)| ((*key).to_string(), (*label).to_string())),
    );
    let mut seen = Vec::new();
    rows.retain(|(key, label)| {
        let key = format!("{key}|{label}");
        if seen.contains(&key) {
            false
        } else {
            seen.push(key);
            true
        }
    });
    rows
}

/// 「快捷键」窗口。
pub struct ShortcutsWindow {
    window: Window,
    closed: Rc<Cell<bool>>,
}

impl ShortcutsWindow {
    pub fn new(theme: &'static dyn Theme) -> Self {
        let closed: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        let footer_close = closed.clone();
        let rows = rows();
        let half = rows.len().div_ceil(2);
        let mut columns = Flex::row()
            .gap(space::XXXL)
            .padding(Edges::ZERO)
            .mouse_filter(MouseFilter::Ignore);
        for column_rows in [&rows[..half], &rows[half.min(rows.len())..]] {
            let mut column = Flex::column()
                .gap(space::XXXS)
                .padding(Edges::ZERO)
                .mouse_filter(MouseFilter::Ignore);
            for (key, label) in column_rows {
                column = column.child(
                    Flex::row()
                        .align(igui_ui::Align::Center)
                        .gap(space::SM)
                        .padding(Edges::ZERO)
                        .basis(igui_ui::SizeBasis::Px(ROW_HEIGHT))
                        .mouse_filter(MouseFilter::Ignore)
                        .child(
                            Text::caption(key, theme)
                                .tone(Tone::Muted)
                                .min_size(KEY_WIDTH, 0.0)
                                .mouse_filter(MouseFilter::Ignore),
                        )
                        .child(Text::small(label, theme).mouse_filter(MouseFilter::Ignore)),
                );
            }
            columns = columns.child(column);
        }
        let content = Flex::column()
            .gap(space::MD)
            .padding(Edges::ZERO)
            .mouse_filter(MouseFilter::Ignore)
            .child(
                Text::caption("按住 Ctrl/Cmd 的组合键；工具键在画布里直接按。", theme)
                    .tone(Tone::Muted),
            )
            .child(columns);

        let window = Window::new(theme, "快捷键", WINDOW_SIZE, WINDOW_MIN, {
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
