//! 钢笔的三个动作：路径 -> 选区 / 填充 / 描边（+ 清除）。
//!
//! 钢笔本身（[`crate::tools::PenTool`]）只攒路径，不碰像素；真正落到图层上的
//! 是这里：选区写进 `AppState`，填充复用 [`fill_selection`]，描边复用当前画笔
//! 的 [`BrushTool::stroke_path`]。所以「一条路径 = 一步 undo」只发生在描边 /
//! 填充上，**路径本身不进历史栈**（和选区一样）。

use std::rc::Rc;

use crate::app::state::AppState;
use crate::tools::brush::BrushMode;
use crate::tools::{fill_selection, ToolContext};

use super::options_bar::PenAction;
use super::EditorView;

/// 路径光栅化 / 描边用的展平容差（文档像素）。
const PEN_RASTER_TOLERANCE: f32 = 0.25;

impl EditorView {
    /// 选项栏的钢笔动作。
    pub(super) fn apply_pen_action(&mut self, action: PenAction) {
        match action {
            PenAction::Select => {
                self.pen_to_selection();
            }
            PenAction::Fill => {
                self.pen_fill();
            }
            PenAction::Stroke => {
                self.pen_stroke();
            }
            PenAction::Clear => {
                let had = self.pen.cancel();
                *self.message.borrow_mut() = Some(if had {
                    "钢笔：已清除路径".to_string()
                } else {
                    "钢笔：没有路径".to_string()
                });
            }
        }
    }

    /// 把当前路径变成选区（开放路径按首尾相连闭合）。
    pub(super) fn pen_to_selection(&mut self) -> bool {
        let Some(selection) = self.pen_selection() else {
            return false;
        };
        let area = selection.area();
        self.state.borrow_mut().selection = Some(Rc::new(selection));
        *self.message.borrow_mut() = Some(format!("钢笔选区 {area} 像素"));
        true
    }

    /// 用前景色填充路径内部（按当前选区裁剪，一条 undo）。
    pub(super) fn pen_fill(&mut self) -> bool {
        let Some(region) = self.pen_selection() else {
            return false;
        };
        let (foreground, clip) = {
            let state = self.state.borrow();
            (state.foreground, state.selection.clone())
        };
        {
            let mut state = self.state.borrow_mut();
            let AppState {
                document, history, ..
            } = &mut *state;
            let mut ctx = ToolContext { document, history };
            fill_selection(&mut ctx, &region, clip.as_deref(), foreground);
        }
        self.texture_dirty = true;
        *self.message.borrow_mut() = Some("钢笔：已填充".to_string());
        true
    }

    /// 用当前画笔沿路径描一遍（宽度 / 硬度 / 不透明度 / 预设都跟随画笔）。
    pub(super) fn pen_stroke(&mut self) -> bool {
        let points = self.pen.flattened(PEN_RASTER_TOLERANCE);
        if points.len() < 2 {
            *self.message.borrow_mut() = Some("钢笔：没有可描边的路径".to_string());
            return false;
        }
        let (foreground, selection) = {
            let state = self.state.borrow();
            (state.foreground, state.selection.clone())
        };
        // 描边就是「用画笔沿路径画一笔」，颜色 / 选区裁剪和手画一致。
        self.brush.mode = BrushMode::Paint;
        self.brush.color = foreground;
        self.brush.clip = selection;
        {
            let mut state = self.state.borrow_mut();
            let AppState {
                document, history, ..
            } = &mut *state;
            let mut ctx = ToolContext { document, history };
            self.brush.stroke_path(&mut ctx, &points);
        }
        self.texture_dirty = true;
        *self.message.borrow_mut() = Some("钢笔：已描边".to_string());
        true
    }

    /// 路径 -> 选区（围不出区域时给一句提示）。
    fn pen_selection(&self) -> Option<crate::canvas::Selection> {
        let (width, height) = self.document_size();
        let selection = self.pen.polygon((width, height));
        if selection.is_none() {
            *self.message.borrow_mut() = Some("钢笔：路径围不出区域".to_string());
        }
        selection
    }

    /// 选项栏里的路径摘要（锚点数 + 开闭状态）。
    pub(super) fn pen_summary(&self) -> String {
        let path = self.pen.path();
        if path.is_empty() {
            return "没有路径".to_string();
        }
        format!(
            "{} 个锚点 · {} 段 · {}",
            path.len(),
            path.segment_count(),
            if path.is_closed() {
                "已闭合"
            } else {
                "开放"
            }
        )
    }

    /// 收尾当前路径并在状态栏报告（`Enter` 用）。
    pub(super) fn finish_pen_path(&mut self) -> bool {
        if !self.pen.finish() {
            return false;
        }
        *self.message.borrow_mut() = Some(format!("钢笔：{}", self.pen_summary()));
        true
    }
}
