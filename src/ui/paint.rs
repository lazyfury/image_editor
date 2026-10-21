//! 编辑器绘制：把文档 / 像素网格 / 选区 / 预览画进 `PaintContext`。
//!
//! 入口是 [`EditorView::paint`]（宿主 / 自检调用）；其余都是它内部的步骤。
//! 调用方负责先 `clip_rect` 到画布区域，这里不再重复判断。

use igui_core::{Color, Rect, Size, Vec2};
use igui_render::{Paint, PaintContext};

use crate::canvas::{pixel_selection, DOCUMENT_TEXTURE};

use super::selection;
use crate::tools::BrushShape;

use super::{
    EditorView, ANCHOR_SIZE, CROSSHAIR_ARM, GRID_MIN_ZOOM, HANDLE_DOT, MIN_CURSOR_RADIUS,
    PEN_PREVIEW_TOLERANCE,
};

/// 把矩形对齐到 `1 / scale_factor` 的网格（设备像素）。
fn snap_to_device(rect: Rect, scale_factor: f32) -> Rect {
    let unit = 1.0 / scale_factor.max(0.01);
    let snap = |value: f32| (value / unit).round() * unit;
    let min = Vec2::new(snap(rect.left()), snap(rect.top()));
    let max = Vec2::new(snap(rect.right()), snap(rect.bottom()));
    Rect::from_min_max(
        min,
        Vec2::new(max.x.max(min.x + unit), max.y.max(min.y + unit)),
    )
}

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
        self.paint_brush_cursor(ctx);
        self.paint_selection(ctx);
        self.paint_lasso(ctx);
        self.paint_pen(ctx);
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
        let mut rect = Rect::from_min_max(
            Vec2::new(min.x.min(max.x), min.y.min(max.y)),
            Vec2::new(min.x.max(max.x), min.y.max(max.y)),
        );
        // 最近邻放大时把矩形对齐到设备像素网格：错半格会让 1 像素的硬边在屏幕上
        // 一会儿 1px、一会儿 2px（看着像毛刺）。缩小时用线性，对齐就没有意义了。
        if self.image_smoothing() == crate::canvas::ImageSmoothing::Nearest {
            rect = snap_to_device(rect, self.scale_factor);
        }
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

    /// 笔刷光标圈：跟着指针画当前笔刷的轮廓（屏幕空间，大小不随缩放走样）。
    ///
    /// 只在画笔 / 橡皮、且指针落在画布区域里时画。黑白各画一遍（2px 黑底 +
    /// 1px 白线），这样深浅底色上都看得见 —— 和选区的马赛克描边同一个思路。
    fn paint_brush_cursor(&self, ctx: &mut PaintContext) {
        if !super::options_bar::tool_has_brush(self.state.borrow().active_tool) {
            return;
        }
        let Some(pointer) = self.pointer else {
            return;
        };
        if !self.canvas_area_contains(pointer) {
            return;
        }
        let zoom = self.state.borrow().canvas.zoom;
        let tip = self.brush.tip();
        // 直径换算到屏幕；太小（比如 1px 笔缩到 25%）时给个最小半径，不然看不见。
        let radius = (tip.size * zoom * 0.5).max(MIN_CURSOR_RADIUS);
        let black = Paint::new(Color::BLACK);
        let white = Paint::new(Color::WHITE);
        let outline = |ctx: &mut PaintContext, width: f32, paint: Paint| match tip.shape {
            BrushShape::Round => ctx.stroke_circle(pointer, radius, width, paint),
            BrushShape::Square => {
                let rect = Rect::from_center_size(pointer, Size::splat(radius * 2.0));
                ctx.stroke_rect(rect, width, paint);
            }
        };
        outline(ctx, 2.0, black);
        outline(ctx, 1.0, white);
        // 中心十字：细笔也看得出准心在哪。
        for (dx, dy) in [(1.0, 0.0), (0.0, 1.0)] {
            let arm = Vec2::new(dx, dy) * CROSSHAIR_ARM;
            ctx.draw_line(pointer - arm, pointer + arm, 1.0, white);
        }
    }

    /// 钢笔画到一半 / 已收尾的路径：双色虚线 + 锚点 + 控制柄 + 橡皮筋。
    ///
    /// 全部画在屏幕空间（按相机换算），容差随缩放收紧 —— 放大看曲线也不会变折线。
    fn paint_pen(&self, ctx: &mut PaintContext) {
        // 只在钢笔是当前工具时显示路径（没有 Paths 面板，换工具就看不见了）。
        if self.state.borrow().active_tool != crate::app::state::ActiveTool::Pen {
            return;
        }
        let path = self.pen.path();
        if path.is_empty() {
            return;
        }
        let camera = self.state.borrow().canvas;
        let tolerance = PEN_PREVIEW_TOLERANCE / camera.zoom.max(0.01);
        let to_screen = |point: Vec2| camera.document_to_screen(point);
        let palette = self.theme.palette();

        // 已完成的段：闭合路径绕回起点，开放路径只连相邻锚点。
        let points: Vec<Vec2> = path.flatten(tolerance).into_iter().map(to_screen).collect();
        if path.is_closed() {
            selection::paint_polyline(ctx, &points);
        } else {
            let edges: Vec<(Vec2, Vec2)> =
                points.windows(2).map(|pair| (pair[0], pair[1])).collect();
            selection::paint(ctx, &edges);
        }

        // 控制柄：锚点 -> 柄端细线 + 小圆点（只有平滑锚点才有）。
        for anchor in path.anchors() {
            let center = to_screen(anchor.point);
            for handle in [anchor.handle_in, anchor.handle_out] {
                if handle == Vec2::ZERO {
                    continue;
                }
                let tip = to_screen(anchor.point + handle);
                ctx.draw_line(center, tip, 1.0, Paint::new(palette.muted));
                ctx.fill_circle(tip, HANDLE_DOT, Paint::new(palette.foreground));
            }
        }

        // 锚点：空心方块；点回起点就能闭合时把起点画成实心提示。
        let closable = self.pen.can_close();
        for (index, anchor) in path.anchors().iter().enumerate() {
            let center = to_screen(anchor.point);
            let rect = Rect::from_center_size(center, Size::splat(ANCHOR_SIZE));
            if index == 0 && closable {
                ctx.fill_rect(rect, Paint::new(palette.foreground));
            } else {
                ctx.stroke_rect(rect, 1.0, Paint::new(palette.foreground));
            }
        }

        // 橡皮筋：最后一个锚点 -> 光标（带控制柄时是曲线）。
        if let Some(band) = self.pen.rubber_band(tolerance) {
            let screen: Vec<Vec2> = band.into_iter().map(to_screen).collect();
            for pair in screen.windows(2) {
                ctx.draw_line(pair[0], pair[1], 1.0, Paint::new(palette.muted));
            }
        }
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
        let Some((start, end)) = self.gradient.tool.preview() else {
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
