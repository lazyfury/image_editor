//! 「新建文档」模态窗口：选画布尺寸 / 背景色。
//!
//! 由首页的「新建项目」打开。它现在是一扇**模拟桌面的浮动窗口**
//! （[`crate::ui::window::Window`]）：标题栏可拖动移动，四条边 / 角可拖动缩放，
//! 右上角关闭按钮，点窗口外或 Esc 取消。窗口在宿主
//! （[`crate::app::application`]）的底层视图之上 `layout` / `paint`，有它时输入
//! 全部交给它。点「创建」把选择通过共享格交回宿主，宿主把它变成同一个窗口里的
//! 一个新 [`EditorView`](crate::ui::EditorView)。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use igui_components::{Button, Component, Flex, NodeRef, Text};
use igui_core::{
    Color as UiColor, Edges, EventResult, InputEvent, NodeId, Rect, Size, Vec2, ViewportSize,
};
use igui_render::PaintContext;
use igui_scene::SceneTree;
use igui_theme::{radius, space, TextSize, Theme, Tone};
use igui_ui::{Align, Justify, MouseFilter, SurfaceStyle, TextMeasurer};

use super::window::Window;
use crate::canvas::{CHECKER_DARK, CHECKER_LIGHT};
use crate::document::Color;

/// 「新建项目」选出来的默认值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewDocumentSpec {
    pub width: u32,
    pub height: u32,
    pub background: Color,
}

impl Default for NewDocumentSpec {
    fn default() -> Self {
        Self {
            width: 32,
            height: 32,
            background: Color::TRANSPARENT,
        }
    }
}

/// 侧边的最小 / 最大边长（逻辑像素即文档像素）。步进按钮每次 ±1，所以尺寸不受
/// 倍数限制，任意边长都能改到。
const MIN_SIDE: u32 = 1;
const MAX_SIDE: u32 = 2048;
const SIDE_STEP: u32 = 1;

/// 预设尺寸（仍保留，用来快速跳到常见大小）。
const PRESETS: [(u32, u32); 6] = [
    (16, 16),
    (32, 32),
    (64, 64),
    (128, 128),
    (256, 256),
    (512, 512),
];

/// 背景色预设（名字，颜色）。第一个是透明。
const BACKGROUNDS: [(&str, Color); 4] = [
    ("透明", Color::TRANSPARENT),
    ("白色", Color::WHITE),
    ("黑色", Color::BLACK),
    ("红色", Color::RED),
];

/// 初始窗口尺寸 / 拖动缩放下限（逻辑像素）。
const WINDOW_SIZE: Vec2 = Vec2::new(460.0, 420.0);
const WINDOW_MIN: Vec2 = Vec2::new(380.0, 320.0);

/// 「新建文档」浮层给出的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewDocumentResult {
    Create(NewDocumentSpec),
    Cancel,
}

/// 内容构建时记录下来的控件节点：宿主据此把选择变化同步到标签。
#[derive(Default)]
struct ModalNodes {
    width: Cell<Option<NodeId>>,
    height: Cell<Option<NodeId>>,
    background: Cell<Option<NodeId>>,
    presets: RefCell<Vec<NodeId>>,
    backgrounds: RefCell<Vec<NodeId>>,
    /// 「创建 / 取消」按钮；测试真的点它们。
    #[allow(dead_code)]
    create: Cell<Option<NodeId>>,
    #[allow(dead_code)]
    cancel: Cell<Option<NodeId>>,
}

/// 「新建文档」模态窗口：把选择控件构建进一扇浮动 [`Window`]。
pub struct NewDocumentModal {
    /// 当前选择（共享格，回调只能往里写）。
    choices: Rc<RefCell<NewDocumentSpec>>,
    /// 有改动、还没同步到标签。
    dirty: Rc<Cell<bool>>,
    /// 创建 / 取消的结果；宿主取走一次。
    result: Rc<Cell<Option<NewDocumentResult>>>,
    nodes: Rc<ModalNodes>,
    window: Window,
}

impl NewDocumentModal {
    pub fn new(theme: &'static dyn Theme) -> Self {
        let choices: Rc<RefCell<NewDocumentSpec>> =
            Rc::new(RefCell::new(NewDocumentSpec::default()));
        let dirty: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        let result: Rc<Cell<Option<NewDocumentResult>>> = Rc::new(Cell::new(None));
        let nodes = Rc::new(ModalNodes::default());

        let window = Window::new(theme, "新建文档", WINDOW_SIZE, WINDOW_MIN, {
            let choices = choices.clone();
            let dirty = dirty.clone();
            let result = result.clone();
            let nodes = nodes.clone();
            move |tree, content, footer| {
                build_content(
                    tree, content, footer, theme, &choices, &dirty, &result, &nodes,
                );
            }
        });

        Self {
            choices,
            dirty,
            result,
            nodes,
            window,
        }
    }

    /// 用后端真实字体度量排版。
    pub fn set_text_measurer(&mut self, measurer: Rc<dyn TextMeasurer>) {
        self.window.set_text_measurer(measurer);
    }

    /// 把共享格里的选择写进标签（只在控件改动后）。
    pub fn update(&mut self) {
        if !self.dirty.replace(false) {
            return;
        }
        let spec = *self.choices.borrow();
        if let Some(id) = self.nodes.width.get() {
            self.window.set_text(id, format!("{} px", spec.width));
        }
        if let Some(id) = self.nodes.height.get() {
            self.window.set_text(id, format!("{} px", spec.height));
        }
        if let Some(id) = self.nodes.background.get() {
            self.window
                .set_text(id, format!("背景：{}", background_name(spec.background)));
        }
    }

    /// 创建 / 取消的结果；取走一次。窗口的关闭请求（关闭按钮 / Esc / 点窗口外）
    /// 在这里折算成取消。
    pub fn take_result(&self) -> Option<NewDocumentResult> {
        if self.result.get().is_none() && self.window.take_close_request() {
            self.result.set(Some(NewDocumentResult::Cancel));
        }
        self.result.take()
    }

    pub fn event(&mut self, event: &InputEvent) -> EventResult {
        self.window.event(event)
    }

    pub fn layout(&mut self, viewport: ViewportSize) {
        self.window.layout(viewport);
    }

    pub fn paint(&self, ctx: &mut PaintContext) {
        self.window.paint(ctx);
    }

    /// 当前选择（测试用）。
    #[cfg(test)]
    fn spec(&self) -> NewDocumentSpec {
        *self.choices.borrow()
    }

    #[cfg(test)]
    fn accept(&self) {
        self.result
            .set(Some(NewDocumentResult::Create(*self.choices.borrow())));
    }

    #[cfg(test)]
    fn cancel(&self) {
        self.result.set(Some(NewDocumentResult::Cancel));
    }

    #[cfg(test)]
    fn tree(&self) -> &SceneTree {
        self.window.tree()
    }

    #[cfg(test)]
    fn rect(&self) -> Rect {
        self.window.rect()
    }
}

/// 把尺寸 / 背景色选择控件构建到窗口内容区里（含底部「取消 / 创建」行）。
fn build_content(
    tree: &mut SceneTree,
    parent: NodeId,
    footer: NodeId,
    theme: &'static dyn Theme,
    choices: &Rc<RefCell<NewDocumentSpec>>,
    dirty: &Rc<Cell<bool>>,
    result: &Rc<Cell<Option<NewDocumentResult>>>,
    nodes: &Rc<ModalNodes>,
) {
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
            choices.clone(),
            dirty.clone(),
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
            choices.clone(),
            dirty.clone(),
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
            choices.clone(),
            dirty.clone(),
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
            choices.clone(),
            dirty.clone(),
            &slot,
        ));
    }

    let create_slot = NodeRef::new();
    let cancel_slot = NodeRef::new();
    let create_result = result.clone();
    let create_choices = choices.clone();
    let cancel_result = result.clone();
    let actions = Flex::row()
        .justify(Justify::End)
        .gap(space::SM)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(
            Button::secondary("取消", theme)
                .on_click(move |_tree, _id| {
                    cancel_result.set(Some(NewDocumentResult::Cancel));
                })
                .ref_(&cancel_slot),
        )
        .child(
            Button::primary("创建", theme)
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
fn background_name(color: Color) -> &'static str {
    BACKGROUNDS
        .iter()
        .find(|(_, candidate)| *candidate == color)
        .map(|(name, _)| *name)
        .unwrap_or("自定义")
}

/// 一行「标签  [−]  数值  [+]」。
fn stepper_row(
    theme: &'static dyn Theme,
    label: &'static str,
    value_ref: &NodeRef,
    choices: Rc<RefCell<NewDocumentSpec>>,
    dirty: Rc<Cell<bool>>,
    less: fn(&mut NewDocumentSpec),
    more: fn(&mut NewDocumentSpec),
) -> impl Component {
    Flex::row()
        .gap(space::SM)
        .padding(Edges::ZERO)
        .align(Align::Center)
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::small(label, theme))
        .child(step_button(
            theme,
            "−",
            choices.clone(),
            dirty.clone(),
            less,
        ))
        .child(Text::small("", theme).min_size(72.0, 0.0).ref_(value_ref))
        .child(step_button(theme, "+", choices.clone(), dirty, more))
}

fn step_button(
    theme: &'static dyn Theme,
    label: &'static str,
    choices: Rc<RefCell<NewDocumentSpec>>,
    dirty: Rc<Cell<bool>>,
    apply: fn(&mut NewDocumentSpec),
) -> Button {
    Button::secondary(label, theme).on_click(move |_tree, _id| {
        apply(&mut choices.borrow_mut());
        dirty.set(true);
    })
}

fn preset_button(
    theme: &'static dyn Theme,
    width: u32,
    height: u32,
    choices: Rc<RefCell<NewDocumentSpec>>,
    dirty: Rc<Cell<bool>>,
    slot: &NodeRef,
) -> impl Component {
    Button::secondary(format!("{width}×{height}"), theme)
        .font_size(TextSize::Small.px())
        .on_click(move |_tree, _id| {
            let mut spec = choices.borrow_mut();
            spec.width = width;
            spec.height = height;
            dirty.set(true);
        })
        .ref_(slot)
}

/// 一个背景色块：点击选中，选中的描一圈前景边。透明色用棋盘格预览。
fn background_swatch(
    theme: &'static dyn Theme,
    _name: &'static str,
    color: Color,
    choices: Rc<RefCell<NewDocumentSpec>>,
    dirty: Rc<Cell<bool>>,
    slot: &NodeRef,
) -> impl Component {
    let ui = to_ui(color);
    let transparent = color.a == 0;
    let selected = choices.clone();
    let clicked = choices;
    Flex::new()
        .padding(Edges::ZERO)
        .min_size(28.0, 28.0)
        .on_click(move |_tree, _id| {
            clicked.borrow_mut().background = color;
            dirty.set(true);
        })
        .dynamic_background(move |interact| {
            let is_selected = selected.borrow().background == color;
            let style = SurfaceStyle::new(ui).radius(radius::SM);
            if is_selected || interact.hovered {
                style.border(theme.palette().foreground)
            } else {
                style.border(theme.palette().border)
            }
        })
        // 棋盘格画在色块前景（内缩 1px 给边框让位），跟画布的透明预览一致。
        .foreground(move |ctx, rect, _| {
            if transparent {
                paint_checkerboard(ctx, rect);
            }
        })
        .ref_(slot)
}

/// 棋盘格格子边长（逻辑像素）。
const CHECKER_CELL: f32 = 6.0;

/// 在 `rect` 里画透明棋盘格（内缩 1px，露出边框）。
fn paint_checkerboard(ctx: &mut PaintContext, rect: Rect) {
    let inner = Rect::from_min_size(
        Vec2::new(rect.left() + 1.0, rect.top() + 1.0),
        Size::new(
            (rect.size.width - 2.0).max(0.0),
            (rect.size.height - 2.0).max(0.0),
        ),
    );
    if inner.size.width <= 0.0 || inner.size.height <= 0.0 {
        return;
    }
    let cols = (inner.size.width / CHECKER_CELL).ceil() as i32;
    let rows = (inner.size.height / CHECKER_CELL).ceil() as i32;
    for row in 0..rows {
        for col in 0..cols {
            let x = inner.left() + col as f32 * CHECKER_CELL;
            let y = inner.top() + row as f32 * CHECKER_CELL;
            let w = CHECKER_CELL.min(inner.right() - x);
            let h = CHECKER_CELL.min(inner.bottom() - y);
            if w <= 0.0 || h <= 0.0 {
                continue;
            }
            let color = if (row + col) % 2 == 0 {
                CHECKER_LIGHT
            } else {
                CHECKER_DARK
            };
            ctx.fill_rect(
                Rect::from_min_size(Vec2::new(x, y), Size::new(w, h)),
                to_ui(color),
            );
        }
    }
}

/// 文档色（8 位 RGBA）-> UI 色（0..1）。
fn to_ui(color: Color) -> UiColor {
    UiColor::new(
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
        color.a as f32 / 255.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use igui_core::{PointerButton, Size};
    use igui_render::DrawCommand;

    fn viewport() -> ViewportSize {
        ViewportSize::new(Size::new(700.0, 640.0))
    }

    fn modal() -> NewDocumentModal {
        let mut modal = NewDocumentModal::new(crate::theme::editor_theme(false));
        modal.layout(viewport());
        modal
    }

    fn click(modal: &mut NewDocumentModal, position: Vec2) {
        modal.event(&InputEvent::PointerDown {
            position,
            button: PointerButton::Left,
        });
        modal.event(&InputEvent::PointerUp {
            position,
            button: PointerButton::Left,
        });
        modal.update();
        modal.layout(viewport());
    }

    fn center(modal: &NewDocumentModal, id: NodeId) -> Option<Vec2> {
        igui_ui::control(modal.tree(), id).map(|control| control.rect.center())
    }

    fn rects(modal: &NewDocumentModal, ids: &RefCell<Vec<NodeId>>) -> Vec<Rect> {
        ids.borrow()
            .iter()
            .filter_map(|id| igui_ui::control(modal.tree(), *id).map(|control| control.rect))
            .collect()
    }

    /// 拖动右边，把窗口缩到最小宽度，用来逼出预设换行。
    fn resize_to_min(modal: &mut NewDocumentModal) {
        let rect = modal.rect();
        let grab = Vec2::new(rect.right() - 2.0, rect.center().y);
        modal.event(&InputEvent::PointerDown {
            position: grab,
            button: PointerButton::Left,
        });
        modal.event(&InputEvent::PointerMove {
            position: grab - Vec2::new(10_000.0, 0.0),
        });
        modal.event(&InputEvent::PointerUp {
            position: grab - Vec2::new(10_000.0, 0.0),
            button: PointerButton::Left,
        });
        modal.layout(viewport());
    }

    #[test]
    fn the_default_spec_is_32_transparent() {
        assert_eq!(
            NewDocumentSpec::default(),
            NewDocumentSpec {
                width: 32,
                height: 32,
                background: Color::TRANSPARENT,
            }
        );
    }

    #[test]
    fn the_window_is_centered_and_keeps_a_minimum_size() {
        let modal = modal();
        let rect = modal.rect();
        let offset = rect.center() - viewport().logical_rect().center();
        assert!(
            offset.x.abs() < 2.0 && offset.y.abs() < 2.0,
            "窗口应大致居中：{offset:?}"
        );
        assert!(rect.size.width >= WINDOW_MIN.x, "rect = {rect:?}");
        assert!(rect.size.height >= WINDOW_MIN.y, "rect = {rect:?}");
    }

    /// 拖动标题栏能移动窗口，且内容不会被挪出窗口。
    #[test]
    fn dragging_the_title_bar_moves_the_modal() {
        let mut modal = modal();
        let before = modal.rect();
        let grab = Vec2::new(before.center().x, before.top() + 10.0);
        modal.event(&InputEvent::PointerDown {
            position: grab,
            button: PointerButton::Left,
        });
        modal.event(&InputEvent::PointerMove {
            position: grab + Vec2::new(-60.0, 40.0),
        });
        modal.event(&InputEvent::PointerUp {
            position: grab + Vec2::new(-60.0, 40.0),
            button: PointerButton::Left,
        });
        modal.layout(viewport());

        let after = modal.rect();
        assert_eq!(after.origin, before.origin + Vec2::new(-60.0, 40.0));
        assert_eq!(after.size, before.size);
    }

    /// 缩到最小后，预设换行且不压住背景色那一行。
    #[test]
    fn a_wrapped_preset_row_does_not_overlap_the_background_row() {
        let mut modal = modal();
        resize_to_min(&mut modal);

        let presets = rects(&modal, &modal.nodes.presets);
        let first_row = presets.first().map(|rect| rect.top()).unwrap_or(0.0);
        let rows = presets
            .iter()
            .filter(|rect| (rect.top() - first_row).abs() > 0.5)
            .count();
        assert!(rows >= 1, "最小宽度下预设应换行：{presets:?}");
        for (i, a) in presets.iter().enumerate() {
            for b in &presets[i + 1..] {
                assert!(a.intersection(*b).is_none(), "预设不应重叠：{a:?} vs {b:?}");
            }
        }

        let preset_bottom = presets
            .iter()
            .map(|rect| rect.bottom())
            .fold(0.0f32, f32::max);
        let background_top = rects(&modal, &modal.nodes.backgrounds)
            .iter()
            .map(|rect| rect.top())
            .fold(f32::INFINITY, f32::min);
        assert!(
            preset_bottom <= background_top + 0.5,
            "换行后的预设压住了下一行：preset_bottom={preset_bottom} background_top={background_top}"
        );
    }

    /// 透明背景在色块里用棋盘格预览，而不是一片空白。
    #[test]
    fn the_transparent_swatch_previews_a_checkerboard() {
        let modal = modal();
        let mut ctx = PaintContext::new();
        modal.paint(&mut ctx);
        let commands = ctx.into_draw_list().commands().to_vec();
        let swatch = rects(&modal, &modal.nodes.backgrounds)[0];

        let fills = |color: UiColor| {
            commands.iter().any(|command| {
                matches!(command, DrawCommand::FillRect { rect, paint }
                    if paint.color == color && rect.intersects(swatch))
            })
        };
        assert!(fills(to_ui(CHECKER_LIGHT)), "缺少亮格");
        assert!(fills(to_ui(CHECKER_DARK)), "缺少暗格");
    }

    #[test]
    fn a_preset_then_create_returns_that_spec() {
        let mut modal = modal();
        let index = PRESETS.len() - 1;
        let (width, height) = PRESETS[index];
        let id = modal.nodes.presets.borrow()[index];
        let position = center(&modal, id).expect("预设按钮");
        click(&mut modal, position);
        assert_eq!(modal.spec().width, width);
        assert_eq!(modal.spec().height, height);

        modal.accept();
        assert_eq!(
            modal.take_result(),
            Some(NewDocumentResult::Create(NewDocumentSpec {
                width,
                height,
                background: Color::TRANSPARENT,
            }))
        );
        assert_eq!(modal.take_result(), None, "结果只能取走一次");
    }

    #[test]
    fn choosing_a_background_then_create_returns_it() {
        let mut modal = modal();
        let id = modal.nodes.backgrounds.borrow()[0];
        let position = center(&modal, id).expect("透明色块");
        click(&mut modal, position);
        assert_eq!(modal.spec().background, Color::TRANSPARENT);

        modal.accept();
        assert_eq!(
            modal.take_result(),
            Some(NewDocumentResult::Create(NewDocumentSpec {
                width: 32,
                height: 32,
                background: Color::TRANSPARENT,
            }))
        );
    }

    /// 点「创建」按钮本身（不是测试后门）也产出结果。
    #[test]
    fn clicking_the_create_button_returns_the_spec() {
        let mut modal = modal();
        let id = modal.nodes.create.get().expect("创建按钮");
        let position = center(&modal, id).expect("创建按钮 rect");
        click(&mut modal, position);
        assert_eq!(
            modal.take_result(),
            Some(NewDocumentResult::Create(NewDocumentSpec::default()))
        );
    }

    #[test]
    fn cancel_reports_cancel() {
        let modal = modal();
        modal.cancel();
        assert_eq!(modal.take_result(), Some(NewDocumentResult::Cancel));
    }

    /// Esc 关掉窗口，取结果时折算成取消。
    #[test]
    fn escape_marks_the_modal_cancelled() {
        let mut modal = modal();
        modal.event(&InputEvent::KeyDown {
            key: igui_core::Key::Escape,
        });
        assert_eq!(modal.take_result(), Some(NewDocumentResult::Cancel));
    }

    /// 点窗口外（遮罩）也折算成取消。
    #[test]
    fn a_click_outside_marks_the_modal_cancelled() {
        let mut modal = modal();
        let outside = Vec2::new(2.0, 2.0);
        assert!(!modal.rect().contains(outside));
        modal.event(&InputEvent::PointerDown {
            position: outside,
            button: PointerButton::Left,
        });
        assert_eq!(modal.take_result(), Some(NewDocumentResult::Cancel));
    }
}
