//! 编辑器绘制：把文档 / 像素网格 / 选区 / 预览画进 `PaintContext`。
//!
//! 入口是 [`EditorView::paint`]（宿主 / 自检调用）；其余都是它内部的步骤。
//! 调用方负责先 `clip_rect` 到画布区域，这里不再重复判断。

use igui_core::{Rect, Size, Vec2};
use igui_render::{Paint, PaintContext};

use crate::canvas::{pixel_selection, DOCUMENT_TEXTURE};

use super::selection;
use super::{EditorView, GRID_MIN_ZOOM};

impl EditorView {
    /// 发出这一帧的绘制命令：先在画布区域的裁剪里画世界（文档图像 + 像素网格 +
    /// 选区），再让 `igui_ui::paint` 画 UI 覆盖在上层。
    ///
    /// 单树模型下 `igui_ui::paint` 也会画世界 canvas item，所以文档不再挂成
    /// 场景 `Node2D`（那会被排在 UI 之后、画到所有面板之上）；这里手动画并裁到
    /// 画布区域。
    pub fn paint(&self, ctx: &mut PaintContext) {
        ctx.save();
        if let Some(area) = self.canvas_area_rect() {
            ctx.clip_rect(area);
        }
        self.paint_document(ctx);
        self.paint_pixel_grid(ctx);
        self.paint_selection(ctx);
        self.paint_lasso(ctx);
        self.paint_rect_fill_preview(ctx);
        self.paint_gradient_preview(ctx);
        ctx.restore();
        igui_ui::paint(&self.tree, ctx);
        self.overlays.paint(ctx);
    }

    /// 把文档图像按相机变换画到屏幕空间。调用方负责先裁到画布区域。
    fn paint_document(&self, ctx: &mut PaintContext) {
        let (camera, size) = {
            let state = self.state.borrow();
            (
                state.canvas,
                Size::new(state.document.width as f32, state.document.height as f32),
            )
        };
        let min = camera.document_to_screen(Vec2::ZERO);
        let max = camera.document_to_screen(Vec2::new(size.width, size.height));
        let rect = Rect::from_min_max(
            Vec2::new(min.x.min(max.x), min.y.min(max.y)),
            Vec2::new(min.x.max(max.x), min.y.max(max.y)),
        );
        ctx.draw_image(DOCUMENT_TEXTURE, rect, None, Paint::default());
    }

    /// 像素模式 + 放大到 [`GRID_MIN_ZOOM`] 以上时，在文档上画像素网格。
    ///
    /// 网格画在屏幕空间（相机换算后的坐标），线宽 1 逻辑像素、不随缩放变粗；
    /// 只画「文档矩形 ∩ 画布区域」里的那部分，所以命令数有界。
    fn paint_pixel_grid(&self, ctx: &mut PaintContext) {
        if !self.pixel_mode.get() {
            return;
        }
        let state = self.state.borrow();
        let camera = state.canvas;
        if camera.zoom < GRID_MIN_ZOOM {
            return;
        }
        let Some(area) = self.canvas_area_rect() else {
            return;
        };
        let origin = camera.document_to_screen(Vec2::ZERO);
        let corner = camera.document_to_screen(Vec2::new(
            state.document.width as f32,
            state.document.height as f32,
        ));
        let Some(visible) = Rect::from_min_max(origin, corner).intersection(area) else {
            return;
        };
        let (min, max) = (visible.min(), visible.max());
        let color = self.theme.palette().border.with_alpha(0.3);
        let zoom = camera.zoom;

        let x_first = ((min.x - origin.x) / zoom).ceil() as i64;
        let x_last = ((max.x - origin.x) / zoom).floor() as i64;
        for x in x_first..=x_last {
            let sx = origin.x + x as f32 * zoom;
            ctx.draw_line(Vec2::new(sx, min.y), Vec2::new(sx, max.y), 1.0, color);
        }
        let y_first = ((min.y - origin.y) / zoom).ceil() as i64;
        let y_last = ((max.y - origin.y) / zoom).floor() as i64;
        for y in y_first..=y_last {
            let sy = origin.y + y as f32 * zoom;
            ctx.draw_line(Vec2::new(min.x, sy), Vec2::new(max.x, sy), 1.0, color);
        }
    }

    /// 框选选区的描边：把文档像素矩形换算成屏幕坐标，画一圈黑白相间的
    /// 「马赛克线段」（[`selection`]）。画在文档图像之上、UI 之下。
    fn paint_selection(&self, ctx: &mut PaintContext) {
        let (camera, outline) = {
            let state = self.state.borrow();
            let Some(selection) = state.selection.as_deref() else {
                return;
            };
            (state.canvas, selection.outline().to_vec())
        };
        let edges: Vec<(Vec2, Vec2)> = outline
            .iter()
            .map(|(from, to)| {
                (
                    camera.document_to_screen(*from),
                    camera.document_to_screen(*to),
                )
            })
            .collect();
        selection::paint(ctx, &edges);
    }

    /// 套索拖动中的实时折线（复用 marching ants；`paint_polyline` 会自动闭合）。
    fn paint_lasso(&self, ctx: &mut PaintContext) {
        if self.lasso_points.len() < 2 {
            return;
        }
        let camera = self.state.borrow().canvas;
        let points: Vec<Vec2> = self
            .lasso_points
            .iter()
            .map(|point| camera.document_to_screen(*point))
            .collect();
        selection::paint_polyline(ctx, &points);
    }

    /// 矩形填充拖动中的预览：把两个角点裁成像素矩形，画一圈 marching ants。
    fn paint_rect_fill_preview(&self, ctx: &mut PaintContext) {
        let Some((anchor, current)) = self.rect_fill.preview() else {
            return;
        };
        let (width, height) = self.document_size();
        let Some(region) = pixel_selection(anchor, current, width, height) else {
            return;
        };
        let camera = self.state.borrow().canvas;
        let corners = [
            Vec2::new(region.x as f32, region.y as f32),
            Vec2::new(region.right() as f32, region.y as f32),
            Vec2::new(region.right() as f32, region.bottom() as f32),
            Vec2::new(region.x as f32, region.bottom() as f32),
        ];
        let points: Vec<Vec2> = corners
            .iter()
            .map(|point| camera.document_to_screen(*point))
            .collect();
        selection::paint_polyline(ctx, &points);
    }

    /// 渐变拖动中的轴预览：起点→终点细线 + 两端小圆点。
    fn paint_gradient_preview(&self, ctx: &mut PaintContext) {
        let Some((start, end)) = self.gradient_tool.preview() else {
            return;
        };
        if (end - start).length() < 1.0 {
            return;
        }
        let camera = self.state.borrow().canvas;
        let from = camera.document_to_screen(start);
        let to = camera.document_to_screen(end);
        let color = self.theme.palette().foreground;
        ctx.draw_line(from, to, 1.0, Paint::new(color));
        ctx.fill_circle(from, 3.0, Paint::new(color));
        ctx.fill_circle(to, 3.0, Paint::new(color));
    }
}
