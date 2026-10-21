//! 「新建文档」表单：尺寸步进 / 预设 / 背景色 + 页脚的「取消 / 创建」。
//!
//! 只往窗口的内容区与页脚挂控件；状态在三个共享格里（选择 / 脏标记 / 结果），
//! 控件节点收进 [`ModalNodes`] 供 `update` 回写文本。

use igui_components::{Button, Component, Flex, NodeRef, Text};
use igui_core::{Edges, NodeId};
use igui_scene::SceneTree;
use igui_theme::{space, Theme, Tone};
use igui_ui::{Align, Justify, MouseFilter};

use super::super::window::FOOTER_BUTTON_MIN_WIDTH;
use super::widgets::{background_swatch, preset_button, stepper_row};
use super::{
    ModalNodes, NewDocumentResult, NewDocumentSpec, BACKGROUNDS, MAX_SIDE, MIN_SIDE, PRESETS,
    SIDE_STEP,
};
use crate::document::Color;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// 建表单时要用的模态共享格（控件回调只能往里写）。
pub(super) struct FormWiring<'a> {
    pub(super) choices: &'a Rc<RefCell<NewDocumentSpec>>,
    pub(super) dirty: &'a Rc<Cell<bool>>,
    pub(super) result: &'a Rc<Cell<Option<NewDocumentResult>>>,
    pub(super) nodes: &'a Rc<ModalNodes>,
}

/// 把尺寸 / 背景色选择控件构建到窗口内容区里（含底部「取消 / 创建」行）。
pub(super) fn build_content(
    tree: &mut SceneTree,
    parent: NodeId,
    footer: NodeId,
    theme: &'static dyn Theme,
    wiring: &FormWiring<'_>,
) {
    let FormWiring {
        choices,
        dirty,
        result,
        nodes,
    } = wiring;
    let width_ref = NodeRef::new();
    let height_ref = NodeRef::new();
    let background_ref = NodeRef::new();

    let size_steppers = Flex::column()
        .gap(space::XS)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(stepper_row(
            theme,
            "宽度",
            &width_ref,
            (*choices).clone(),
            (*dirty).clone(),
            |spec| {
                spec.width = spec.width.saturating_sub(SIDE_STEP).max(MIN_SIDE);
            },
            |spec| {
                spec.width = (spec.width + SIDE_STEP).min(MAX_SIDE);
            },
        ))
        .child(stepper_row(
            theme,
            "高度",
            &height_ref,
            (*choices).clone(),
            (*dirty).clone(),
            |spec| {
                spec.height = spec.height.saturating_sub(SIDE_STEP).max(MIN_SIDE);
            },
            |spec| {
                spec.height = (spec.height + SIDE_STEP).min(MAX_SIDE);
            },
        ));

    let mut presets = Flex::row()
        .wrap(true)
        .gap(space::XXS)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore);
    let mut preset_slots: Vec<NodeRef> = Vec::new();
    for (width, height) in PRESETS {
        let slot = NodeRef::new();
        preset_slots.push(slot.clone());
        presets = presets.child(preset_button(
            theme,
            width,
            height,
            (*choices).clone(),
            (*dirty).clone(),
            &slot,
        ));
    }

    let mut backgrounds = Flex::row()
        .gap(space::XS)
        .padding(Edges::ZERO)
        .align(Align::Center)
        .mouse_filter(MouseFilter::Ignore);
    let mut background_slots: Vec<NodeRef> = Vec::new();
    for (name, color) in BACKGROUNDS {
        let slot = NodeRef::new();
        background_slots.push(slot.clone());
        backgrounds = backgrounds.child(background_swatch(
            theme,
            name,
            color,
            (*choices).clone(),
            (*dirty).clone(),
            &slot,
        ));
    }

    let create_slot = NodeRef::new();
    let cancel_slot = NodeRef::new();
    let create_result = (*result).clone();
    let create_choices = (*choices).clone();
    let cancel_result = (*result).clone();
    let actions = Flex::row()
        .justify(Justify::End)
        .gap(space::SM)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(
            Button::secondary("取消", theme)
                .min_size(FOOTER_BUTTON_MIN_WIDTH, 0.0)
                .on_click(move |_tree, _id| {
                    cancel_result.set(Some(NewDocumentResult::Cancel));
                })
                .ref_(&cancel_slot),
        )
        .child(
            Button::primary("创建", theme)
                .min_size(FOOTER_BUTTON_MIN_WIDTH, 0.0)
                .on_click(move |_tree, _id| {
                    create_result.set(Some(NewDocumentResult::Create(*create_choices.borrow())));
                })
                .ref_(&create_slot),
        );

    let page = Flex::column()
        .gap(space::LG)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(
            Text::small("选择画布尺寸和背景色；创建后在同一个窗口里编辑。", theme)
                .tone(Tone::Muted),
        )
        .child(Text::subheading("尺寸", theme))
        .child(size_steppers)
        .child(presets)
        .child(
            Text::caption("大尺寸（4K / A4 300dpi）在 CPU 合成下会明显变慢。", theme)
                .tone(Tone::Muted),
        )
        .child(Text::subheading("背景色", theme))
        .child(backgrounds)
        .child(
            Text::caption("", theme)
                .tone(Tone::Muted)
                .ref_(&background_ref),
        );

    tree.add_child(parent, page);
    tree.add_child(footer, actions);

    nodes.width.set(width_ref.get());
    nodes.height.set(height_ref.get());
    nodes.background.set(background_ref.get());
    nodes.create.set(create_slot.get());
    nodes.cancel.set(cancel_slot.get());
    *nodes.presets.borrow_mut() = preset_slots.iter().filter_map(|slot| slot.get()).collect();
    *nodes.backgrounds.borrow_mut() = background_slots
        .iter()
        .filter_map(|slot| slot.get())
        .collect();
}

/// 背景色的名字（状态行显示用）。
pub(super) fn background_name(color: Color) -> &'static str {
    BACKGROUNDS
        .iter()
        .find(|(_, candidate)| *candidate == color)
        .map(|(name, _)| *name)
        .unwrap_or("自定义")
}
