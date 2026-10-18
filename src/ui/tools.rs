//! 工具运行时：把指针事件翻译成 `Tool` 调用，并管好每笔的 undo。
//!
//! `begin_tool` / `continue_tool` / `end_tool` 是主流程，其余是它们的支路
//! （选区 / 吸管 / 选项栏请求）。工具状态都在 [`EditorView`] 的字段里。

use std::rc::Rc;

use igui_core::Vec2;

use crate::app::state::{ActiveTool, AppState};
use crate::canvas::{document_to_pixel, pixel_selection, Selection};
use crate::renderer::sample_pixel;
use crate::tools::{
    fill_selection, magic_wand_region, BrushMode, BrushTool, PaintBucketTool, Tool, ToolContext,
};

use super::{left_pointer, BrushAdjust, BucketAdjust, EditorView};

impl EditorView {
    pub(super) fn is_dragging(&self) -> bool {
        self.brush.is_drawing()
            || self.move_tool.is_moving()
            || self.select_anchor.is_some()
            || !self.lasso_points.is_empty()
            || self.rect_fill.is_filling()
            || self.gradient_tool.is_drawing()
    }

    /// 左键落下：把屏幕点换算成文档坐标，交给当前工具。
    pub(super) fn begin_tool(&mut self, screen: Vec2) {
        let tool = self.state.borrow().active_tool;
        let position = self.document_position(screen);
        match tool {
            ActiveTool::Brush | ActiveTool::Eraser => {
                self.brush.mode = if tool == ActiveTool::Eraser {
                    BrushMode::Erase
                } else {
                    BrushMode::Paint
                };
                let (foreground, selection) = {
                    let state = self.state.borrow();
                    (state.foreground, state.selection.clone())
                };
                self.brush.color = foreground;
                self.brush.clip = selection;
                tracing::debug!(target: "image_editor", tool = self.brush.name(), "stroke_started");
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.brush.on_pointer_down(&mut ctx, left_pointer(position));
                self.texture_dirty = true;
            }
            ActiveTool::Move => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.move_tool
                    .on_pointer_down(&mut ctx, left_pointer(position));
                self.texture_dirty = true;
            }
            ActiveTool::RectangleSelect => {
                self.select_anchor = Some(position);
                self.update_selection(position);
            }
            ActiveTool::LassoSelect | ActiveTool::FreeFill => {
                self.lasso_points.clear();
                self.lasso_points.push(position);
            }
            ActiveTool::PaintBucket => {
                let (foreground, selection) = {
                    let state = self.state.borrow();
                    (state.foreground, state.selection.clone())
                };
                self.paint_bucket.color = foreground;
                self.paint_bucket.clip = selection;
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.paint_bucket
                    .on_pointer_down(&mut ctx, left_pointer(position));
                self.texture_dirty = true;
            }
            // 魔棒：点一下把容差连通区域变成选区（不落笔）。容差 / 连续复用油漆桶。
            ActiveTool::MagicWand => {
                let (tolerance, contiguous) =
                    (self.paint_bucket.tolerance, self.paint_bucket.contiguous);
                let mut state = self.state.borrow_mut();
                let AppState {
                    document,
                    selection,
                    ..
                } = &mut *state;
                let (width, height) = (document.width, document.height);
                let Some(seed) = document_to_pixel(position, width, height) else {
                    return;
                };
                let Some(layer) = document.active_layer() else {
                    return;
                };
                let offset = layer.position;
                let Some(mask) = magic_wand_region(
                    &layer.pixels,
                    offset,
                    (width, height),
                    seed,
                    tolerance,
                    contiguous,
                ) else {
                    return;
                };
                let next = Selection::from_mask((width, height), mask);
                *selection = if next.is_empty() {
                    None
                } else {
                    Some(Rc::new(next))
                };
            }
            ActiveTool::RectangleFill => {
                let (foreground, selection) = {
                    let state = self.state.borrow();
                    (state.foreground, state.selection.clone())
                };
                self.rect_fill.color = foreground;
                self.rect_fill.clip = selection;
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.rect_fill
                    .on_pointer_down(&mut ctx, left_pointer(position));
            }
            ActiveTool::Gradient => {
                let (foreground, background, selection) = {
                    let state = self.state.borrow();
                    (state.foreground, state.background, state.selection.clone())
                };
                if !self.gradient_customized.get() {
                    self.gradient
                        .borrow_mut()
                        .set_endpoints(foreground, background);
                }
                self.gradient_tool.clip = selection;
                self.gradient_tool.kind = self.gradient_kind;
                self.gradient_tool.reverse = self.gradient_reverse.get();
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.gradient_tool
                    .on_pointer_down(&mut ctx, left_pointer(position));
            }
            ActiveTool::Eyedropper => self.pick_color(position),
        }
    }

    pub(super) fn continue_tool(&mut self, screen: Vec2) {
        let tool = self.state.borrow().active_tool;
        let position = self.document_position(screen);
        match tool {
            ActiveTool::Brush | ActiveTool::Eraser if self.brush.is_drawing() => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.brush.on_pointer_move(&mut ctx, left_pointer(position));
                self.texture_dirty = true;
            }
            ActiveTool::Move if self.move_tool.is_moving() => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.move_tool
                    .on_pointer_move(&mut ctx, left_pointer(position));
                self.texture_dirty = true;
            }
            ActiveTool::RectangleSelect => self.update_selection(position),
            ActiveTool::LassoSelect | ActiveTool::FreeFill if !self.lasso_points.is_empty() => {
                if self
                    .lasso_points
                    .last()
                    .map_or(true, |last| (position - *last).length() >= 1.0)
                {
                    self.lasso_points.push(position);
                }
            }
            ActiveTool::RectangleFill if self.rect_fill.is_filling() => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.rect_fill
                    .on_pointer_move(&mut ctx, left_pointer(position));
            }
            ActiveTool::Gradient if self.gradient_tool.is_drawing() => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.gradient_tool
                    .on_pointer_move(&mut ctx, left_pointer(position));
                self.texture_dirty = true;
            }
            _ => {}
        }
    }

    pub(super) fn end_tool(&mut self, screen: Vec2) {
        let tool = self.state.borrow().active_tool;
        let position = self.document_position(screen);
        match tool {
            ActiveTool::Brush | ActiveTool::Eraser if self.brush.is_drawing() => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.brush.on_pointer_up(&mut ctx, left_pointer(position));
                self.texture_dirty = true;
            }
            ActiveTool::Move if self.move_tool.is_moving() => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.move_tool
                    .on_pointer_up(&mut ctx, left_pointer(position));
            }
            ActiveTool::RectangleSelect if self.select_anchor.take().is_some() => {
                self.update_selection(position);
                let message = match self.state.borrow().selection.as_deref() {
                    Some(selection) => match selection.bounds() {
                        Some(region) => format!(
                            "选区 {} × {} @ ({}, {})",
                            region.width, region.height, region.x, region.y
                        ),
                        None => "选区已清空".to_string(),
                    },
                    None => "选区已清空".to_string(),
                };
                *self.message.borrow_mut() = Some(message);
            }
            ActiveTool::LassoSelect if !self.lasso_points.is_empty() => {
                self.lasso_points.push(position);
                let points = std::mem::take(&mut self.lasso_points);
                let (width, height) = self.document_size();
                let selection = Selection::polygon(&points, (width, height));
                let message = if selection.is_empty() {
                    "套索：没有围出区域".to_string()
                } else {
                    format!("自由选区 {} 像素", selection.area())
                };
                self.state.borrow_mut().selection =
                    (!selection.is_empty()).then(|| Rc::new(selection));
                *self.message.borrow_mut() = Some(message);
            }
            ActiveTool::FreeFill if !self.lasso_points.is_empty() => {
                self.lasso_points.push(position);
                let points = std::mem::take(&mut self.lasso_points);
                let (width, height) = self.document_size();
                let region = Selection::polygon(&points, (width, height));
                if region.is_empty() {
                    *self.message.borrow_mut() = Some("自由填充：没有围出区域".to_string());
                } else {
                    let (foreground, clip) = {
                        let state = self.state.borrow();
                        (state.foreground, state.selection.clone())
                    };
                    let mut state = self.state.borrow_mut();
                    let AppState {
                        document, history, ..
                    } = &mut *state;
                    let mut ctx = ToolContext { document, history };
                    fill_selection(&mut ctx, &region, clip.as_deref(), foreground);
                    self.texture_dirty = true;
                    *self.message.borrow_mut() = Some("自由填充：已填充".to_string());
                }
            }
            ActiveTool::RectangleFill if self.rect_fill.is_filling() => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.rect_fill
                    .on_pointer_up(&mut ctx, left_pointer(position));
                self.texture_dirty = true;
                *self.message.borrow_mut() = Some("矩形填充".to_string());
            }
            ActiveTool::Gradient if self.gradient_tool.is_drawing() => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.gradient_tool
                    .on_pointer_up(&mut ctx, left_pointer(position));
                self.texture_dirty = true;
                *self.message.borrow_mut() = Some("渐变".to_string());
            }
            _ => {}
        }
    }

    /// 拖拽框选：起点 + 当前点 -> 裁剪到画布的整数选区。
    fn update_selection(&mut self, position: Vec2) {
        let Some(anchor) = self.select_anchor else {
            return;
        };
        let (width, height) = self.document_size();
        let selection = pixel_selection(anchor, position, width, height)
            .map(|region| Rc::new(Selection::rect(region, (width, height))));
        self.state.borrow_mut().selection = selection;
    }

    /// 吸管：取合成后 `position` 处的颜色作为前景色。
    fn pick_color(&mut self, position: Vec2) {
        let (width, height) = self.document_size();
        let Some((x, y)) = document_to_pixel(position, width, height) else {
            *self.message.borrow_mut() = Some("吸管：点在画布外".to_string());
            return;
        };
        let color = {
            let state = self.state.borrow();
            sample_pixel(&state.document, x, y)
        };
        self.state.borrow_mut().foreground = color;
        tracing::info!(
            target: "image_editor",
            r = color.r,
            g = color.g,
            b = color.b,
            "color_picked"
        );
        *self.message.borrow_mut() = Some(format!(
            "吸管：({x}, {y}) → #{:02X}{:02X}{:02X}",
            color.r, color.g, color.b
        ));
    }

    /// Escape 清空选区；返回是否真的清了。
    pub(super) fn clear_selection(&mut self) -> bool {
        if self.state.borrow().selection.is_none() {
            return false;
        }
        self.state.borrow_mut().selection = None;
        *self.message.borrow_mut() = Some("已清空选区".to_string());
        true
    }

    pub(super) fn adjust_brush_size(&mut self, delta: f32) {
        self.brush.size = (self.brush.size + delta).clamp(BrushTool::MIN_SIZE, BrushTool::MAX_SIZE);
        *self.message.borrow_mut() = Some(format!("笔刷大小 {:.0}px", self.brush.size));
    }

    pub(super) fn adjust_brush_opacity(&mut self, delta: f32) {
        self.brush.opacity = (self.brush.opacity + delta).clamp(0.05, 1.0);
        *self.message.borrow_mut() =
            Some(format!("笔刷不透明度 {:.0}%", self.brush.opacity * 100.0));
    }

    /// 选项栏的 `−` / `+` 请求 -> 真正的笔刷调整。
    pub(super) fn apply_brush_adjust(&mut self, adjust: BrushAdjust) {
        match adjust {
            BrushAdjust::SizeDown => self.adjust_brush_size(-1.0),
            BrushAdjust::SizeUp => self.adjust_brush_size(1.0),
            BrushAdjust::OpacityDown => self.adjust_brush_opacity(-0.05),
            BrushAdjust::OpacityUp => self.adjust_brush_opacity(0.05),
        }
    }

    /// 油漆桶选项栏的「容差 −/+」。
    pub(super) fn apply_bucket_adjust(&mut self, adjust: BucketAdjust) {
        let tolerance = &mut self.paint_bucket.tolerance;
        match adjust {
            BucketAdjust::ToleranceDown => {
                *tolerance = tolerance.saturating_sub(PaintBucketTool::TOLERANCE_STEP);
            }
            BucketAdjust::ToleranceUp => {
                // `u8` 的容差封顶就是 255，`saturating_add` 已经处理。
                *tolerance = tolerance.saturating_add(PaintBucketTool::TOLERANCE_STEP);
            }
        }
    }
}
