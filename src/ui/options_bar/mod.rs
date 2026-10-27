//! 工具选项栏：菜单栏下面的一行，显示当前工具的配置。
//!
//! 画笔 / 橡皮显示笔刷预设、大小、硬度、不透明度（`−` / `+` 按钮）；钢笔显示
//! 锚点数 + 「建立选区 / 填充 / 描边 / 清除」；其余工具显示一句操作提示。按钮只
//! 往意图队列里塞一个 [`Intent`]，真正的改动由
//! [`EditorView::update`](crate::ui::EditorView) 统一执行 —— 回调拿不到
//! `&mut self`，和菜单 / 面板一个套路。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use igui_components::{Button, Component, Flex, NodeRef, Text};
use igui_core::{Color, Edges, NodeId};
use igui_theme::{radius, space, SurfaceLevel, Theme, Tone};
use igui_ui::{Align, MouseFilter, SurfaceStyle};

use crate::app::state::ActiveTool;
use crate::document::Gradient;
use crate::ui::gradient_editor;

use super::{Intent, Intents};

mod rows;

/// 选项栏的固定高度（逻辑像素，= 主题的 `row_height`）。
///
/// 切工具时选项栏显示的行不一样（画笔 / 钢笔 / 油漆桶 / 渐变 / 只有提示），
/// 内容高度会差 8~16px，不钉死的话画布会跟着上下跳。
pub const OPTIONS_BAR_HEIGHT: f32 = 32.0;

/// 选项栏里的一次笔刷调整。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrushAdjust {
    SizeDown,
    SizeUp,
    HardnessDown,
    HardnessUp,
    SmoothDown,
    SmoothUp,
    SpacingDown,
    SpacingUp,
    OpacityDown,
    OpacityUp,
}

impl BrushAdjust {
    /// 自检 / 报告里的短名字。
    pub const fn label(self) -> &'static str {
        match self {
            Self::SizeDown => "笔刷大小 −",
            Self::SizeUp => "笔刷大小 +",
            Self::HardnessDown => "笔刷硬度 −",
            Self::HardnessUp => "笔刷硬度 +",
            Self::SmoothDown => "笔刷平滑 −",
            Self::SmoothUp => "笔刷平滑 +",
            Self::SpacingDown => "笔刷间距 −",
            Self::SpacingUp => "笔刷间距 +",
            Self::OpacityDown => "不透明度 −",
            Self::OpacityUp => "不透明度 +",
        }
    }
}

/// 选项栏里的布尔开关（像素模式 / 方形笔）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrushToggle {
    /// 像素模式：硬边（不做抗锯齿）。
    Hard,
    /// 方形笔（否则圆头）。
    Square,
}

impl BrushToggle {
    /// 按钮上的短标签 / 自检报告里的名字。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Hard => "像素",
            Self::Square => "方形",
        }
    }
}

/// 选项栏里的一次钢笔动作（路径 -> 选区 / 像素）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenAction {
    /// 路径围成的区域变成选区。
    Select,
    /// 用前景色填充路径内部。
    Fill,
    /// 用当前画笔沿路径描一遍。
    Stroke,
    /// 丢掉当前路径。
    Clear,
}

impl PenAction {
    pub const ALL: [Self; 4] = [Self::Select, Self::Fill, Self::Stroke, Self::Clear];

    /// 按钮标签 / 自检报告里的短名字。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Select => "建立选区",
            Self::Fill => "填充",
            Self::Stroke => "描边",
            Self::Clear => "清除",
        }
    }
}

/// 选项栏里的一次油漆桶调整。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BucketAdjust {
    ToleranceDown,
    ToleranceUp,
}

/// 油漆桶的布尔开关。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BucketToggle {
    /// 连续：只填连通的同色区域（否则填全图所有匹配像素）。
    Contiguous,
}

impl BucketToggle {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Contiguous => "连续",
        }
    }
}

/// 选项栏里的一次渐变类型切换。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradientAdjust {
    KindPrev,
    KindNext,
}

/// 渐变工具选项栏的共享状态 / 意图队列。
pub struct GradientOptions {
    pub gradient: Rc<RefCell<Gradient>>,
    pub reverse: Rc<Cell<bool>>,
    /// 类型切换 / 「编辑…」都往这里塞意图。
    pub intents: Intents,
}

/// 选项栏在视图里的状态：控件句柄 + 按钮表 + 四个「显示源」共享格。
///
/// 这些都是「选项栏自己的东西」：`update` 每帧把共享格写进工具（`brush` /
/// `paint_bucket`），`sync_options` 把状态写回文本。收成一组，`EditorView`
/// 上就少 28 个散字段。
pub struct OptionsState {
    /// 整行（测试 / 自检验证「菜单 → 标签条 → 选项栏」的顺序）。
    pub panel: NodeId,
    pub tool: NodeId,
    pub brush: NodeId,
    pub size: NodeId,
    pub hardness: NodeId,
    pub smooth: NodeId,
    pub spacing: NodeId,
    pub opacity: NodeId,
    /// 对称绘画的值标签 + 循环按钮。
    pub symmetry: NodeId,
    pub symmetry_button: NodeId,
    /// 「预设」触发按钮 / 它的值标签。
    pub preset: NodeId,
    pub preset_label: NodeId,
    /// 钢笔选项行 / 路径摘要 / 动作按钮。
    pub pen: NodeId,
    pub pen_summary: NodeId,
    pub pen_buttons: Vec<(PenAction, NodeId)>,
    pub bucket: NodeId,
    pub gradient: NodeId,
    pub tolerance: NodeId,
    pub hint: NodeId,
    /// 「全部属性…」按钮（打开设置窗口）。
    pub settings_button: NodeId,
    /// 按钮表（测试 / 自检点击用）。
    pub brush_buttons: Vec<(BrushAdjust, NodeId)>,
    pub brush_toggles: Vec<(BrushToggle, NodeId)>,
    pub bucket_buttons: Vec<(BucketAdjust, NodeId)>,
    pub bucket_toggles: Vec<(BucketToggle, NodeId)>,
    /// 显示源：选项栏按钮直接翻转，`update` 每帧同步进工具。
    pub pixel_mode: Rc<Cell<bool>>,
    pub square_mode: Rc<Cell<bool>>,
    pub symmetry_mode: Rc<Cell<crate::tools::Symmetry>>,
    pub bucket_contiguous: Rc<Cell<bool>>,
}

/// 选项栏里需要回写的文本 / 容器槽位。
#[derive(Default)]
pub struct OptionsRefs {
    /// 选项栏整行（菜单栏 / 标签条下面的那一行；宿主与测试验证顺序用）。
    pub panel: NodeRef,
    pub tool: NodeRef,
    pub brush: NodeRef,
    /// 「预设」触发按钮的值标签（`sync` 写当前预设名）。
    pub preset: NodeRef,
    /// 「预设」触发按钮本身（测试点击 / 下拉锚点）。
    pub preset_button: NodeRef,
    pub size: NodeRef,
    /// 对称绘画：值标签 + 循环按钮。
    pub symmetry: NodeRef,
    pub symmetry_button: NodeRef,
    pub hardness: NodeRef,
    pub smooth: NodeRef,
    pub spacing: NodeRef,
    pub opacity: NodeRef,
    pub bucket: NodeRef,
    /// 钢笔：整行容器 / 路径摘要文字 + 四个动作按钮。
    pub pen_panel: NodeRef,
    pub pen: NodeRef,
    pub tolerance: NodeRef,
    pub gradient: NodeRef,
    pub gradient_kind: NodeRef,
    pub gradient_preview: NodeRef,
    pub gradient_edit: NodeRef,
    pub hint: NodeRef,
    /// 「全部属性…」按钮（选项栏放不下的属性在 `ui/dialogs` 的窗口里）。
    pub settings: NodeRef,
}

/// 选项栏各行的共享输入：意图队列 + 各工具的开关格（显示源）。
pub struct BarInputs {
    pub intents: Intents,
    pub hard: Rc<Cell<bool>>,
    pub square: Rc<Cell<bool>>,
    pub symmetry: Rc<Cell<crate::tools::Symmetry>>,
    pub contiguous: Rc<Cell<bool>>,
    pub gradient: GradientOptions,
}

/// 按钮节点收集器：测试与自检靠它们真的点一下（每行只写自己那一组）。
pub struct OptionSlots<'a> {
    pub brush: &'a mut Vec<(BrushAdjust, NodeRef)>,
    pub brush_toggles: &'a mut Vec<(BrushToggle, NodeRef)>,
    pub pen: &'a mut Vec<(PenAction, NodeRef)>,
    pub bucket: &'a mut Vec<(BucketAdjust, NodeRef)>,
    pub bucket_toggles: &'a mut Vec<(BucketToggle, NodeRef)>,
    pub gradient: &'a mut Vec<(GradientAdjust, NodeRef)>,
}

/// 工具选项栏：工具名 + 各工具的配置行（画笔 / 钢笔 / 油漆桶 / 渐变）+ 提示。
///
/// 每行一个函数，`update` 每帧按当前工具显示 / 隐藏（见 `sync::sync_options`）。
pub fn options_bar(
    theme: &'static dyn Theme,
    preset_label: &str,
    inputs: BarInputs,
    refs: &OptionsRefs,
    slots: OptionSlots<'_>,
) -> impl Component {
    let brush = rows::brush_row(
        theme,
        preset_label,
        &inputs,
        refs,
        slots.brush,
        slots.brush_toggles,
    );
    let pen = rows::pen_row(theme, &inputs.intents, refs, slots.pen);
    let bucket = rows::bucket_row(theme, &inputs, refs, slots.bucket, slots.bucket_toggles);
    let gradient = rows::gradient_row(theme, &inputs.gradient, refs, slots.gradient);

    Flex::row()
        .align(Align::Center)
        .gap(space::MD)
        // 高度钉死：切工具只换行，不换高度（见 `OPTIONS_BAR_HEIGHT`）。
        // 纵向不留 padding：常量算的是**整行**高度，别让 padding 再加 8px。
        .min_size(0.0, OPTIONS_BAR_HEIGHT)
        .padding(Edges::symmetric(space::SM, 0.0))
        .background(theme.surface(SurfaceLevel::Raised))
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::small("", theme).ref_(&refs.tool))
        .child(brush.ref_(&refs.brush))
        .child(pen.ref_(&refs.pen_panel))
        .child(bucket.ref_(&refs.bucket))
        .child(gradient.ref_(&refs.gradient))
        .child(Text::caption("", theme).tone(Tone::Muted).ref_(&refs.hint))
        .child(settings_button(theme, &inputs.intents).ref_(&refs.settings))
        .ref_(&refs.panel)
}

/// 选项栏末尾的「全部属性…」：一行放不下的属性（笔刷 / 笔尖 / 油漆桶 / 记录）
/// 都在它打开的模拟窗口里。
fn settings_button(theme: &'static dyn Theme, intents: &Intents) -> impl Component {
    let intents = intents.clone();
    Button::ghost("全部属性…", theme)
        .on_click(move |_tree, _id| intents.push(Intent::Dialog(crate::ui::DialogKind::Settings)))
}

pub const fn options_hint(tool: ActiveTool) -> &'static str {
    match tool {
        ActiveTool::Move => "在画布上拖动移动当前图层",
        ActiveTool::RectangleSelect => "拖动框选 · Esc 清空",
        ActiveTool::LassoSelect => "按住拖动套索 · 松手成选区 · Esc 清空",
        ActiveTool::Pen => "点击落锚点 · 拖动出曲线 · 点回起点闭合 · Enter 收尾 · Esc 取消",
        ActiveTool::PaintBucket => "点击填充连通区域 · 容差可选",
        ActiveTool::MagicWand => "点击选中容差连通区域 · 容差可选",
        ActiveTool::Gradient => "拖一条轴定义方向 · 编辑… 调多节点颜色",
        ActiveTool::RectangleFill => "拖动矩形 · 松手用前景色填充",
        ActiveTool::FreeFill => "按住拖出自由形状 · 松手填充内部",
        ActiveTool::Eyedropper => "点击画布取合成后的颜色",
        ActiveTool::Hand => "拖动平移画布 · 按住空格临时平移",
        ActiveTool::Zoom => "点击放大 · 按住 Option 点击缩小",
        ActiveTool::Brush | ActiveTool::Eraser => "",
    }
}

/// 画笔 / 橡皮才显示笔刷配置。
pub const fn tool_has_brush(tool: ActiveTool) -> bool {
    matches!(tool, ActiveTool::Brush | ActiveTool::Eraser)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_brush_and_eraser_show_the_brush_config() {
        assert!(tool_has_brush(ActiveTool::Brush));
        assert!(tool_has_brush(ActiveTool::Eraser));
        assert!(!tool_has_brush(ActiveTool::Move));
        assert!(!tool_has_brush(ActiveTool::RectangleSelect));
        assert!(!tool_has_brush(ActiveTool::Eyedropper));
    }

    #[test]
    fn every_tool_has_a_hint_or_brush_config() {
        for tool in ActiveTool::ALL {
            assert_eq!(tool_has_brush(tool), options_hint(tool).is_empty());
        }
    }
}
