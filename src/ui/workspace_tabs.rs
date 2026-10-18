//! 工作区标签条：一行工作区标签 + 「主页」+「+」新建。
//!
//! 这是**应用层**的小视图（自带 `SceneTree`），放在编辑器上方一条
//! [`TAB_STRIP_HEIGHT`] 高的区域里；`EditorView` 用顶部预留把内容让开。点击只写
//! 一个请求格（[`WorkspaceTabAction`]），宿主取走执行——回调拿不到 `&mut self`。

use std::cell::Cell;
use std::rc::Rc;

use igui_components::{Button, Component, Flex, NodeRef};
use igui_core::{Color, Edges, EventResult, InputEvent, NodeId, Vec2, ViewportSize};
use igui_render::PaintContext;
use igui_scene::SceneTree;
use igui_theme::{radius, space, SurfaceLevel, Theme};
use igui_ui::{Align, MouseFilter, SizeBasis, SurfaceStyle, TextMeasurer};

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

    pub fn layout(&mut self, viewport: ViewportSize) {
        self.tree.set_viewport_size(viewport.logical_size());
        igui_ui::layout(&mut self.tree, viewport);
        self.tree.update();
    }

    pub fn event(&mut self, event: &InputEvent) -> EventResult {
        igui_ui::handle_input(&mut self.tree, event)
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
            .basis(SizeBasis::Px(TAB_STRIP_HEIGHT))
            .grow(0.0)
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
        self.tree.add_child(layer, bar);

        self.home_node = home_slot.get();
        self.new_node = new_slot.get();
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
