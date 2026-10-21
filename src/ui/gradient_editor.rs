//! 多节点渐变编辑器（游戏引擎式色带）：色带预览 + 可拖动节点 + 节点操作行。
//!
//! 由渐变工具的选项栏「编辑…」用 `Overlays::popover` 打开。色带是一个控件，
//! `on_pointer` 把指针 x 映射成位置 0..1：
//!
//! - 按下靠近某个节点（±[`HANDLE_HIT`] 逻辑像素）→ 选中并开始拖动；
//! - 按下空白处 → 在该位置新增节点（颜色取当前采样值）；
//! - 拖动 → 移动节点（保持排序）。
//!
//! 节点颜色用「选中节点 + [前景色]/[背景色]」设置，复用现有调色盘改前景的流程，
//! 不另做取色器。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use igui_components::{set_text, Button, Component, Flex, NodeRef, Text};
use igui_core::{Color as UiColor, Edges, NodeId, Rect, Size, Vec2};
use igui_render::{Paint, PaintContext};
use igui_scene::SceneTree;
use igui_theme::{radius, space, Theme, Tone};
use igui_ui::{Align, MouseFilter, PointerPhase, SurfaceStyle};

use crate::app::state::AppState;
use crate::document::{Color, Gradient};
use crate::tools::{GradientKind, GradientTool};

use super::GradientAdjust;

/// 渐变工具的视图侧状态（[`crate::ui::EditorView`] 的 `gradient` 字段）。
///
/// 色带本身（`ramp`）是 `Rc<RefCell<_>>`：绘制引擎 [`GradientTool`] 和这个编辑器
/// 共用同一份，编辑器里拖节点立刻反映到下一次拖动。类型 / 反向 / 「自定义过」也
/// 是共享格：选项栏按钮直接写，`update` 每帧对齐进工具。
pub(super) struct GradientState {
    /// 多节点色带（`GradientTool` 与编辑器共享）。
    pub(super) ramp: Rc<RefCell<Gradient>>,
    /// 绘制引擎（读 `ramp` / `kind` / `reverse` / 选区）。
    pub(super) tool: GradientTool,
    /// 选项栏切出来的类型（线性 / 径向……）。
    pub(super) kind: GradientKind,
    pub(super) reverse: Rc<Cell<bool>>,
    /// 用户改过色带节点后为 true；否则首尾跟随前景 / 背景。
    pub(super) customized: Rc<Cell<bool>>,
    /// 编辑器里选中的节点下标。
    pub(super) selected: Rc<Cell<usize>>,
    /// 编辑器是否开着。
    pub(super) editor_open: Cell<bool>,
    /// 选项栏的类型标签 / 预览条（popover 的锚点）/「编辑…」按钮。
    pub(super) kind_label: NodeId,
    pub(super) preview: NodeId,
    pub(super) edit_button: NodeId,
    /// 类型切换按钮节点（测试 / 自检点击用）。
    pub(super) buttons: Vec<(GradientAdjust, NodeId)>,
}

/// 色带高度（逻辑像素）：上半条画渐变，下半条放节点手柄。
const BAR_HEIGHT: f32 = 30.0;
/// 渐变条（画渐变的部分）的高度。
const STRIP_HEIGHT: f32 = 18.0;
/// 节点手柄的命中半径（逻辑像素）。
const HANDLE_HIT: f32 = 9.0;
/// 预览用的切片数。
const SLICES: usize = 48;

/// 编辑器里需要回写的节点槽位（测试点击用）。
#[derive(Default)]
pub struct GradientEditorRefs {
    pub bar: NodeRef,
    pub foreground: NodeRef,
    pub background: NodeRef,
    pub remove: NodeRef,
    pub reset: NodeRef,
}

/// 文档色（8 位 RGBA）-> UI 色（0..1）。
pub fn to_ui(color: Color) -> UiColor {
    UiColor::new(
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
        color.a as f32 / 255.0,
    )
}

/// 在 `rect` 里用 `SLICES` 片实心色块拼出渐变预览。
pub fn paint_gradient_bar(ctx: &mut PaintContext, rect: Rect, gradient: &Gradient) {
    let width = rect.size.width;
    if width <= 0.0 {
        return;
    }
    for index in 0..SLICES {
        let t = (index as f32 + 0.5) / SLICES as f32;
        let color = gradient.evaluate(t);
        let x = rect.left() + width * index as f32 / SLICES as f32;
        let slice = width / SLICES as f32;
        ctx.fill_rect(
            Rect::from_min_size(
                Vec2::new(x, rect.top()),
                Size::new(slice + 1.0, rect.size.height),
            ),
            to_ui(color),
        );
    }
}

/// 编辑器内容：色带 + 节点操作行。返回内容根节点。
// 构建函数：控件树 + 主题 + 共享状态 + 引用槽，参数天然多。
#[allow(clippy::too_many_arguments)]
pub fn gradient_editor_content(
    tree: &mut SceneTree,
    root: NodeId,
    theme: &'static dyn Theme,
    state: Rc<RefCell<AppState>>,
    gradient: Rc<RefCell<Gradient>>,
    customized: Rc<Cell<bool>>,
    selected: Rc<Cell<usize>>,
    refs: &GradientEditorRefs,
) -> NodeId {
    let position_ref = NodeRef::new();
    let dragging = Rc::new(Cell::new(false));

    let bar_gradient = gradient.clone();
    let bar_selected = selected.clone();
    let bar_customized = customized.clone();
    let bar_dragging = dragging.clone();
    let bar_position = position_ref.clone();
    let paint_gradient = gradient.clone();
    let paint_selected = selected.clone();
    let bar = Flex::new()
        .min_size(200.0, BAR_HEIGHT)
        .mouse_filter(MouseFilter::Stop)
        .on_pointer(move |tree, _id, phase, rect, point| {
            let t = ((point.x - rect.left()) / rect.size.width.max(1.0)).clamp(0.0, 1.0);
            match phase {
                PointerPhase::Down => {
                    let nearest = bar_gradient
                        .borrow()
                        .stops()
                        .iter()
                        .enumerate()
                        .map(|(index, stop)| (index, (stop.position - t).abs() * rect.size.width))
                        .filter(|(_, distance)| *distance <= HANDLE_HIT)
                        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
                    match nearest {
                        Some((index, _)) => {
                            bar_selected.set(index);
                        }
                        None => {
                            let color = bar_gradient.borrow().evaluate(t);
                            let index = bar_gradient.borrow_mut().add_stop(t, color);
                            bar_selected.set(index);
                            bar_customized.set(true);
                        }
                    }
                    bar_dragging.set(true);
                    sync_position(tree, &bar_position, &bar_gradient, &bar_selected);
                }
                PointerPhase::Move if bar_dragging.get() => {
                    let index = bar_selected.get();
                    let new_index = bar_gradient.borrow_mut().move_stop(index, t);
                    bar_selected.set(new_index);
                    bar_customized.set(true);
                    sync_position(tree, &bar_position, &bar_gradient, &bar_selected);
                }
                PointerPhase::Up => bar_dragging.set(false),
                _ => {}
            }
        })
        .foreground(move |ctx, rect, _| {
            paint_gradient_bar(
                ctx,
                Rect::from_min_size(rect.origin, Size::new(rect.size.width, STRIP_HEIGHT)),
                &paint_gradient.borrow(),
            );
            let stops = paint_gradient.borrow();
            let selected = paint_selected.get();
            let handle_size = 10.0;
            for (index, stop) in stops.stops().iter().enumerate() {
                let x = rect.left() + rect.size.width * stop.position;
                let handle = Rect::from_center_size(
                    Vec2::new(x, rect.top() + STRIP_HEIGHT + handle_size * 0.5 - 2.0),
                    Size::splat(handle_size),
                );
                ctx.fill_rect(handle, to_ui(stop.color));
                let border = if index == selected {
                    theme.palette().foreground
                } else {
                    theme.palette().border
                };
                ctx.stroke_rect(handle, 1.0, Paint::new(border));
            }
        })
        .ref_(&refs.bar);

    let fg_gradient = gradient.clone();
    let fg_state = state.clone();
    let fg_customized = customized.clone();
    let fg_selected = selected.clone();
    let foreground = Button::ghost("前景色", theme)
        .on_click(move |_tree, _id| {
            let color = fg_state.borrow().foreground;
            fg_gradient.borrow_mut().set_color(fg_selected.get(), color);
            fg_customized.set(true);
        })
        .ref_(&refs.foreground);

    let bg_gradient = gradient.clone();
    let bg_state = state.clone();
    let bg_customized = customized.clone();
    let bg_selected = selected.clone();
    let background = Button::ghost("背景色", theme)
        .on_click(move |_tree, _id| {
            let color = bg_state.borrow().background;
            bg_gradient.borrow_mut().set_color(bg_selected.get(), color);
            bg_customized.set(true);
        })
        .ref_(&refs.background);

    let remove_gradient = gradient.clone();
    let remove_selected = selected.clone();
    let remove = Button::ghost("删除", theme)
        .on_click(move |_tree, _id| {
            let index = remove_selected.get();
            if remove_gradient.borrow_mut().remove_stop(index) {
                let last = remove_gradient.borrow().stops().len() - 1;
                remove_selected.set(index.min(last));
            }
        })
        .ref_(&refs.remove);

    let reset_gradient = gradient.clone();
    let reset_state = state.clone();
    let reset_customized = customized.clone();
    let reset_selected = selected.clone();
    let reset = Button::ghost("重置", theme)
        .on_click(move |_tree, _id| {
            let (foreground, background) = {
                let state = reset_state.borrow();
                (state.foreground, state.background)
            };
            *reset_gradient.borrow_mut() = Gradient::from_colors(foreground, background);
            reset_selected.set(0);
            reset_customized.set(false);
        })
        .ref_(&refs.reset);

    let row = Flex::row()
        .align(Align::Center)
        .gap(space::XS)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::caption("位置", theme).tone(Tone::Muted))
        .child(Text::small("", theme).ref_(&position_ref))
        .child(foreground)
        .child(background)
        .child(remove)
        .child(reset);

    let column = Flex::column()
        .gap(space::SM)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(bar)
        .child(row);
    let node = tree.add_child(root, column);
    sync_position(tree, &position_ref, &gradient, &selected);
    node
}

/// 把选中节点的位置写进「位置」标签。
fn sync_position(
    tree: &mut SceneTree,
    position_ref: &NodeRef,
    gradient: &Rc<RefCell<Gradient>>,
    selected: &Rc<Cell<usize>>,
) {
    let Some(id) = position_ref.get() else {
        return;
    };
    let stops = gradient.borrow();
    let index = selected.get().min(stops.stops().len().saturating_sub(1));
    let position = stops.stops()[index].position;
    set_text(tree, id, format!("{position:.2}"));
}

/// 一个色带预览组件（选项栏里显示当前 ramp；点击可选）。
pub fn gradient_preview(
    theme: &'static dyn Theme,
    gradient: Rc<RefCell<Gradient>>,
    slot: &NodeRef,
) -> impl Component {
    Flex::new()
        .min_size(96.0, STRIP_HEIGHT)
        .mouse_filter(MouseFilter::Ignore)
        .foreground(move |ctx, rect, _| {
            paint_gradient_bar(ctx, rect, &gradient.borrow());
        })
        .surface(SurfaceStyle::new(theme.palette().surface).radius(radius::SM))
        .ref_(slot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use igui_core::{InputEvent, PointerButton, ViewportSize};

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

    /// 点「前景色」把选中节点设成当前前景色。
    #[test]
    fn the_foreground_button_sets_the_selected_stop() {
        let theme = crate::theme::editor_theme(false);
        let state = Rc::new(RefCell::new(AppState::default()));
        state.borrow_mut().foreground = Color::RED;
        let gradient = Rc::new(RefCell::new(Gradient::from_colors(
            Color::BLACK,
            Color::WHITE,
        )));
        let customized = Rc::new(Cell::new(false));
        let selected = Rc::new(Cell::new(1));
        let refs = GradientEditorRefs::default();

        let mut tree = SceneTree::new();
        let root = tree.root();
        gradient_editor_content(
            &mut tree,
            root,
            theme,
            state,
            gradient.clone(),
            customized.clone(),
            selected,
            &refs,
        );
        let viewport = ViewportSize::new(Size::new(360.0, 120.0));
        tree.set_viewport_size(viewport.logical_size());
        igui_ui::layout(&mut tree, viewport);
        tree.update();

        let button = refs.foreground.get().expect("前景色按钮");
        let center = igui_ui::control(&tree, button).unwrap().rect.center();
        click(&mut tree, center);
        assert_eq!(gradient.borrow().stops()[1].color, Color::RED);
        assert!(customized.get(), "改过颜色应标记为自定义");
    }

    /// 色带预览会发出渐变切片。
    #[test]
    fn the_preview_emits_gradient_slices() {
        let gradient = Gradient::from_colors(Color::BLACK, Color::WHITE);
        let mut ctx = PaintContext::new();
        paint_gradient_bar(
            &mut ctx,
            Rect::from_min_size(Vec2::ZERO, Size::new(100.0, 18.0)),
            &gradient,
        );
        let fills = ctx
            .into_draw_list()
            .commands()
            .iter()
            .filter(|command| matches!(command, igui_render::DrawCommand::FillRect { .. }))
            .count();
        assert!(fills >= SLICES, "应至少 {SLICES} 片，实际 {fills}");
    }
}
