//! 工作区标签条：一行工作区标签 + 「主页」+「+」新建。
//!
//! 这是**应用层**的小视图（自带 `SceneTree`），放在编辑器上方一条
//! [`TAB_STRIP_HEIGHT`] 高的区域里；`EditorView` 用顶部预留把内容让开。点击只写
//! 一个请求格（[`WorkspaceTabAction`]），宿主取走执行——回调拿不到 `&mut self`。

use std::cell::Cell;
use std::rc::Rc;

use igui_components::{update_control, Button, Component, Flex, NodeRef};
use igui_core::{Color, Edges, EventResult, InputEvent, NodeId, Rect, Vec2, ViewportSize};
use igui_render::PaintContext;
use igui_scene::SceneTree;
use igui_theme::{radius, space, SurfaceLevel, Theme};
use igui_ui::{Align, MouseFilter, SurfaceStyle, TextMeasurer};

/// 标签条高度（逻辑像素）。
pub const TAB_STRIP_HEIGHT: f32 = 32.0;

/// 点标签条留下的请求。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceTabAction {
    Switch(usize),
    Close(usize),
    New,
    Home,
}

/// 一个标签要显示的信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceTab {
    pub name: String,
    pub dirty: bool,
}

/// 工作区标签条视图。
pub struct WorkspaceTabs {
    theme: &'static dyn Theme,
    tree: SceneTree,
    request: Rc<Cell<Option<WorkspaceTabAction>>>,
    tabs: Vec<WorkspaceTab>,
    active: usize,
    /// 标签条节点的顶部偏移（菜单栏高度；宿主每帧同步）。
    top: f32,
    /// 标签条节点（判断指针是否落在条上）。
    bar: Option<NodeId>,
    /// 这次按下的起点在标签条上（后续 move / up 归标签条）。
    captured: bool,
    tab_nodes: Vec<NodeId>,
    home_node: Option<NodeId>,
    new_node: Option<NodeId>,
}

impl WorkspaceTabs {
    pub fn new(theme: &'static dyn Theme) -> Self {
        let mut tabs = Self {
            theme,
            tree: SceneTree::new(),
            request: Rc::new(Cell::new(None)),
            tabs: Vec::new(),
            active: 0,
            top: 0.0,
            bar: None,
            captured: false,
            tab_nodes: Vec::new(),
            home_node: None,
            new_node: None,
        };
        tabs.rebuild();
        tabs
    }

    /// 用后端真实字体度量排版。
    pub fn set_text_measurer(&mut self, measurer: Rc<dyn TextMeasurer>) {
        igui_ui::set_text_measurer(&mut self.tree, measurer);
    }

    /// 这一帧有没有标签条请求；取走一次。
    pub fn take_request(&self) -> Option<WorkspaceTabAction> {
        self.request.take()
    }

    /// 更新标签列表 / 激活项；内容没变就不重建。
    pub fn set_tabs(&mut self, tabs: &[WorkspaceTab], active: usize) {
        if self.tabs == tabs && self.active == active {
            return;
        }
        self.tabs = tabs.to_vec();
        self.active = active;
        self.rebuild();
    }

    /// 把标签条钉到 `top`（逻辑像素）：菜单栏在最上时，它就在菜单栏下面。
    pub fn set_top(&mut self, top: f32) {
        if (self.top - top).abs() < f32::EPSILON {
            return;
        }
        self.top = top;
        if let Some(bar) = self.bar {
            update_control(&mut self.tree, bar, |data| {
                data.offsets = Edges::new(0.0, self.top, 0.0, self.top + TAB_STRIP_HEIGHT);
            });
        }
    }

    /// 标签条当前占的那一条（逻辑坐标）；宿主 / 自检用它验证顺序。
    pub fn bar_rect(&self) -> Option<Rect> {
        self.bar
            .and_then(|id| igui_ui::control(&self.tree, id))
            .map(|control| control.rect)
    }

    pub fn layout(&mut self, viewport: ViewportSize) {
        self.tree.set_viewport_size(viewport.logical_size());
        igui_ui::layout(&mut self.tree, viewport);
        self.tree.update();
    }

    /// 路由一个事件。
    ///
    /// 只有**落在标签条上**（或从标签条上按下、仍被捕获）的指针事件才算被
    /// 标签条处理：`igui_ui::handle_input` 对每个 `PointerUp` 都报 `Handled`，
    /// 不这样过滤会把编辑器的松手事件吞掉，工具按钮就永远不触发。
    /// 指针下的控件光标（宿主选光标用）。
    pub fn hovered_cursor(&self) -> igui_core::Cursor {
        igui_ui::hovered_cursor(&self.tree)
    }

    pub fn event(&mut self, event: &InputEvent) -> EventResult {
        if let InputEvent::PointerDown { position, .. } = event {
            self.captured = self.bar_contains(*position);
        }
        let inside = event
            .position()
            .is_some_and(|position| self.bar_contains(position));
        let _ = igui_ui::handle_input(&mut self.tree, event);
        if let InputEvent::PointerUp { .. } = event {
            let captured = std::mem::take(&mut self.captured);
            return if captured || inside {
                EventResult::Handled
            } else {
                EventResult::Ignored
            };
        }
        if event.position().is_some() {
            return if self.captured || inside {
                EventResult::Handled
            } else {
                EventResult::Ignored
            };
        }
        EventResult::Ignored
    }

    /// 指针是否落在标签条那条上。
    fn bar_contains(&self, position: Vec2) -> bool {
        self.bar
            .and_then(|id| igui_ui::control(&self.tree, id))
            .is_some_and(|control| control.rect.contains(position))
    }

    pub fn paint(&self, ctx: &mut PaintContext) {
        self.tree.paint(ctx);
        igui_ui::paint(&self.tree, ctx);
    }

    /// 第 `index` 个标签的中心点（测试 / 自检点击用）。
    #[cfg(test)]
    pub fn tab_center(&self, index: usize) -> Option<Vec2> {
        let id = *self.tab_nodes.get(index)?;
        igui_ui::control(&self.tree, id).map(|control| control.rect.center())
    }

    #[cfg(test)]
    pub fn new_center(&self) -> Option<Vec2> {
        let id = self.new_node?;
        igui_ui::control(&self.tree, id).map(|control| control.rect.center())
    }

    #[cfg(test)]
    pub fn home_center(&self) -> Option<Vec2> {
        let id = self.home_node?;
        igui_ui::control(&self.tree, id).map(|control| control.rect.center())
    }

    fn rebuild(&mut self) {
        self.tree = SceneTree::new();
        self.tab_nodes.clear();
        let theme = self.theme;
        let request = self.request.clone();
        let active = self.active;
        let tabs = self.tabs.clone();

        let mut bar = Flex::row()
            // 标签条的父节点是根控件，子节点按 **anchors/offsets** 摆（不是 flex），
            // 所以这里显式把它钉成顶部一条，否则它会撑满整个视口。
            .anchors(Edges::new(0.0, 0.0, 1.0, 0.0))
            .offsets(Edges::new(0.0, self.top, 0.0, self.top + TAB_STRIP_HEIGHT))
            .align(Align::Center)
            .gap(space::XXS)
            .padding(Edges::symmetric(space::SM, space::XXS))
            .background(theme.surface(SurfaceLevel::Surface))
            .foreground(move |ctx, rect, _| {
                ctx.draw_line(
                    Vec2::new(rect.left(), rect.bottom() - 0.5),
                    Vec2::new(rect.right(), rect.bottom() - 0.5),
                    1.0,
                    theme.palette().border,
                );
            })
            .mouse_filter(MouseFilter::Ignore);

        let home_slot = NodeRef::new();
        let home_request = request.clone();
        bar = bar.child(
            Button::ghost("主页", theme)
                .on_click(move |_tree, _id| home_request.set(Some(WorkspaceTabAction::Home)))
                .ref_(&home_slot),
        );

        let mut tab_nodes = Vec::with_capacity(tabs.len());
        for (index, tab) in tabs.iter().enumerate() {
            let slot = NodeRef::new();
            tab_nodes.push(slot.clone());
            bar = bar.child(tab_item(theme, index, tab, active, request.clone(), &slot));
        }

        let new_slot = NodeRef::new();
        let new_request = request.clone();
        bar = bar.child(
            Button::ghost("+ 新建", theme)
                .on_click(move |_tree, _id| new_request.set(Some(WorkspaceTabAction::New)))
                .ref_(&new_slot),
        );

        let root = self.tree.root();
        let layer = self
            .tree
            .add_child(root, Flex::column().mouse_filter(MouseFilter::Ignore));
        let bar = bar.build(&mut self.tree, layer);

        self.home_node = home_slot.get();
        self.new_node = new_slot.get();
        self.bar = Some(bar);
        self.tab_nodes = tab_nodes.iter().filter_map(|slot| slot.get()).collect();
    }
}

/// 一个标签：名字按钮（切换 + 激活高亮）+ 关闭按钮。
fn tab_item(
    theme: &'static dyn Theme,
    index: usize,
    tab: &WorkspaceTab,
    active: usize,
    request: Rc<Cell<Option<WorkspaceTabAction>>>,
    slot: &NodeRef,
) -> impl Component {
    let switch_request = request.clone();
    let close_request = request;
    let label = format!("{}{}", if tab.dirty { "● " } else { "" }, tab.name);
    Flex::row()
        .align(Align::Center)
        .gap(space::XXXS)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(
            Button::ghost(label, theme)
                .on_click(move |_tree, _id| {
                    switch_request.set(Some(WorkspaceTabAction::Switch(index)));
                })
                .dynamic_background(move |interact| {
                    if index == active {
                        SurfaceStyle::new(theme.palette().selection).radius(radius::SM)
                    } else if interact.hovered || interact.pressed {
                        SurfaceStyle::new(theme.palette().surface_hover).radius(radius::SM)
                    } else {
                        SurfaceStyle::new(Color::TRANSPARENT)
                    }
                })
                .ref_(slot),
        )
        .child(
            Button::ghost("×", theme)
                .font_size(igui_theme::TextSize::Small.px())
                .on_click(move |_tree, _id| {
                    close_request.set(Some(WorkspaceTabAction::Close(index)));
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use igui_core::{PointerButton, Size};

    fn viewport() -> ViewportSize {
        ViewportSize::new(Size::new(600.0, 400.0))
    }

    fn click(tabs: &mut WorkspaceTabs, position: Vec2) {
        tabs.event(&InputEvent::PointerDown {
            position,
            button: PointerButton::Left,
        });
        tabs.event(&InputEvent::PointerUp {
            position,
            button: PointerButton::Left,
        });
        tabs.layout(viewport());
    }

    #[test]
    fn clicking_a_tab_requests_a_switch() {
        let mut tabs = WorkspaceTabs::new(crate::theme::editor_theme(false));
        tabs.set_tabs(
            &[
                WorkspaceTab {
                    name: "A".into(),
                    dirty: false,
                },
                WorkspaceTab {
                    name: "B".into(),
                    dirty: true,
                },
            ],
            0,
        );
        tabs.layout(viewport());
        let center = tabs.tab_center(1).expect("第二个标签");
        click(&mut tabs, center);
        assert_eq!(tabs.take_request(), Some(WorkspaceTabAction::Switch(1)));
    }

    #[test]
    fn the_new_button_requests_a_new_workspace() {
        let mut tabs = WorkspaceTabs::new(crate::theme::editor_theme(false));
        tabs.layout(viewport());
        let center = tabs.new_center().expect("新建按钮");
        click(&mut tabs, center);
        assert_eq!(tabs.take_request(), Some(WorkspaceTabAction::New));
    }

    #[test]
    fn the_home_button_requests_home() {
        let mut tabs = WorkspaceTabs::new(crate::theme::editor_theme(false));
        tabs.layout(viewport());
        let center = tabs.home_center().expect("主页按钮");
        click(&mut tabs, center);
        assert_eq!(tabs.take_request(), Some(WorkspaceTabAction::Home));
    }

    /// 标签条下方的指针事件必须透传（`igui_ui` 对每个 PointerUp 都报 Handled）。
    #[test]
    fn pointer_events_below_the_bar_fall_through() {
        let mut tabs = WorkspaceTabs::new(crate::theme::editor_theme(false));
        tabs.set_tabs(
            &[WorkspaceTab {
                name: "A".into(),
                dirty: false,
            }],
            0,
        );
        tabs.layout(viewport());
        let below = Vec2::new(20.0, 200.0);
        assert_eq!(
            tabs.event(&InputEvent::PointerDown {
                position: below,
                button: PointerButton::Left,
            }),
            EventResult::Ignored
        );
        assert_eq!(
            tabs.event(&InputEvent::PointerUp {
                position: below,
                button: PointerButton::Left,
            }),
            EventResult::Ignored,
            "松手必须透传，否则编辑器的笔画 / 工具按钮永远收不到"
        );
    }

    #[test]
    fn the_dirty_flag_shows_in_the_label() {
        let mut tabs = WorkspaceTabs::new(crate::theme::editor_theme(false));
        tabs.set_tabs(
            &[WorkspaceTab {
                name: "画布".into(),
                dirty: true,
            }],
            0,
        );
        tabs.layout(viewport());
        let mut ctx = PaintContext::new();
        tabs.paint(&mut ctx);
        let texts: Vec<String> = ctx
            .into_draw_list()
            .commands()
            .iter()
            .filter_map(|command| match command {
                igui_render::DrawCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            texts.iter().any(|text| text == "● 画布"),
            "texts = {texts:?}"
        );
    }
}
