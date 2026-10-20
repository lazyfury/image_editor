//! 工具选项栏：菜单栏下面的一行，显示当前工具的配置。
//!
//! 画笔 / 橡皮显示笔刷预设、大小、硬度、不透明度（`−` / `+` 按钮）；钢笔显示
//! 锚点数 + 「建立选区 / 填充 / 描边 / 清除」；其余工具显示一句操作提示。按钮只写一个共享请求格（[`BrushAdjust`]），真正的改动由
//! [`EditorView::update`](crate::ui::EditorView) 统一执行 —— 回调拿不到
//! `&mut self`，和菜单 / 文件面板一个套路。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use igui_components::{Button, Component, Flex, NodeRef, Text};
use igui_core::{Color, Edges};
use igui_theme::{radius, space, SurfaceLevel, Theme, Tone};
use igui_ui::{Align, MouseFilter, SurfaceStyle};

use crate::app::state::ActiveTool;
use crate::document::Gradient;
use crate::ui::gradient_editor;

/// 选项栏里的一次笔刷调整。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrushAdjust {
    SizeDown,
    SizeUp,
    HardnessDown,
    HardnessUp,
    SmoothDown,
    SmoothUp,
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

/// 渐变工具选项栏的共享状态 / 请求。
pub struct GradientOptions {
    pub gradient: Rc<RefCell<Gradient>>,
    pub reverse: Rc<Cell<bool>>,
    pub request: Rc<Cell<Option<GradientAdjust>>>,
    /// 点「编辑…」置位；`update` 取走后打开多节点编辑器。
    pub edit: Rc<Cell<bool>>,
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
}

/// 工具选项栏。`request` 收下 `−` / `+` 的调整请求；`buttons` 收集按钮节点，
/// 测试与自检靠它们真的点一下。
#[allow(clippy::too_many_arguments)]
pub fn options_bar(
    theme: &'static dyn Theme,
    request: Rc<Cell<Option<BrushAdjust>>>,
    // 点了「预设」触发按钮：写入按钮节点 id，`update` 取走后开下拉。
    preset_open: Rc<Cell<Option<igui_core::NodeId>>>,
    preset_label: &str,
    hard_state: Rc<Cell<bool>>,
    square_state: Rc<Cell<bool>>,
    symmetry_state: Rc<Cell<crate::tools::Symmetry>>,
    pen_request: Rc<Cell<Option<PenAction>>>,
    bucket_request: Rc<Cell<Option<BucketAdjust>>>,
    contiguous_state: Rc<Cell<bool>>,
    gradient: GradientOptions,
    refs: &OptionsRefs,
    buttons: &mut Vec<(BrushAdjust, NodeRef)>,
    toggles: &mut Vec<(BrushToggle, NodeRef)>,
    pen_buttons: &mut Vec<(PenAction, NodeRef)>,
    bucket_buttons: &mut Vec<(BucketAdjust, NodeRef)>,
    bucket_toggles: &mut Vec<(BucketToggle, NodeRef)>,
    gradient_buttons: &mut Vec<(GradientAdjust, NodeRef)>,
) -> impl Component {
    let brush = Flex::row()
        .align(Align::Center)
        .gap(space::XS)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(
            Button::ghost("预设 ▾", theme)
                .on_click(move |_tree, id| preset_open.set(Some(id)))
                .ref_(&refs.preset_button),
        )
        .child(Text::small(preset_label, theme).ref_(&refs.preset))
        .child(Text::caption("大小", theme).tone(Tone::Muted))
        .child(step_button(
            theme,
            "−",
            request.clone(),
            BrushAdjust::SizeDown,
            buttons,
        ))
        .child(Text::small("", theme).ref_(&refs.size))
        .child(step_button(
            theme,
            "+",
            request.clone(),
            BrushAdjust::SizeUp,
            buttons,
        ))
        .child(Text::caption("硬度", theme).tone(Tone::Muted))
        .child(step_button(
            theme,
            "−",
            request.clone(),
            BrushAdjust::HardnessDown,
            buttons,
        ))
        .child(Text::small("", theme).ref_(&refs.hardness))
        .child(step_button(
            theme,
            "+",
            request.clone(),
            BrushAdjust::HardnessUp,
            buttons,
        ))
        .child(Text::caption("平滑", theme).tone(Tone::Muted))
        .child(step_button(
            theme,
            "−",
            request.clone(),
            BrushAdjust::SmoothDown,
            buttons,
        ))
        .child(Text::small("", theme).ref_(&refs.smooth))
        .child(step_button(
            theme,
            "+",
            request.clone(),
            BrushAdjust::SmoothUp,
            buttons,
        ))
        .child(Text::caption("不透明度", theme).tone(Tone::Muted))
        .child(step_button(
            theme,
            "−",
            request.clone(),
            BrushAdjust::OpacityDown,
            buttons,
        ))
        .child(Text::small("", theme).ref_(&refs.opacity))
        .child(step_button(
            theme,
            "+",
            request.clone(),
            BrushAdjust::OpacityUp,
            buttons,
        ))
        .child(Text::caption("对称", theme).tone(Tone::Muted))
        .child(
            Button::ghost("切换", theme)
                .on_click({
                    let state = symmetry_state.clone();
                    move |_tree, _id| state.set(state.get().next())
                })
                .ref_(&refs.symmetry_button),
        )
        .child(Text::small("关", theme).ref_(&refs.symmetry))
        .child(toggle_button(theme, BrushToggle::Hard, hard_state, toggles))
        .child(toggle_button(
            theme,
            BrushToggle::Square,
            square_state,
            toggles,
        ));

    let pen = {
        let mut row = Flex::row()
            .align(Align::Center)
            .gap(space::XS)
            .padding(Edges::ZERO)
            .mouse_filter(MouseFilter::Ignore)
            .child(Text::small("", theme).ref_(&refs.pen));
        for action in PenAction::ALL {
            let slot = NodeRef::new();
            pen_buttons.push((action, slot.clone()));
            let request = pen_request.clone();
            row = row.child(
                Button::ghost(action.label(), theme)
                    .on_click(move |_tree, _id| request.set(Some(action)))
                    .ref_(&slot),
            );
        }
        row
    };

    let bucket = Flex::row()
        .align(Align::Center)
        .gap(space::XS)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::caption("容差", theme).tone(Tone::Muted))
        .child(bucket_step_button(
            theme,
            "−",
            bucket_request.clone(),
            BucketAdjust::ToleranceDown,
            bucket_buttons,
        ))
        .child(Text::small("", theme).ref_(&refs.tolerance))
        .child(bucket_step_button(
            theme,
            "+",
            bucket_request,
            BucketAdjust::ToleranceUp,
            bucket_buttons,
        ))
        .child(bucket_toggle_button(
            theme,
            BucketToggle::Contiguous,
            contiguous_state,
            bucket_toggles,
        ));

    let edit_request = gradient.edit.clone();
    let gradient_row = Flex::row()
        .align(Align::Center)
        .gap(space::XS)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::caption("类型", theme).tone(Tone::Muted))
        .child(gradient_step_button(
            theme,
            "−",
            gradient.request.clone(),
            GradientAdjust::KindPrev,
            gradient_buttons,
        ))
        .child(Text::small("", theme).ref_(&refs.gradient_kind))
        .child(gradient_step_button(
            theme,
            "+",
            gradient.request.clone(),
            GradientAdjust::KindNext,
            gradient_buttons,
        ))
        .child(gradient_editor::gradient_preview(
            theme,
            gradient.gradient.clone(),
            &refs.gradient_preview,
        ))
        .child(
            Button::ghost("编辑…", theme)
                .on_click(move |_tree, _id| edit_request.set(true))
                .ref_(&refs.gradient_edit),
        )
        .child(gradient_toggle_button(theme, "反向", gradient.reverse));

    Flex::row()
        .align(Align::Center)
        .gap(space::MD)
        .padding(Edges::symmetric(space::SM, space::XXS))
        .background(theme.surface(SurfaceLevel::Raised))
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::small("", theme).ref_(&refs.tool))
        .child(brush.ref_(&refs.brush))
        .child(pen.ref_(&refs.pen_panel))
        .child(bucket.ref_(&refs.bucket))
        .child(gradient_row.ref_(&refs.gradient))
        .child(Text::caption("", theme).tone(Tone::Muted).ref_(&refs.hint))
        .ref_(&refs.panel)
}

/// 非画笔工具的提示文字（画笔 / 橡皮为空，因为它们的配置就在左边）。
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
        ActiveTool::Brush | ActiveTool::Eraser => "",
    }
}

/// 画笔 / 橡皮才显示笔刷配置。
pub const fn tool_has_brush(tool: ActiveTool) -> bool {
    matches!(tool, ActiveTool::Brush | ActiveTool::Eraser)
}

fn step_button(
    theme: &'static dyn Theme,
    label: &str,
    request: Rc<Cell<Option<BrushAdjust>>>,
    adjust: BrushAdjust,
    buttons: &mut Vec<(BrushAdjust, NodeRef)>,
) -> impl Component {
    let slot = NodeRef::new();
    buttons.push((adjust, slot.clone()));
    Button::ghost(label, theme)
        .on_click(move |_tree, _id| request.set(Some(adjust)))
        .ref_(&slot)
}

/// 一个布尔开关：点击翻转共享状态，`dynamic_background` 直接读它显示激活态。
fn toggle_button(
    theme: &'static dyn Theme,
    toggle: BrushToggle,
    state: Rc<Cell<bool>>,
    toggles: &mut Vec<(BrushToggle, NodeRef)>,
) -> impl Component {
    let slot = NodeRef::new();
    toggles.push((toggle, slot.clone()));
    let click_state = state.clone();
    Button::ghost(toggle.label(), theme)
        .on_click(move |_tree, _id| click_state.set(!click_state.get()))
        .dynamic_background(move |interact| active_background(theme, state.get(), interact))
        .ref_(&slot)
}

fn bucket_step_button(
    theme: &'static dyn Theme,
    label: &str,
    request: Rc<Cell<Option<BucketAdjust>>>,
    adjust: BucketAdjust,
    buttons: &mut Vec<(BucketAdjust, NodeRef)>,
) -> impl Component {
    let slot = NodeRef::new();
    buttons.push((adjust, slot.clone()));
    Button::ghost(label, theme)
        .on_click(move |_tree, _id| request.set(Some(adjust)))
        .ref_(&slot)
}

fn bucket_toggle_button(
    theme: &'static dyn Theme,
    toggle: BucketToggle,
    state: Rc<Cell<bool>>,
    toggles: &mut Vec<(BucketToggle, NodeRef)>,
) -> impl Component {
    let slot = NodeRef::new();
    toggles.push((toggle, slot.clone()));
    let click_state = state.clone();
    Button::ghost(toggle.label(), theme)
        .on_click(move |_tree, _id| click_state.set(!click_state.get()))
        .dynamic_background(move |interact| active_background(theme, state.get(), interact))
        .ref_(&slot)
}

fn gradient_step_button(
    theme: &'static dyn Theme,
    label: &str,
    request: Rc<Cell<Option<GradientAdjust>>>,
    adjust: GradientAdjust,
    buttons: &mut Vec<(GradientAdjust, NodeRef)>,
) -> impl Component {
    let slot = NodeRef::new();
    buttons.push((adjust, slot.clone()));
    Button::ghost(label, theme)
        .on_click(move |_tree, _id| request.set(Some(adjust)))
        .ref_(&slot)
}

fn gradient_toggle_button(
    theme: &'static dyn Theme,
    label: &str,
    state: Rc<Cell<bool>>,
) -> impl Component {
    let click_state = state.clone();
    Button::ghost(label, theme)
        .on_click(move |_tree, _id| click_state.set(!click_state.get()))
        .dynamic_background(move |interact| active_background(theme, state.get(), interact))
}

/// 激活态底色 / 悬停态底色 / 透明。
fn active_background(
    theme: &'static dyn Theme,
    active: bool,
    interact: igui_ui::InteractState,
) -> SurfaceStyle {
    if active {
        SurfaceStyle::new(theme.palette().selection).radius(radius::SM)
    } else if interact.hovered || interact.pressed {
        SurfaceStyle::new(theme.palette().surface_hover).radius(radius::SM)
    } else {
        SurfaceStyle::new(Color::TRANSPARENT)
    }
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
