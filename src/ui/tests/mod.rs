//! 视图级测试：点真实控件、读真实绘制命令（不截图）。
//!
//! 夹具（`viewport` / `click*` / `label_text` / `paint_commands` / `paint_a_stroke`）
//! 放在这里；各功能组一个文件，见下面的 `mod`。点一下有三种粒度，别混：
//!
//! - [`click`]：直接喂 `SceneTree`（不走视图事件路由，最快）；
//! - `click_event`：走 `EditorView::event`（菜单 / 覆盖层参与路由）；
//! - [`click_at`]：走 `EditorView::event` 并再 `update()` 一帧。

use super::*;
use crate::canvas::DOCUMENT_TEXTURE;
use crate::document::{Color, PixelBuffer, Point};
use crate::renderer::sample_pixel;
use crate::tools::GradientKind;
use igui_core::{InputEvent, Key, NodeId, PointerButton, Size, Vec2, ViewportSize};
use igui_render::{DrawCommand, PaintContext};

mod brush;
mod canvas;
mod chrome;
mod fill;
mod gradient;
mod history;
mod io;
mod layers;
mod menus;
mod panels;
mod pen;
mod selection;
mod toolbar;
mod tools;

/// 一个切到画笔、选项栏已排好版的新视图。
fn brush_view() -> EditorView {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    view.layout(viewport());
    view
}

/// 切到某个标签页（等同点标签：写共享状态后 `update` + `layout`）。
fn select_tab(view: &mut EditorView, tab: SidebarTab) {
    view.layout.active_tab.set(tab);
    view.update();
    view.layout(viewport());
}

fn frame_texts(view: &EditorView) -> Vec<String> {
    paint_commands(view)
        .iter()
        .filter_map(|command| match command {
            DrawCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn viewport() -> ViewportSize {
    ViewportSize::new(Size::new(1280.0, 800.0))
}

fn click(view: &mut EditorView, position: Vec2) {
    igui_ui::handle_input(
        &mut view.tree,
        &InputEvent::PointerDown {
            position,
            button: PointerButton::Left,
        },
    );
    igui_ui::handle_input(
        &mut view.tree,
        &InputEvent::PointerUp {
            position,
            button: PointerButton::Left,
        },
    );
}

fn label_text(view: &EditorView, id: NodeId) -> String {
    igui_ui::content(&view.tree, id)
        .and_then(|content| content.as_text())
        .map(str::to_string)
        .expect("expected a label")
}

fn paint_commands(view: &EditorView) -> Vec<DrawCommand> {
    let mut ctx = PaintContext::new();
    view.paint(&mut ctx);
    ctx.into_draw_list().into_commands()
}

/// 用画笔在画布中央画一笔（屏幕坐标 -> 文档坐标由视图换算）。
fn paint_a_stroke(view: &mut EditorView) {
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    let point = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));
    view.event(&InputEvent::PointerDown {
        position: point,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove {
        position: point + Vec2::new(40.0, 0.0),
    });
    view.event(&InputEvent::PointerUp {
        position: point + Vec2::new(40.0, 0.0),
        button: PointerButton::Left,
    });
    view.update();
}

/// 在屏幕坐标 `position` 处点一下（走真实事件，含 `update`）。
fn click_at(view: &mut EditorView, position: Vec2) {
    for event in [
        InputEvent::PointerDown {
            position,
            button: PointerButton::Left,
        },
        InputEvent::PointerUp {
            position,
            button: PointerButton::Left,
        },
    ] {
        view.event(&event);
    }
    view.update();
}
