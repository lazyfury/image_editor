//! 选项栏的数值请求 -> 工具参数（画笔 `−` / `+`、预设、油漆桶容差）。
//!
//! 选项栏按钮不能直接改工具（回调拿不到 `&mut self`），只把请求写进共享格；
//! `EditorView::update` 每帧取走，交给这里执行。

use crate::tools::{BrushShape, BrushTool, PaintBucketTool};
use crate::ui::{BrushAdjust, BucketAdjust, EditorView};

impl EditorView {
    pub(in crate::ui) fn adjust_brush_size(&mut self, delta: f32) {
        self.brush.size = (self.brush.size + delta).clamp(BrushTool::MIN_SIZE, BrushTool::MAX_SIZE);
        *self.message.borrow_mut() = Some(format!("笔刷大小 {:.0}px", self.brush.size));
    }

    pub(in crate::ui) fn adjust_brush_hardness(&mut self, delta: f32) {
        self.brush.hardness = (self.brush.hardness + delta).clamp(0.05, 1.0);
        *self.message.borrow_mut() = Some(format!("笔刷硬度 {:.0}%", self.brush.hardness * 100.0));
    }

    /// 平滑强度：0% = 折线（笔到手到），100% = 二次中点平滑。
    pub(in crate::ui) fn adjust_brush_smooth(&mut self, delta: f32) {
        self.brush.smooth = (self.brush.smooth + delta).clamp(0.0, 1.0);
        *self.message.borrow_mut() = Some(format!("笔刷平滑 {:.0}%", self.brush.smooth * 100.0));
    }

    /// 落笔间距（相对直径）：调小一点笔迹才连得上。
    pub(in crate::ui) fn adjust_brush_spacing(&mut self, delta: f32) {
        self.brush.spacing_ratio = (self.brush.spacing_ratio + delta).clamp(0.02, 1.0);
        *self.message.borrow_mut() =
            Some(format!("笔刷间距 {:.0}%", self.brush.spacing_ratio * 100.0));
    }

    pub(in crate::ui) fn adjust_brush_opacity(&mut self, delta: f32) {
        self.brush.opacity = (self.brush.opacity + delta).clamp(0.05, 1.0);
        *self.message.borrow_mut() =
            Some(format!("笔刷不透明度 {:.0}%", self.brush.opacity * 100.0));
    }

    /// 选项栏的 `−` / `+` 请求 -> 真正的笔刷调整。
    pub(in crate::ui) fn apply_brush_adjust(&mut self, adjust: BrushAdjust) {
        match adjust {
            BrushAdjust::SizeDown => self.adjust_brush_size(-1.0),
            BrushAdjust::SizeUp => self.adjust_brush_size(1.0),
            BrushAdjust::HardnessDown => self.adjust_brush_hardness(-0.05),
            BrushAdjust::HardnessUp => self.adjust_brush_hardness(0.05),
            BrushAdjust::SmoothDown => self.adjust_brush_smooth(-0.1),
            BrushAdjust::SmoothUp => self.adjust_brush_smooth(0.1),
            BrushAdjust::SpacingDown => self.adjust_brush_spacing(-0.02),
            BrushAdjust::SpacingUp => self.adjust_brush_spacing(0.02),
            BrushAdjust::OpacityDown => self.adjust_brush_opacity(-0.05),
            BrushAdjust::OpacityUp => self.adjust_brush_opacity(0.05),
        }
    }

    /// 应用选项栏选中的一个笔刷预设。
    ///
    /// 选项栏的「像素 / 方形」开关是**显示源**，`update` 每帧把它们写进画笔，
    /// 所以预设改了这两项之后必须回写共享状态，否则下一帧就被覆盖回去。
    pub(in crate::ui) fn apply_brush_preset(&mut self, index: usize) {
        let Some(preset) = crate::tools::brush::preset::PRESETS.get(index) else {
            return;
        };
        preset.apply(&mut self.brush);
        self.options.pixel_mode.set(self.brush.hard);
        self.options
            .square_mode
            .set(self.brush.shape == BrushShape::Square);
        *self.message.borrow_mut() = Some(format!("笔刷预设 {}", preset.name));
    }

    /// 油漆桶选项栏的「容差 −/+」。
    pub(in crate::ui) fn apply_bucket_adjust(&mut self, adjust: BucketAdjust) {
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
