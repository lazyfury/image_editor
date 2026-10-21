//! 选项栏的四行：画笔 / 钢笔 / 油漆桶 / 渐变。
//!
//! 每行一个 builder，外加它们共用的两种按钮（`−` / `+` 步进、布尔开关）。
//! 共享输入在 [`BarInputs`]，按钮节点写进 [`OptionSlots`] 里对应的那一组。

use super::*;

/// 画笔 / 橡皮这一行：预设、大小、硬度、平滑、间距、不透明度、对称、两个开关。
pub(super) fn brush_row(
    theme: &'static dyn Theme,
    preset_label: &str,
    inputs: &BarInputs,
    refs: &OptionsRefs,
    buttons: &mut Vec<(BrushAdjust, NodeRef)>,
    toggles: &mut Vec<(BrushToggle, NodeRef)>,
) -> Flex {
    let preset_intents = inputs.intents.clone();
    Flex::row()
        .align(Align::Center)
        .gap(space::XS)
        .min_size(0.0, OPTIONS_BAR_HEIGHT)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(
            Button::ghost("预设 ▾", theme)
                .on_click(move |_tree, id| preset_intents.push(Intent::OpenBrushPresets(id)))
                .ref_(&refs.preset_button),
        )
        .child(Text::small(preset_label, theme).ref_(&refs.preset))
        .child(Text::caption("大小", theme).tone(Tone::Muted))
        .child(step_button(
            theme,
            "−",
            inputs.intents.clone(),
            BrushAdjust::SizeDown,
            buttons,
        ))
        .child(Text::small("", theme).ref_(&refs.size))
        .child(step_button(
            theme,
            "+",
            inputs.intents.clone(),
            BrushAdjust::SizeUp,
            buttons,
        ))
        .child(Text::caption("硬度", theme).tone(Tone::Muted))
        .child(step_button(
            theme,
            "−",
            inputs.intents.clone(),
            BrushAdjust::HardnessDown,
            buttons,
        ))
        .child(Text::small("", theme).ref_(&refs.hardness))
        .child(step_button(
            theme,
            "+",
            inputs.intents.clone(),
            BrushAdjust::HardnessUp,
            buttons,
        ))
        .child(Text::caption("平滑", theme).tone(Tone::Muted))
        .child(step_button(
            theme,
            "−",
            inputs.intents.clone(),
            BrushAdjust::SmoothDown,
            buttons,
        ))
        .child(Text::small("", theme).ref_(&refs.smooth))
        .child(step_button(
            theme,
            "+",
            inputs.intents.clone(),
            BrushAdjust::SmoothUp,
            buttons,
        ))
        .child(Text::caption("间距", theme).tone(Tone::Muted))
        .child(step_button(
            theme,
            "−",
            inputs.intents.clone(),
            BrushAdjust::SpacingDown,
            buttons,
        ))
        .child(Text::small("", theme).ref_(&refs.spacing))
        .child(step_button(
            theme,
            "+",
            inputs.intents.clone(),
            BrushAdjust::SpacingUp,
            buttons,
        ))
        .child(Text::caption("不透明度", theme).tone(Tone::Muted))
        .child(step_button(
            theme,
            "−",
            inputs.intents.clone(),
            BrushAdjust::OpacityDown,
            buttons,
        ))
        .child(Text::small("", theme).ref_(&refs.opacity))
        .child(step_button(
            theme,
            "+",
            inputs.intents.clone(),
            BrushAdjust::OpacityUp,
            buttons,
        ))
        .child(Text::caption("对称", theme).tone(Tone::Muted))
        .child(
            Button::ghost("切换", theme)
                .on_click({
                    let state = inputs.symmetry.clone();
                    move |_tree, _id| state.set(state.get().next())
                })
                .ref_(&refs.symmetry_button),
        )
        .child(Text::small("关", theme).ref_(&refs.symmetry))
        .child(toggle_button(
            theme,
            BrushToggle::Hard,
            inputs.hard.clone(),
            toggles,
        ))
        .child(toggle_button(
            theme,
            BrushToggle::Square,
            inputs.square.clone(),
            toggles,
        ))
}

/// 钢笔这一行：路径摘要 + 选区 / 填充 / 描边 / 清除。
pub(super) fn pen_row(
    theme: &'static dyn Theme,
    intents: &Intents,
    refs: &OptionsRefs,
    pen_buttons: &mut Vec<(PenAction, NodeRef)>,
) -> Flex {
    let mut row = Flex::row()
        .align(Align::Center)
        .gap(space::XS)
        .min_size(0.0, OPTIONS_BAR_HEIGHT)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::small("", theme).ref_(&refs.pen));
    for action in PenAction::ALL {
        let slot = NodeRef::new();
        pen_buttons.push((action, slot.clone()));
        let intents = intents.clone();
        row = row.child(
            Button::ghost(action.label(), theme)
                .on_click(move |_tree, _id| intents.push(Intent::Pen(action)))
                .ref_(&slot),
        );
    }
    row
}

/// 油漆桶 / 魔棒这一行：容差 `−` / `+` + 「连续」开关。
pub(super) fn bucket_row(
    theme: &'static dyn Theme,
    inputs: &BarInputs,
    refs: &OptionsRefs,
    bucket_buttons: &mut Vec<(BucketAdjust, NodeRef)>,
    bucket_toggles: &mut Vec<(BucketToggle, NodeRef)>,
) -> Flex {
    Flex::row()
        .align(Align::Center)
        .gap(space::XS)
        .min_size(0.0, OPTIONS_BAR_HEIGHT)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::caption("容差", theme).tone(Tone::Muted))
        .child(bucket_step_button(
            theme,
            "−",
            inputs.intents.clone(),
            BucketAdjust::ToleranceDown,
            bucket_buttons,
        ))
        .child(Text::small("", theme).ref_(&refs.tolerance))
        .child(bucket_step_button(
            theme,
            "+",
            inputs.intents.clone(),
            BucketAdjust::ToleranceUp,
            bucket_buttons,
        ))
        .child(bucket_toggle_button(
            theme,
            BucketToggle::Contiguous,
            inputs.contiguous.clone(),
            bucket_toggles,
        ))
}

/// 渐变这一行：类型 `−` / `+`、色带预览、「编辑…」、「反向」。
pub(super) fn gradient_row(
    theme: &'static dyn Theme,
    gradient: &GradientOptions,
    refs: &OptionsRefs,
    gradient_buttons: &mut Vec<(GradientAdjust, NodeRef)>,
) -> Flex {
    let edit_intents = gradient.intents.clone();
    Flex::row()
        .align(Align::Center)
        .gap(space::XS)
        .min_size(0.0, OPTIONS_BAR_HEIGHT)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::caption("类型", theme).tone(Tone::Muted))
        .child(gradient_step_button(
            theme,
            "−",
            gradient.intents.clone(),
            GradientAdjust::KindPrev,
            gradient_buttons,
        ))
        .child(Text::small("", theme).ref_(&refs.gradient_kind))
        .child(gradient_step_button(
            theme,
            "+",
            gradient.intents.clone(),
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
                .on_click(move |_tree, _id| edit_intents.push(Intent::OpenGradientEditor))
                .ref_(&refs.gradient_edit),
        )
        .child(gradient_toggle_button(
            theme,
            "反向",
            gradient.reverse.clone(),
        ))
}

/// 一个 `−` / `+` 步进按钮：点击只塞意图，值由 `update` 应用。
fn step_button(
    theme: &'static dyn Theme,
    label: &str,
    intents: Intents,
    adjust: BrushAdjust,
    buttons: &mut Vec<(BrushAdjust, NodeRef)>,
) -> impl Component {
    let slot = NodeRef::new();
    buttons.push((adjust, slot.clone()));
    Button::ghost(label, theme)
        .on_click(move |_tree, _id| intents.push(Intent::Brush(adjust)))
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
    intents: Intents,
    adjust: BucketAdjust,
    buttons: &mut Vec<(BucketAdjust, NodeRef)>,
) -> impl Component {
    let slot = NodeRef::new();
    buttons.push((adjust, slot.clone()));
    Button::ghost(label, theme)
        .on_click(move |_tree, _id| intents.push(Intent::Bucket(adjust)))
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
    intents: Intents,
    adjust: GradientAdjust,
    buttons: &mut Vec<(GradientAdjust, NodeRef)>,
) -> impl Component {
    let slot = NodeRef::new();
    buttons.push((adjust, slot.clone()));
    Button::ghost(label, theme)
        .on_click(move |_tree, _id| intents.push(Intent::Gradient(adjust)))
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
