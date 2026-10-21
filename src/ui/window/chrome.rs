//! 窗口的外观：三段结构（标题栏 / 内容区 / 页脚）+ 右下角缩放手柄。
//!
//! 只有形状与配色，不含任何状态：返回外框组件和两个 `NodeRef`（内容 / 页脚），
//! 由 [`Window::new`](super::Window::new) 挂到树上、把正文交给调用方。

use igui_components::{Component, Flex, NodeRef, Text};
use igui_core::{Edges, Vec2};
use igui_render::CornerRadii;
use igui_theme::space;
use igui_ui::{Align, InteractState, Justify, MouseFilter, SizeBasis, SurfaceStyle};
use std::cell::Cell;
use std::rc::Rc;

use super::*;

/// 建窗口外观：标题栏 + 内容区 + 页脚，外加右下角三道缩放手柄线。
///
/// 返回 `(外框, 内容区 NodeRef, 页脚 NodeRef)`；正文由调用方加进内容区。
pub(super) fn frame(
    theme: &'static dyn Theme,
    title: &str,
    size: Vec2,
    min_size: Vec2,
    close: Rc<Cell<bool>>,
) -> (Flex, NodeRef, NodeRef) {
    let palette = theme.palette();
    let content_ref = NodeRef::new();
    let footer_ref = NodeRef::new();

    // 标题栏：外框不设 padding，标题只留左边距，关闭按钮贴右，下沿一条细线。
    let header = Flex::row()
        .basis(SizeBasis::Px(TITLE_BAR_HEIGHT))
        .grow(0.0)
        .gap(0.0)
        .align(Align::Center)
        .padding(Edges::new(TITLE_PADDING_X, 0.0, 0.0, 0.0))
        .mouse_filter(MouseFilter::Ignore)
        // 标题栏比窗口底色浅一档，上沿跟着外框的圆角走（下沿是直角）。
        .surface(
            SurfaceStyle::new(theme.surface(HEADER_SURFACE)).corners(CornerRadii::new(
                WINDOW_RADIUS,
                WINDOW_RADIUS,
                0.0,
                0.0,
            )),
        )
        .foreground(move |ctx, rect, _| {
            // 标题栏下沿一条细线，把可拖区域画清楚。
            ctx.draw_line(
                Vec2::new(rect.left(), rect.bottom() - 0.5),
                Vec2::new(rect.right(), rect.bottom() - 0.5),
                1.0,
                palette.border_subtle,
            );
        })
        .child(
            Text::small(title, theme)
                .color(palette.foreground)
                .grow(1.0)
                // 标题不能抢指针，否则抓标题文字拖不动窗口。
                .mouse_filter(MouseFilter::Ignore),
        )
        .child(close_button(theme, close.clone()));

    // 内容区：撑满标题栏与页脚之间的空间。
    let content = Flex::column()
        .grow(1.0)
        .gap(space::LG)
        .padding(Edges::all(space::LG))
        .mouse_filter(MouseFilter::Ignore)
        .ref_(&content_ref);

    // 页脚：操作按钮右对齐，上沿一条细线。
    let footer = Flex::row()
        .grow(0.0)
        .shrink(0.0)
        .min_size(0.0, FOOTER_MIN_HEIGHT)
        .align(Align::Center)
        .justify(Justify::End)
        .gap(space::SM)
        .padding(Edges::symmetric(space::LG, space::MD))
        .mouse_filter(MouseFilter::Ignore)
        .foreground(move |ctx, rect, _| {
            ctx.draw_line(
                Vec2::new(rect.left(), rect.top() + 0.5),
                Vec2::new(rect.right(), rect.top() + 0.5),
                1.0,
                palette.border_subtle,
            );
        })
        .ref_(&footer_ref);

    let frame = Flex::column()
        .anchors(Edges::ZERO)
        .offsets(Edges::new(0.0, 0.0, size.x, size.y))
        .min_size(min_size.x, min_size.y)
        // `Flex` 默认 padding 16 / gap 8；外框必须显式清零，否则标题栏、
        // 页脚都会被缩进，和内容区的 padding 重复。
        .padding(Edges::ZERO)
        .gap(0.0)
        .surface(
            SurfaceStyle::new(theme.surface(WINDOW_SURFACE))
                .border(palette.border)
                .radius(WINDOW_RADIUS),
        )
        .mouse_filter(MouseFilter::Ignore)
        // 右下角的三道斜线：提示「这里可以拖动缩放」。
        .foreground(move |ctx, rect, _| {
            let corner = Vec2::new(rect.right() - GRIP_INSET, rect.bottom() - GRIP_INSET);
            for index in 1..=GRIP_LINES {
                let arm = GRIP_STEP * index as f32;
                ctx.draw_line(
                    Vec2::new(corner.x - arm, corner.y),
                    Vec2::new(corner.x, corner.y - arm),
                    1.0,
                    palette.subtle,
                );
            }
        })
        .child(header)
        .child(content)
        .child(footer);

    (frame, content_ref, footer_ref)
}

/// 右上角的关闭按钮：悬停变红，前景画一个叉。
fn close_button(theme: &'static dyn Theme, close: Rc<Cell<bool>>) -> impl Component {
    let foreground = theme.palette().foreground;
    let on_accent = theme.palette().on_accent;
    Flex::new()
        .min_size(CLOSE_BUTTON, CLOSE_BUTTON)
        .basis(SizeBasis::Px(CLOSE_BUTTON))
        .shrink(0.0)
        .align(Align::Center)
        .justify(Justify::Center)
        .mouse_filter(MouseFilter::Stop)
        .on_click(move |_tree, _id| close.set(true))
        .dynamic_background(move |state| {
            if hovered(state) {
                SurfaceStyle::new(CLOSE_HOVER)
            } else {
                SurfaceStyle::new(Color::TRANSPARENT)
            }
        })
        .foreground(move |ctx, rect, state| {
            let color = if hovered(state) {
                on_accent
            } else {
                foreground
            };
            let center = rect.center();
            let arm = 5.0;
            ctx.draw_line(
                center + Vec2::new(-arm, -arm),
                center + Vec2::new(arm, arm),
                1.5,
                color,
            );
            ctx.draw_line(
                center + Vec2::new(arm, -arm),
                center + Vec2::new(-arm, arm),
                1.5,
                color,
            );
        })
}

fn hovered(state: InteractState) -> bool {
    state.hovered || state.pressed
}
