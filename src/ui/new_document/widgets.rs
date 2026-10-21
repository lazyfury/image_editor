//! 表单里的几种小控件：步进行、预设按钮、背景色块（透明色用棋盘格预览）。

use igui_components::{Button, Component, Flex, NodeRef, Text};
use igui_core::{Color as UiColor, Edges, Rect, Size, Vec2};
use igui_render::PaintContext;
use igui_theme::{radius, space, TextSize, Theme};
use igui_ui::{Align, MouseFilter, SurfaceStyle};

use super::NewDocumentSpec;
use crate::canvas::{CHECKER_DARK, CHECKER_LIGHT};
use crate::document::Color;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// 一行「标签  [−]  数值  [+]」。
pub(super) fn stepper_row(
    theme: &'static dyn Theme,
    label: &'static str,
    value_ref: &NodeRef,
    choices: Rc<RefCell<NewDocumentSpec>>,
    dirty: Rc<Cell<bool>>,
    less: fn(&mut NewDocumentSpec),
    more: fn(&mut NewDocumentSpec),
) -> impl Component {
    Flex::row()
        .gap(space::SM)
        .padding(Edges::ZERO)
        .align(Align::Center)
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::small(label, theme))
        .child(step_button(
            theme,
            "−",
            choices.clone(),
            dirty.clone(),
            less,
        ))
        .child(Text::small("", theme).min_size(72.0, 0.0).ref_(value_ref))
        .child(step_button(theme, "+", choices.clone(), dirty, more))
}

fn step_button(
    theme: &'static dyn Theme,
    label: &'static str,
    choices: Rc<RefCell<NewDocumentSpec>>,
    dirty: Rc<Cell<bool>>,
    apply: fn(&mut NewDocumentSpec),
) -> Button {
    Button::secondary(label, theme).on_click(move |_tree, _id| {
        apply(&mut choices.borrow_mut());
        dirty.set(true);
    })
}

pub(super) fn preset_button(
    theme: &'static dyn Theme,
    width: u32,
    height: u32,
    choices: Rc<RefCell<NewDocumentSpec>>,
    dirty: Rc<Cell<bool>>,
    slot: &NodeRef,
) -> impl Component {
    Button::secondary(format!("{width}×{height}"), theme)
        .font_size(TextSize::Small.px())
        .on_click(move |_tree, _id| {
            let mut spec = choices.borrow_mut();
            spec.width = width;
            spec.height = height;
            dirty.set(true);
        })
        .ref_(slot)
}

/// 一个背景色块：点击选中，选中的描一圈前景边。透明色用棋盘格预览。
pub(super) fn background_swatch(
    theme: &'static dyn Theme,
    _name: &'static str,
    color: Color,
    choices: Rc<RefCell<NewDocumentSpec>>,
    dirty: Rc<Cell<bool>>,
    slot: &NodeRef,
) -> impl Component {
    let ui = to_ui(color);
    let transparent = color.a == 0;
    let selected = choices.clone();
    let clicked = choices;
    Flex::new()
        .padding(Edges::ZERO)
        .min_size(28.0, 28.0)
        .on_click(move |_tree, _id| {
            clicked.borrow_mut().background = color;
            dirty.set(true);
        })
        .dynamic_background(move |interact| {
            let is_selected = selected.borrow().background == color;
            let style = SurfaceStyle::new(ui).radius(radius::SM);
            if is_selected || interact.hovered {
                style.border(theme.palette().foreground)
            } else {
                style.border(theme.palette().border)
            }
        })
        // 棋盘格画在色块前景（内缩 1px 给边框让位），跟画布的透明预览一致。
        .foreground(move |ctx, rect, _| {
            if transparent {
                paint_checkerboard(ctx, rect);
            }
        })
        .ref_(slot)
}

/// 棋盘格格子边长（逻辑像素）。
const CHECKER_CELL: f32 = 6.0;

/// 在 `rect` 里画透明棋盘格（内缩 1px，露出边框）。
fn paint_checkerboard(ctx: &mut PaintContext, rect: Rect) {
    let inner = Rect::from_min_size(
        Vec2::new(rect.left() + 1.0, rect.top() + 1.0),
        Size::new(
            (rect.size.width - 2.0).max(0.0),
            (rect.size.height - 2.0).max(0.0),
        ),
    );
    if inner.size.width <= 0.0 || inner.size.height <= 0.0 {
        return;
    }
    let cols = (inner.size.width / CHECKER_CELL).ceil() as i32;
    let rows = (inner.size.height / CHECKER_CELL).ceil() as i32;
    for row in 0..rows {
        for col in 0..cols {
            let x = inner.left() + col as f32 * CHECKER_CELL;
            let y = inner.top() + row as f32 * CHECKER_CELL;
            let w = CHECKER_CELL.min(inner.right() - x);
            let h = CHECKER_CELL.min(inner.bottom() - y);
            if w <= 0.0 || h <= 0.0 {
                continue;
            }
            let color = if (row + col) % 2 == 0 {
                CHECKER_LIGHT
            } else {
                CHECKER_DARK
            };
            ctx.fill_rect(
                Rect::from_min_size(Vec2::new(x, y), Size::new(w, h)),
                to_ui(color),
            );
        }
    }
}

/// 文档色（8 位 RGBA）-> UI 色（0..1）。
pub(super) fn to_ui(color: Color) -> UiColor {
    UiColor::new(
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
        color.a as f32 / 255.0,
    )
}
