//! 工具栏：分组工具按钮（长按展开切换）+ 撤销 / 重做。
//!
//! 相关工具收进一个「组按钮」（见 [`crate::app::tool_groups`]）：组按钮画当前
//! 「脸」的图标 + 小字标签，右下角一个小三角表示「可长按展开」。
//!
//! - **短按**：激活脸工具（往 [`ToolbarWiring::intents`] 里塞 [`Intent::Tool`]，
//!   由 `EditorView` 统一应用并同步脸）。
//! - **长按**（按住 ≥ 阈值且没怎么移动）：`EditorView` 轮询 [`LongPress`] 打开
//!   向右的浮层，列出组内全部工具。
//!
//! 图标用 `foreground` 动态画（读共享的脸格），换脸不需要重建树；标签用
//! `set_text` 同步（跟状态栏一个套路）。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

use igui_components::{Button, Component, Divider, Flex, NodeRef, Text};
use igui_core::{Color, Edges, NodeId, Rect, Size, Vec2};
use igui_scene::SceneTree;
use igui_theme::{radius, space, SurfaceLevel, Theme};
use igui_ui::{Align, MouseFilter, PointerPhase, SizeBasis, SurfaceStyle};

use crate::app::state::{ActiveTool, AppState, HistoryAction};
use crate::app::tool_groups::{ToolGroup, TOOL_GROUPS};
use crate::icons::{Icon, IconSet, TOOLBAR_ICON};
use crate::ui::{Intent, Intents, LONG_PRESS_SLOP, TOOLBAR_WIDTH};

/// 工具栏长按状态。由组按钮的 `on_pointer` 写，`EditorView::update` 轮询。
#[derive(Debug, Clone, Copy, Default)]
pub enum LongPress {
    #[default]
    Idle,
    /// 已按下、还在等阈值。`moved` 为真表示指针移动超过容差、不再算长按。
    Pending {
        group: usize,
        start: Instant,
        origin: Vec2,
        moved: bool,
    },
    /// 已展开浮层；松手时不要再当短按激活。
    Fired,
}

/// 工具栏的共享接线（回调拿不到 `&mut EditorView`，都走共享格）。
pub struct ToolbarWiring {
    pub state: Rc<RefCell<AppState>>,
    pub message: Rc<RefCell<Option<String>>>,
    pub icons: Rc<IconSet>,
    /// 每个组的「脸」工具，下标对应 [`TOOL_GROUPS`]。
    pub faces: Vec<Rc<Cell<ActiveTool>>>,
    pub long_press: Rc<RefCell<LongPress>>,
    /// 意图队列（短按 / 浮层选中都塞 [`Intent::Tool`]）。
    pub intents: Intents,
}

/// 竖直工具栏。`group_slots` / `label_slots` 收集每个组按钮与其标签的节点，
/// `history_slots` 收集撤销 / 重做按钮；测试与自检靠它们模拟点击。
pub fn tool_bar(
    theme: &'static dyn Theme,
    wiring: ToolbarWiring,
    group_slots: &mut Vec<NodeRef>,
    label_slots: &mut Vec<NodeRef>,
    history_slots: &mut Vec<(HistoryAction, NodeRef)>,
) -> impl Component {
    let mut bar = Flex::column()
        .gap(space::XXXS)
        .padding(Edges::symmetric(space::XXS, space::XXS))
        .basis(SizeBasis::Px(TOOLBAR_WIDTH))
        .shrink(0.0)
        .background(theme.surface(SurfaceLevel::Surface))
        .mouse_filter(MouseFilter::Ignore);
    for (index, group) in TOOL_GROUPS.iter().enumerate() {
        let button_slot = NodeRef::new();
        let label_slot = NodeRef::new();
        group_slots.push(button_slot.clone());
        label_slots.push(label_slot.clone());
        let face = wiring
            .faces
            .get(index)
            .cloned()
            .unwrap_or_else(|| Rc::new(Cell::new(group.face())));
        bar = bar.child(group_button(
            theme,
            *group,
            index,
            &wiring,
            face,
            &button_slot,
            &label_slot,
        ));
    }
    bar = bar.child(Divider::horizontal(theme));
    for action in HistoryAction::ALL {
        let slot = NodeRef::new();
        history_slots.push((action, slot.clone()));
        bar = bar.child(history_button(
            theme,
            action,
            wiring.state.clone(),
            wiring.message.clone(),
            wiring.icons.clone(),
            &slot,
        ));
    }
    bar
}

/// 一个组按钮：动态画脸图标（+ 可展开小三角）、短按激活脸、长按记录手势。
fn group_button(
    theme: &'static dyn Theme,
    group: ToolGroup,
    index: usize,
    wiring: &ToolbarWiring,
    face: Rc<Cell<ActiveTool>>,
    button_slot: &NodeRef,
    label_slot: &NodeRef,
) -> impl Component {
    let color = theme.palette().foreground;
    let icons = wiring.icons.clone();
    let press = wiring.long_press.clone();
    let intents = wiring.intents.clone();
    let click_face = face.clone();
    let highlight_state = wiring.state.clone();
    let expandable = !group.is_single();

    let button = Button::ghost("", theme)
        .min_size(32.0, 28.0)
        .foreground(move |ctx, rect, _| {
            if let Some(document) = icons.document(face.get().icon()) {
                let icon = Rect::from_center_size(rect.center(), Size::splat(TOOLBAR_ICON));
                document.draw(ctx, icon, color);
            }
            if expandable {
                // 右下角一个小三角，提示「可长按展开」。
                let point = |dx: f32, dy: f32| Vec2::new(rect.right() - dx, rect.bottom() - dy);
                let (a, b, c) = (point(6.0, 1.5), point(1.5, 1.5), point(1.5, 6.0));
                ctx.draw_line(a, b, 1.0, color);
                ctx.draw_line(b, c, 1.0, color);
                ctx.draw_line(c, a, 1.0, color);
            }
        })
        .on_pointer(move |_tree, _id, phase, _rect, point| {
            let mut state = press.borrow_mut();
            match phase {
                PointerPhase::Down => {
                    *state = LongPress::Pending {
                        group: index,
                        start: Instant::now(),
                        origin: point,
                        moved: false,
                    };
                }
                PointerPhase::Move => {
                    if let LongPress::Pending { origin, moved, .. } = &mut *state {
                        if (point - *origin).length() > LONG_PRESS_SLOP {
                            *moved = true;
                        }
                    }
                }
                PointerPhase::Up => {
                    let was_pending = matches!(*state, LongPress::Pending { .. });
                    *state = LongPress::Idle;
                    // 长按已展开浮层时，这次松手不算短按。
                    if was_pending {
                        intents.push(Intent::Tool(click_face.get()));
                    }
                }
                PointerPhase::DoubleClick => {}
            }
        })
        .dynamic_background(move |interact| {
            let active = group.contains(highlight_state.borrow().active_tool);
            if active {
                SurfaceStyle::new(theme.palette().selection).radius(radius::SM)
            } else if interact.hovered || interact.pressed {
                SurfaceStyle::new(theme.palette().surface_hover).radius(radius::SM)
            } else {
                SurfaceStyle::new(Color::TRANSPARENT)
            }
        })
        .ref_(button_slot);

    Flex::column()
        .gap(0.0)
        .padding(Edges::ZERO)
        .align(Align::Center)
        .mouse_filter(MouseFilter::Ignore)
        .child(button)
        .child(Text::caption(group.face().short_label(), theme).ref_(label_slot))
}

/// 撤销 / 重做按钮：图标 + 写共享状态里的 `History`，并把结果写到状态栏提示。
fn history_button(
    theme: &'static dyn Theme,
    action: HistoryAction,
    state: Rc<RefCell<AppState>>,
    message: Rc<RefCell<Option<String>>>,
    icons: Rc<IconSet>,
    slot: &NodeRef,
) -> impl Component {
    let button = Button::ghost("", theme)
        .min_size(32.0, 28.0)
        .child(Icon::new(
            icons,
            action.icon(),
            theme.palette().foreground,
            TOOLBAR_ICON,
        ))
        .on_click(move |_tree, _id| {
            let result = {
                let mut state = state.borrow_mut();
                match action {
                    HistoryAction::Undo => state.undo(),
                    HistoryAction::Redo => state.redo(),
                }
            };
            *message.borrow_mut() = Some(match result {
                Some(label) => format!("{}：{label}", action.label()),
                None => format!("没有可{}的操作", action.label()),
            });
        })
        .ref_(slot);
    Flex::column()
        .gap(0.0)
        .padding(Edges::ZERO)
        .align(Align::Center)
        .mouse_filter(MouseFilter::Ignore)
        .child(button)
        .child(Text::caption(action.label(), theme))
}

/// 长按展开的工具浮层：一列「图标 + 文字」行，点一行切工具。
pub fn tool_flyout_content(
    tree: &mut SceneTree,
    root: NodeId,
    theme: &'static dyn Theme,
    group: usize,
    state: Rc<RefCell<AppState>>,
    icons: Rc<IconSet>,
    intents: Intents,
) {
    let Some(group) = TOOL_GROUPS.get(group) else {
        return;
    };
    let mut column = Flex::column()
        .gap(space::XXXS)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore);
    for &tool in group.tools {
        column = column.child(flyout_row(
            theme,
            tool,
            state.clone(),
            icons.clone(),
            intents.clone(),
        ));
    }
    tree.add_child(root, column);
}

/// 浮层里的一行：图标 + 标签，当前工具高亮。
fn flyout_row(
    theme: &'static dyn Theme,
    tool: ActiveTool,
    state: Rc<RefCell<AppState>>,
    icons: Rc<IconSet>,
    intents: Intents,
) -> impl Component {
    let highlight_state = state.clone();
    let click_intents = intents.clone();
    Button::ghost("", theme)
        .min_size(140.0, 28.0)
        .child(
            Flex::row()
                .align(Align::Center)
                .gap(space::SM)
                .padding(Edges::ZERO)
                .mouse_filter(MouseFilter::Ignore)
                .child(Icon::new(
                    icons,
                    tool.icon(),
                    theme.palette().foreground,
                    TOOLBAR_ICON,
                ))
                .child(Text::small(tool.short_label(), theme)),
        )
        .on_click(move |_tree, _id| click_intents.push(Intent::Tool(tool)))
        .dynamic_background(move |interact| {
            let active = highlight_state.borrow().active_tool == tool;
            if active {
                SurfaceStyle::new(theme.palette().selection).radius(radius::SM)
            } else if interact.hovered || interact.pressed {
                SurfaceStyle::new(theme.palette().surface_hover).radius(radius::SM)
            } else {
                SurfaceStyle::new(Color::TRANSPARENT)
            }
        })
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

    /// 浮层里点一行会往意图队列里塞一个工具切换。
    #[test]
    fn clicking_a_flyout_row_requests_that_tool() {
        let theme = crate::theme::editor_theme(false);
        let state = Rc::new(RefCell::new(AppState::default()));
        let icons = Rc::new(IconSet::load());
        let intents = Intents::new();

        let mut tree = SceneTree::new();
        let root = tree.root();
        let row = flyout_row(theme, ActiveTool::Eraser, state, icons, intents.clone())
            .build(&mut tree, root);
        let viewport = ViewportSize::new(Size::new(220.0, 60.0));
        tree.set_viewport_size(viewport.logical_size());
        igui_ui::layout(&mut tree, viewport);
        tree.update();

        let center = igui_ui::control(&tree, row).expect("浮层行").rect.center();
        click(&mut tree, center);
        assert_eq!(intents.take(), vec![Intent::Tool(ActiveTool::Eraser)]);
    }
}
