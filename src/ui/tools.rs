//! 工具运行时：把指针事件翻译成 `Tool` 调用，并管好每笔的 undo。
//!
//! `begin_tool` / `continue_tool` / `end_tool` 是主流程，其余是它们的支路
//! （选区 / 吸管 / 选项栏请求）。工具状态都在 [`EditorView`] 的字段里。

use std::rc::Rc;

use igui_core::{Key, Vec2};

use crate::app::state::{ActiveTool, AppState};
use crate::canvas::{document_to_pixel, pixel_selection, Selection};
use crate::renderer::sample_pixel;
use crate::tools::brush::preset;
use crate::tools::{
    fill_selection, magic_wand_region, BrushMode, BrushShape, BrushTool, PaintBucketTool, Tool,
    ToolContext,
};

use super::{left_pointer, BrushAdjust, BucketAdjust, EditorView, FEATHER_RADIUS, PEN_HIT_RADIUS};

impl EditorView {
    pub(super) fn is_dragging(&self) -> bool {
        self.brush.is_drawing()
            || self.move_tool.is_moving()
            || self.select_anchor.is_some()
            || !self.lasso_points.is_empty()
            || self.rect_fill.is_filling()
            || self.gradient_tool.is_drawing()
            || self.pen.is_dragging()
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
                {
                    let mut state = self.state.borrow_mut();
                    let AppState {
                        document, history, ..
                    } = &mut *state;
                    let mut ctx = ToolContext { document, history };
                    self.brush.on_pointer_down(&mut ctx, left_pointer(position));
                }
                // 借用结束再标脏：画笔把写过的矩形记在 `brush` 里。
                self.mark_brush_dirty();
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
                self.begin_selection_edit();
                self.select_anchor = Some(position);
                self.update_selection(position);
            }
            ActiveTool::LassoSelect => {
                self.begin_selection_edit();
                self.lasso_points.clear();
                self.lasso_points.push(position);
            }
            ActiveTool::FreeFill => {
                self.lasso_points.clear();
                self.lasso_points.push(position);
            }
            // 钢笔：每次按下要么落一个锚点、要么点回起点闭合；拖动出控制柄。
            ActiveTool::Pen => {
                self.sync_pen_radius();
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.pen.on_pointer_down(&mut ctx, left_pointer(position));
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
            // 魔棒：点一下把容差连通区域变成选区（不落笔）。容差 / 连续复用油漆桶，
            // 按住 Shift 加选、Alt 减选。
            ActiveTool::MagicWand => {
                let (tolerance, contiguous) =
                    (self.paint_bucket.tolerance, self.paint_bucket.contiguous);
                let found = {
                    let state = self.state.borrow();
                    let document = &state.document;
                    let (width, height) = (document.width, document.height);
                    document_to_pixel(position, width, height).and_then(|seed| {
                        let layer = document.active_layer()?;
                        magic_wand_region(
                            &layer.pixels,
                            layer.position,
                            (width, height),
                            seed,
                            tolerance,
                            contiguous,
                        )
                        .map(|mask| ((width, height), mask))
                    })
                };
                let Some((document, mask)) = found else {
                    return;
                };
                // 魔棒的掩码是布尔（洪水填充也用它），这里转成覆盖率掩码。
                let coverage = mask
                    .into_iter()
                    .map(|selected| if selected { 255 } else { 0 })
                    .collect();
                self.begin_selection_edit();
                self.apply_new_selection(Selection::from_mask(document, coverage));
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
                {
                    let mut state = self.state.borrow_mut();
                    let AppState {
                        document, history, ..
                    } = &mut *state;
                    let mut ctx = ToolContext { document, history };
                    self.brush.on_pointer_move(&mut ctx, left_pointer(position));
                }
                self.mark_brush_dirty();
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
            ActiveTool::Pen if self.pen.is_dragging() => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.pen.on_pointer_move(&mut ctx, left_pointer(position));
            }
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
                {
                    let mut state = self.state.borrow_mut();
                    let AppState {
                        document, history, ..
                    } = &mut *state;
                    let mut ctx = ToolContext { document, history };
                    self.brush.on_pointer_up(&mut ctx, left_pointer(position));
                }
                self.mark_brush_dirty();
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
            ActiveTool::Pen => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.pen.on_pointer_up(&mut ctx, left_pointer(position));
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
                let next = Selection::polygon(&points, (width, height));
                let message = match self.apply_new_selection(next) {
                    Some(area) => format!("自由选区 {area} 像素"),
                    None => "套索：没有围出区域".to_string(),
                };
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

    /// 画笔刚写过的地方：只把那一块标脏（局部合成），别整图重算。
    pub(super) fn mark_brush_dirty(&mut self) {
        if let Some(region) = self.brush.take_dirty() {
            self.mark_region_dirty(region);
        }
    }

    /// 把「命中锚点 / 闭合」的半径换算到文档单位（屏幕上恒为 `PEN_HIT_RADIUS`）。
    pub(super) fn sync_pen_radius(&mut self) {
        let zoom = self.state.borrow().canvas.zoom.max(0.01);
        self.pen.radius = PEN_HIT_RADIUS / zoom;
    }

    /// 钢笔画到一半时的悬停：更新橡皮筋与闭合提示。
    ///
    /// 不按键的移动不会走 `continue_tool`（那里要求 `is_dragging`），所以指针
    /// 进 / 出画布都要单独转发一次。
    pub(super) fn hover_pen(&mut self, screen: Vec2) {
        self.sync_pen_radius();
        let inside = self.canvas_area_contains(screen);
        self.pen
            .set_hover(inside.then(|| self.document_position(screen)));
    }

    /// 钢笔的键位：Esc 取消 / Enter 收尾 / Backspace 删最后一个锚点。
    ///
    /// 只在钢笔是当前工具、且确实有路径可动时消费事件。
    pub(super) fn handle_pen_key(&mut self, key: Key) -> bool {
        if self.state.borrow().active_tool != ActiveTool::Pen {
            return false;
        }
        match key {
            Key::Escape if self.pen.cancel() => {
                *self.message.borrow_mut() = Some("钢笔：已取消路径".to_string());
                return true;
            }
            Key::Enter => return self.finish_pen_path(),
            Key::Backspace if self.pen.undo_anchor() => {
                *self.message.borrow_mut() = Some(format!("钢笔：{}", self.pen_summary()));
                return true;
            }
            _ => {}
        }
        false
    }

    /// 拖拽框选：起点 + 当前点 -> 裁剪到画布的整数选区。
    fn update_selection(&mut self, position: Vec2) {
        let Some(anchor) = self.select_anchor else {
            return;
        };
        let (width, height) = self.document_size();
        let Some(region) = pixel_selection(anchor, position, width, height) else {
            // 退化拖动（点了一下没拖）：回到按下前的选区。
            self.state.borrow_mut().selection = self.merge_base.clone();
            return;
        };
        let next = Selection::rect(region, (width, height));
        self.apply_new_selection(next);
    }

    /// 选区工具按下时：记下合并方式（Shift 加 / Alt 减）和当时的旧选区。
    pub(super) fn begin_selection_edit(&mut self) {
        self.merge =
            crate::canvas::MergeMode::from_modifiers(self.modifiers.shift, self.modifiers.alt);
        self.merge_base = self.state.borrow().selection.clone();
    }

    /// 把新选区按修饰键合到旧选区上；空结果就是「没有选区」。返回选中的像素数。
    pub(super) fn apply_new_selection(&mut self, next: Selection) -> Option<usize> {
        let merged = self.merge.apply(self.merge_base.as_deref(), next);
        let area = merged.as_ref().map(Selection::area);
        self.state.borrow_mut().selection = merged.map(Rc::new);
        area
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

    /// 全选（整个文档）。
    pub fn select_all(&mut self) -> bool {
        let (width, height) = self.document_size();
        let selection = Selection::all((width, height));
        let area = selection.area();
        self.state.borrow_mut().selection = Some(Rc::new(selection));
        *self.message.borrow_mut() = Some(format!("全选 {area} 像素"));
        true
    }

    /// 反选；没有选区时等于全选，选满整个文档时等于取消选择。
    pub fn invert_selection(&mut self) -> bool {
        let Some(selection) = self.state.borrow().selection.clone() else {
            return self.select_all();
        };
        let inverted = selection.invert();
        let message = if inverted.is_empty() {
            "反选：已取消选择".to_string()
        } else {
            format!("反选 {} 像素", inverted.area())
        };
        self.state.borrow_mut().selection = (!inverted.is_empty()).then(|| Rc::new(inverted));
        *self.message.borrow_mut() = Some(message);
        true
    }

    /// 羽化当前选区（半径 = [`FEATHER_RADIUS`] 文档像素），边缘按覆盖率渐变。
    pub fn feather_selection(&mut self) -> bool {
        let Some(selection) = self.state.borrow().selection.clone() else {
            *self.message.borrow_mut() = Some("没有选区可羽化".to_string());
            return false;
        };
        let feathered = selection.feathered(FEATHER_RADIUS);
        *self.message.borrow_mut() = Some(format!(
            "羽化 {} px · {}",
            FEATHER_RADIUS,
            if feathered.is_empty() {
                "选区已为空".to_string()
            } else {
                format!("{} 像素", feathered.area())
            }
        ));
        self.state.borrow_mut().selection = (!feathered.is_empty()).then(|| Rc::new(feathered));
        true
    }

    pub(super) fn adjust_brush_size(&mut self, delta: f32) {
        self.brush.size = (self.brush.size + delta).clamp(BrushTool::MIN_SIZE, BrushTool::MAX_SIZE);
        *self.message.borrow_mut() = Some(format!("笔刷大小 {:.0}px", self.brush.size));
    }

    pub(super) fn adjust_brush_hardness(&mut self, delta: f32) {
        self.brush.hardness = (self.brush.hardness + delta).clamp(0.05, 1.0);
        *self.message.borrow_mut() = Some(format!("笔刷硬度 {:.0}%", self.brush.hardness * 100.0));
    }

    /// 平滑强度：0% = 折线（笔到手到），100% = 二次中点平滑。
    pub(super) fn adjust_brush_smooth(&mut self, delta: f32) {
        self.brush.smooth = (self.brush.smooth + delta).clamp(0.0, 1.0);
        *self.message.borrow_mut() = Some(format!("笔刷平滑 {:.0}%", self.brush.smooth * 100.0));
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
            BrushAdjust::HardnessDown => self.adjust_brush_hardness(-0.05),
            BrushAdjust::HardnessUp => self.adjust_brush_hardness(0.05),
            BrushAdjust::SmoothDown => self.adjust_brush_smooth(-0.1),
            BrushAdjust::SmoothUp => self.adjust_brush_smooth(0.1),
            BrushAdjust::OpacityDown => self.adjust_brush_opacity(-0.05),
            BrushAdjust::OpacityUp => self.adjust_brush_opacity(0.05),
        }
    }

    /// 应用选项栏选中的一个笔刷预设。
    ///
    /// 选项栏的「像素 / 方形」开关是**显示源**，`update` 每帧把它们写进画笔，
    /// 所以预设改了这两项之后必须回写共享状态，否则下一帧就被覆盖回去。
    pub(super) fn apply_brush_preset(&mut self, index: usize) {
        let Some(preset) = preset::PRESETS.get(index) else {
            return;
        };
        preset.apply(&mut self.brush);
        self.pixel_mode.set(self.brush.hard);
        self.square_mode.set(self.brush.shape == BrushShape::Square);
        *self.message.borrow_mut() = Some(format!("笔刷预设 {}", preset.name));
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
