//! 预览面板（Navigator）：把整张文档缩到侧栏里，并标出主画布当前看到的那一块。
//!
//! 图像来自显示用的合成结果（`take_texture_upload` 顺带缩放 256px 上传），
//! 所以不需要再合成一遍；主画布的可见范围由相机 + 画布矩形换算成**文档坐标的
//! 四个角**，旋转视图时也是精确的四边形而不是包围盒。
//!
//! 面板底部有一排「线性 / 最近邻」按钮，选择预览纹理的采样方式（存进偏好）。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use igui_components::{Button, Component, Flex};
use igui_core::{Color, Edges, Rect, Size, Vec2};
use igui_render::{Paint, TextureId};
use igui_theme::{radius, space, TextSize, Theme};
use igui_ui::{Align, InteractState, MouseFilter, SurfaceStyle};

use crate::app::state::AppState;
use crate::canvas::PreviewFilter;

use super::{Intent, Intents};

/// 预览纹理 id：文档=1、主页画廊从 2 起、图层缩略图从 100000 起，取中间一段。
pub const PREVIEW_TEXTURE: TextureId = TextureId::new(64);
/// 预览纹理的最长边（像素）。面板宽 ~250，256 足够，缩放成本可忽略。
pub const PREVIEW_MAX_EDGE: u32 = 256;
/// 两帧预览之间的最小间隔（毫秒）。
pub const PREVIEW_INTERVAL_MS: u64 = 200;
/// 预览图与面板边缘留白。
const PREVIEW_PADDING: f32 = 6.0;

/// 主画布可见范围的**文档坐标**四角（左上、右上、右下、左下）。
pub type PreviewViewport = Rc<Cell<Option<[Vec2; 4]>>>;

/// 预览面板内容：自绘的预览区域 + 底部采样方式按钮。
pub fn preview_panel(
    theme: &'static dyn Theme,
    state: Rc<RefCell<AppState>>,
    viewport: PreviewViewport,
    filter: Rc<Cell<PreviewFilter>>,
    intents: Intents,
) -> impl Component {
    Flex::column()
        .grow(1.0)
        .gap(space::XS)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(preview_view(theme, state, viewport))
        .child(filter_row(theme, filter, intents))
}

/// 预览图本身（只画，不接收输入）。
fn preview_view(
    theme: &'static dyn Theme,
    state: Rc<RefCell<AppState>>,
    viewport: PreviewViewport,
) -> impl Component {
    Flex::new()
        .grow(1.0)
        .min_size(0.0, 100.0)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .foreground(move |ctx, rect, _state| {
            let document = {
                let state = state.borrow();
                Size::new(state.document.width as f32, state.document.height as f32)
            };
            let fit = fit_rect(rect, document, PREVIEW_PADDING);
            ctx.draw_image(PREVIEW_TEXTURE, fit, None, Paint::default());

            // 可见范围：把文档坐标的四角线性映射到预览矩形里。
            if let Some(corners) = viewport.get() {
                // 视口可能比文档大（适配 / 缩小看）：先把角点夹到文档范围，
                // 这样「整张图都在视野里」时框就贴在预览图上，而不是画到外面被裁掉。
                let to_preview = |point: Vec2| {
                    let x = point.x.clamp(0.0, document.width) / document.width;
                    let y = point.y.clamp(0.0, document.height) / document.height;
                    Vec2::new(
                        fit.min().x + x * fit.size.width,
                        fit.min().y + y * fit.size.height,
                    )
                };
                ctx.save();
                ctx.clip_rect(fit);
                let mapped: Vec<Vec2> = corners.iter().map(|corner| to_preview(*corner)).collect();
                for index in 0..mapped.len() {
                    ctx.draw_line(
                        mapped[index],
                        mapped[(index + 1) % mapped.len()],
                        1.0,
                        Paint::new(theme.palette().accent),
                    );
                }
                ctx.restore();
            }

            ctx.stroke_rect(fit, 1.0, Paint::new(theme.palette().border.with_alpha(0.6)));
        })
}

/// 底部一行：线性 / 最近邻，点一下切到那个过滤方式（`update` 写回共享格）。
fn filter_row(
    theme: &'static dyn Theme,
    filter: Rc<Cell<PreviewFilter>>,
    intents: Intents,
) -> impl Component {
    let mut row = Flex::row()
        .align(Align::Center)
        .gap(space::XXS)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore);
    for option in PreviewFilter::ALL {
        let selected = filter.clone();
        let click = filter.clone();
        let intents = intents.clone();
        row = row.child(
            Button::ghost(option.label(), theme)
                .font_size(TextSize::Small.px())
                .dynamic_background(move |interact: InteractState| {
                    if selected.get() == option {
                        SurfaceStyle::new(theme.palette().selection).radius(radius::SM)
                    } else if interact.hovered || interact.pressed {
                        SurfaceStyle::new(theme.palette().surface_hover).radius(radius::SM)
                    } else {
                        SurfaceStyle::new(Color::TRANSPARENT)
                    }
                })
                .on_click(move |_tree, _id| {
                    click.set(option);
                    intents.push(Intent::PreviewFilter(option));
                }),
        );
    }
    row
}

/// 把 `document` 按比例塞进 `rect`（留 `padding`），保持宽高比、居中。
pub(crate) fn fit_rect(rect: Rect, document: Size, padding: f32) -> Rect {
    let available = Size::new(
        (rect.size.width - 2.0 * padding).max(1.0),
        (rect.size.height - 2.0 * padding).max(1.0),
    );
    let scale = (available.width / document.width).min(available.height / document.height);
    Rect::from_center_size(
        rect.center(),
        Size::new(document.width * scale, document.height * scale),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preview_keeps_the_document_aspect_ratio_and_stays_inside() {
        let rect = Rect::from_min_size(Vec2::new(10.0, 20.0), Size::new(200.0, 140.0));
        let fit = fit_rect(rect, Size::new(400.0, 100.0), 6.0);
        assert!((fit.size.width / fit.size.height - 4.0).abs() < 1e-3);
        assert!(fit.left() >= rect.left() && fit.right() <= rect.right());
        assert!(fit.top() >= rect.top() && fit.bottom() <= rect.bottom());
        assert!((fit.center() - rect.center()).length() < 1e-3, "居中");
    }

    #[test]
    fn a_tall_document_is_limited_by_the_height() {
        let rect = Rect::from_min_size(Vec2::ZERO, Size::new(200.0, 140.0));
        let fit = fit_rect(rect, Size::new(100.0, 400.0), 6.0);
        assert!((fit.size.height - (140.0 - 12.0)).abs() < 1e-3);
        assert!(fit.size.width < fit.size.height);
    }
}
