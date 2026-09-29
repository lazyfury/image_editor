//! 「新建文档」模态框：选画布尺寸 / 背景色。
//!
//! 由首页的「新建项目」打开，内容构建进
//! [`Overlays`](igui_components::Overlays) 的模态框：scrim、居中、边框、Esc /
//! 点外关闭以及「取消 / 创建」按钮行都由它负责。点「创建」把选择通过共享格交回
//! 宿主（[`crate::app::application`]），宿主把它变成同一个窗口里的一个新
//! [`EditorView`](crate::ui::EditorView)。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use igui_components::{Button, Component, Flex, NodeRef, OverlayId, Overlays, Text};
use igui_core::{Color as UiColor, Edges, NodeId, Rect, Size, Vec2};
use igui_render::PaintContext;
use igui_scene::SceneTree;
use igui_theme::{radius, space, TextSize, Theme, Tone};
use igui_ui::{Align, MouseFilter, SurfaceStyle};

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
}

/// 「新建文档」模态框：把选择控件构建进 [`Overlays`] 的私有覆盖树。
///
/// `Overlays::modal` 负责 scrim、居中、边框、Esc / 点外关闭以及「取消 / 创建」
/// 按钮行；这里只提供尺寸 / 背景色控件，并把「创建」时的选择通过共享格交回宿主
/// （[`take_result`]）。
pub struct NewDocumentModal {
    theme: &'static dyn Theme,
    /// 当前选择（共享格，回调只能往里写）。
    choices: Rc<RefCell<NewDocumentSpec>>,
    /// 有改动、还没同步到标签。
    dirty: Rc<Cell<bool>>,
    /// 创建 / 取消的结果；宿主取走一次。
    result: Rc<Cell<Option<NewDocumentResult>>>,
    nodes: Rc<ModalNodes>,
}

impl NewDocumentModal {
    pub fn new(theme: &'static dyn Theme) -> Self {
        Self {
            theme,
            choices: Rc::new(RefCell::new(NewDocumentSpec::default())),
            dirty: Rc::new(Cell::new(false)),
            result: Rc::new(Cell::new(None)),
            nodes: Rc::new(ModalNodes::default()),
        }
    }

    /// 在 `overlays` 上打开模态框，返回它的 id。
    ///
    /// 「创建 / 取消」用 `Overlays` 自带的按钮行，回调把结果写进共享格。
    pub fn open(&self, overlays: &mut Overlays) -> OverlayId {
        let theme = self.theme;
        let choices = self.choices.clone();
        let dirty = self.dirty.clone();
        let nodes = self.nodes.clone();
        let id = overlays.modal("新建文档", move |tree, parent| {
            build_content(tree, parent, theme, &choices, &dirty, &nodes);
        });

        let confirm_choices = self.choices.clone();
        let confirm_result = self.result.clone();
        let cancel_result = self.result.clone();
        overlays
            .confirm_label(id, "创建")
            .cancel_label(id, "取消")
            .on_confirm(id, move || commit(&confirm_choices, &confirm_result))
            .on_cancel(id, move || abort(&cancel_result));
        id
    }

    /// 把共享格里的选择写进标签（只在控件改动后）。
    pub fn sync(&self, overlays: &mut Overlays) {
        if !self.dirty.replace(false) {
            return;
        }
        let spec = *self.choices.borrow();
        if let Some(id) = self.nodes.width.get() {
            overlays.set_text(id, format!("{} px", spec.width));
        }
        if let Some(id) = self.nodes.height.get() {
            overlays.set_text(id, format!("{} px", spec.height));
        }
        if let Some(id) = self.nodes.background.get() {
            overlays.set_text(id, format!("背景：{}", background_name(spec.background)));
        }
    }

    /// 创建 / 取消的结果；取走一次。
    pub fn take_result(&self) -> Option<NewDocumentResult> {
        self.result.take()
    }

    /// 当前选择（测试用）。
    #[cfg(test)]
    fn spec(&self) -> NewDocumentSpec {
        *self.choices.borrow()
    }

    #[cfg(test)]
    fn accept(&self) {
        commit(&self.choices, &self.result);
    }

    #[cfg(test)]
    fn cancel(&self) {
        abort(&self.result);
    }

    /// 把内容构建到一个外部 `tree`（测试直接铺排 / 点击）。
    #[cfg(test)]
    fn build_into(&self, tree: &mut SceneTree, parent: NodeId) {
        build_content(
            tree,
            parent,
            self.theme,
            &self.choices,
            &self.dirty,
            &self.nodes,
        );
    }
}

/// 把尺寸 / 背景色选择控件构建到模态框内容里。
fn build_content(
    tree: &mut SceneTree,
    parent: NodeId,
    theme: &'static dyn Theme,
    choices: &Rc<RefCell<NewDocumentSpec>>,
    dirty: &Rc<Cell<bool>>,
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

    nodes.width.set(width_ref.get());
    nodes.height.set(height_ref.get());
    nodes.background.set(background_ref.get());
    *nodes.presets.borrow_mut() = preset_slots.iter().filter_map(|slot| slot.get()).collect();
    *nodes.backgrounds.borrow_mut() = background_slots
        .iter()
        .filter_map(|slot| slot.get())
        .collect();
}

/// 「创建」：把当前选择写成结果。
fn commit(choices: &Rc<RefCell<NewDocumentSpec>>, result: &Rc<Cell<Option<NewDocumentResult>>>) {
    result.set(Some(NewDocumentResult::Create(*choices.borrow())));
}

/// 「取消 / Esc / 点外」：只标记取消，宿主据此丢弃浮层。
fn abort(result: &Rc<Cell<Option<NewDocumentResult>>>) {
    result.set(Some(NewDocumentResult::Cancel));
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
    Button::secondary(label, theme).on_click(move || {
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
        .on_click(move || {
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
        .on_click(move || {
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
    use igui_core::{InputEvent, Key, PointerButton, ViewportSize};
    use igui_render::DrawCommand;

    fn viewport() -> ViewportSize {
        ViewportSize::new(Size::new(560.0, 640.0))
    }

    /// 把模态内容构建到一棵独立树里铺排，单测控件本身（不经过 `Overlays`）。
    fn content_tree(modal: &NewDocumentModal, viewport: ViewportSize) -> SceneTree {
        let mut tree = SceneTree::new();
        let root = tree.root();
        let page = tree.add_child(root, Flex::new().gap(0.0).padding(Edges::ZERO));
        modal.build_into(&mut tree, page);
        tree.set_viewport_size(viewport.logical_size());
        igui_ui::layout(&mut tree, viewport);
        tree.update();
        tree
    }

    fn click(tree: &mut SceneTree, position: Vec2) {
        igui_ui::handle_input(
            tree,
            &InputEvent::PointerDown {
                position,
                button: PointerButton::Left,
            },
        );
        igui_ui::handle_input(
            tree,
            &InputEvent::PointerUp {
                position,
                button: PointerButton::Left,
            },
        );
        tree.update();
    }

    fn center(tree: &SceneTree, id: NodeId) -> Option<Vec2> {
        igui_ui::control(tree, id).map(|control| control.rect.center())
    }

    fn preset_rects(modal: &NewDocumentModal, tree: &SceneTree) -> Vec<Rect> {
        modal
            .nodes
            .presets
            .borrow()
            .iter()
            .filter_map(|id| igui_ui::control(tree, *id).map(|control| control.rect))
            .collect()
    }

    fn background_rects(modal: &NewDocumentModal, tree: &SceneTree) -> Vec<Rect> {
        modal
            .nodes
            .backgrounds
            .borrow()
            .iter()
            .filter_map(|id| igui_ui::control(tree, *id).map(|control| control.rect))
            .collect()
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
    fn a_wrapped_preset_row_grows_and_does_not_overlap_the_background_row() {
        let modal = NewDocumentModal::new(crate::theme::editor_theme(false));
        // 窄窗口：预设必然换行。
        let tree = content_tree(&modal, ViewportSize::new(Size::new(260.0, 420.0)));

        let presets = preset_rects(&modal, &tree);
        let first_row = presets.first().map(|rect| rect.top()).unwrap_or(0.0);
        let rows = presets
            .iter()
            .filter(|rect| (rect.top() - first_row).abs() > 0.5)
            .count();
        assert!(rows >= 1, "预设应在窄窗口里换行：{presets:?}");
        for (i, a) in presets.iter().enumerate() {
            for b in &presets[i + 1..] {
                assert!(a.intersection(*b).is_none(), "预设不应重叠：{a:?} vs {b:?}");
            }
        }

        let preset_bottom = presets
            .iter()
            .map(|rect| rect.bottom())
            .fold(0.0f32, f32::max);
        let background_top = background_rects(&modal, &tree)
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
        let modal = NewDocumentModal::new(crate::theme::editor_theme(false));
        let tree = content_tree(&modal, viewport());
        let mut ctx = PaintContext::new();
        tree.paint(&mut ctx);
        igui_ui::paint(&tree, &mut ctx);
        let commands = ctx.into_draw_list().commands().to_vec();
        let swatch = background_rects(&modal, &tree)[0];

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
        let modal = NewDocumentModal::new(crate::theme::editor_theme(false));
        let mut tree = content_tree(&modal, viewport());
        let index = PRESETS.len() - 1;
        let (width, height) = PRESETS[index];
        let id = modal.nodes.presets.borrow()[index];
        let position = center(&tree, id).expect("预设按钮");
        click(&mut tree, position);
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
        let modal = NewDocumentModal::new(crate::theme::editor_theme(false));
        let mut tree = content_tree(&modal, viewport());
        let id = modal.nodes.backgrounds.borrow()[0];
        let position = center(&tree, id).expect("透明色块");
        click(&mut tree, position);
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

    #[test]
    fn cancel_reports_cancel() {
        let modal = NewDocumentModal::new(crate::theme::editor_theme(false));
        modal.cancel();
        assert_eq!(modal.take_result(), Some(NewDocumentResult::Cancel));
    }

    /// `Overlays` 的模态框已居中、带边框，并吞掉底层输入。
    #[test]
    fn the_overlay_modal_is_centered_and_swallows_input() {
        let theme = crate::theme::editor_theme(false);
        let modal = NewDocumentModal::new(theme);
        let mut overlays = Overlays::new(theme);
        let id = modal.open(&mut overlays);
        let host = SceneTree::new();
        overlays.layout(&host, viewport());

        let rect = overlays.rect(id).expect("模态 rect");
        assert!(rect.size.width >= 360.0, "模态应有最小宽度：{rect:?}");

        let event = InputEvent::PointerMove {
            position: Vec2::new(4.0, 4.0),
        };
        assert!(overlays.handle_input(&event).is_handled());
    }

    /// Esc 关掉模态但没有创建结果（宿主视为取消）。
    #[test]
    fn escape_closes_the_modal_without_a_result() {
        let theme = crate::theme::editor_theme(false);
        let modal = NewDocumentModal::new(theme);
        let mut overlays = Overlays::new(theme);
        let id = modal.open(&mut overlays);
        let host = SceneTree::new();
        overlays.layout(&host, viewport());

        let event = InputEvent::KeyDown { key: Key::Escape };
        assert!(overlays.handle_input(&event).is_handled());
        assert!(!overlays.is_open(id));
        assert_eq!(modal.take_result(), None);
    }
}
