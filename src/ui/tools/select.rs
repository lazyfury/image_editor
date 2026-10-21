//! 选区类工具的指针适配（框选 / 套索 / 魔棒 / 自由填充）与选区命令。
//!
//! 选区不在撤销栈里（和路径一样）：这些函数只改 `AppState::selection`，落笔的
//! 工具才往历史里压命令。

use std::rc::Rc;

use igui_core::Vec2;

use crate::canvas::{document_to_pixel, pixel_selection, MergeMode, Selection};
use crate::tools::{fill_selection, magic_wand_region};
use crate::ui::{EditorView, FEATHER_RADIUS};

use super::with_ctx;

impl EditorView {
    // -- 按下 ------------------------------------------------------------

    pub(in crate::ui) fn begin_rect_select(&mut self, position: Vec2) {
        self.begin_selection_edit();
        self.select_anchor = Some(position);
        self.update_selection(position);
    }

    pub(in crate::ui) fn begin_lasso(&mut self, position: Vec2) {
        self.begin_selection_edit();
        self.lasso_points.clear();
        self.lasso_points.push(position);
    }

    /// 自由填充自己围区域，不参与选区合并，所以不记合并基线。
    pub(in crate::ui) fn begin_free_fill(&mut self, position: Vec2) {
        self.lasso_points.clear();
        self.lasso_points.push(position);
    }

    /// 魔棒：点一下把容差连通区域变成选区（不落笔）。容差 / 连续复用油漆桶，
    /// 按住 Shift 加选、Alt 减选。
    pub(in crate::ui) fn begin_magic_wand(&mut self, position: Vec2) {
        let (tolerance, contiguous) = (self.paint_bucket.tolerance, self.paint_bucket.contiguous);
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

    // -- 拖动 ------------------------------------------------------------

    /// 套索 / 自由填充共用：按 1 文档像素的间隔累积折线点。
    pub(in crate::ui) fn continue_lasso(&mut self, position: Vec2) {
        if self
            .lasso_points
            .last()
            .map_or(true, |last| (position - *last).length() >= 1.0)
        {
            self.lasso_points.push(position);
        }
    }

    /// 拖拽框选：起点 + 当前点 -> 裁剪到画布的整数选区。
    pub(in crate::ui) fn update_selection(&mut self, position: Vec2) {
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

    // -- 抬起 ------------------------------------------------------------

    pub(in crate::ui) fn end_rect_select(&mut self, position: Vec2) {
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

    pub(in crate::ui) fn end_lasso(&mut self, position: Vec2) {
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

    pub(in crate::ui) fn end_free_fill(&mut self, position: Vec2) {
        self.lasso_points.push(position);
        let points = std::mem::take(&mut self.lasso_points);
        let (width, height) = self.document_size();
        let region = Selection::polygon(&points, (width, height));
        if region.is_empty() {
            *self.message.borrow_mut() = Some("自由填充：没有围出区域".to_string());
            return;
        }
        let (foreground, clip) = {
            let state = self.state.borrow();
            (state.foreground, state.selection.clone())
        };
        with_ctx(&self.state, |ctx| {
            fill_selection(ctx, &region, clip.as_deref(), foreground);
        });
        self.texture_dirty = true;
        *self.message.borrow_mut() = Some("自由填充：已填充".to_string());
    }

    // -- 合并规则 / 选区命令 ---------------------------------------------

    /// 选区工具按下时：记下合并方式（Shift 加 / Alt 减）和当时的旧选区。
    pub(in crate::ui) fn begin_selection_edit(&mut self) {
        self.merge = MergeMode::from_modifiers(self.modifiers.shift, self.modifiers.alt);
        self.merge_base = self.state.borrow().selection.clone();
    }

    /// 把新选区按修饰键合到旧选区上；空结果就是「没有选区」。返回选中的像素数。
    pub(in crate::ui) fn apply_new_selection(&mut self, next: Selection) -> Option<usize> {
        let merged = self.merge.apply(self.merge_base.as_deref(), next);
        let area = merged.as_ref().map(Selection::area);
        self.state.borrow_mut().selection = merged.map(Rc::new);
        area
    }

    /// Escape 清空选区；返回是否真的清了。
    pub(in crate::ui) fn clear_selection(&mut self) -> bool {
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
}
