//! 首页（落地页）：启动时先看到的这一屏 —— 「新建项目」和「画廊（最近工作区）」。
//!
//! 画廊列出**打开中的工作区**（点击切回去）和磁盘上的**最近工作区**（点击打开）；
//! 条目只写一个 [`HomeOpenRequest`] 请求格，宿主
//! （[`crate::app::application`]）取走后切换 / 打开工作区。

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::ui::card::Card as UICard;
use igui_components::{Button, Card, Component, Divider, EmptyState, Flex, NodeRef, Text};
use igui_core::{Color, Edges, EventResult, InputEvent, NodeId, Vec2, ViewportSize};
use igui_render::PaintContext;
use igui_scene::{SceneChild, SceneTree};
use igui_theme::{space, SurfaceLevel, Theme, Tone};
use igui_ui::{Align, Justify, MouseFilter, TextMeasurer};

/// 画廊里的一条：打开中的工作区（`open_index = Some`）或磁盘上的最近文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeEntry {
    pub name: String,
    /// 副标题：打开中显示「打开中」，最近文件显示「宽 × 高 · N 图层」。
    pub detail: String,
    pub open_index: Option<usize>,
    pub path: Option<PathBuf>,
}

/// 画廊里点击留下的请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeOpenRequest {
    /// 切到第 `index` 个已打开的工作区。
    Workspace(usize),
    /// 打开磁盘上的工作区文件。
    Recent(PathBuf),
}

/// 首页视图。一棵自己的 `SceneTree`，只负责排布 / 绘制 / 输入。
pub struct HomeView {
    tree: SceneTree,
    /// 「新建项目」请求。
    new_document: Rc<Cell<bool>>,
    /// 画廊点击请求。
    open_request: Rc<Cell<Option<HomeOpenRequest>>>,
    /// 画廊每条的按钮节点（测试点击用）。
    #[allow(dead_code)]
    entry_buttons: Vec<NodeId>,
    #[allow(dead_code)]
    new_document_button: NodeId,
}

impl HomeView {
    /// 用后端真实字体度量排版。
    pub fn set_text_measurer(&mut self, measurer: Rc<dyn TextMeasurer>) {
        igui_ui::set_text_measurer(&mut self.tree, measurer);
    }

    pub fn new(theme: &'static dyn Theme, entries: Vec<HomeEntry>) -> Self {
        let new_document: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        let open_request: Rc<Cell<Option<HomeOpenRequest>>> = Rc::new(Cell::new(None));
        let button_ref = NodeRef::new();

        let request = new_document.clone();
        let new_window_card = Flex::column().padding(Edges::ZERO).child(
            UICard::new(theme)
                .background(Color::TRANSPARENT)
                .padding(Edges::ZERO)
                .gap(space::SM)
                .child(Text::subheading("新建项目", theme))
                .child(
                    Text::small("在弹出的浮层里选画布尺寸和背景色，然后开始编辑。", theme)
                        .tone(Tone::Muted),
                )
                .child(
                    Flex::row().padding(Edges::ZERO).child(
                        Button::primary("新建项目", theme)
                            .on_click(move |_tree, _id| request.set(true))
                            .ref_(&button_ref),
                    ),
                ),
        );

        let mut entry_refs: Vec<NodeRef> = Vec::new();
        let gallery_list = gallery(theme, &entries, open_request.clone(), &mut entry_refs);
        let gallery_card = Card::new(theme)
            .gap(space::SM)
            .grow(1.0)
            .child(Text::subheading("画廊", theme))
            .child(gallery_list);

        let page = Flex::column()
            .gap(space::XXXL)
            .padding(Edges::all(space::HUGE))
            .mouse_filter(MouseFilter::Ignore)
            .child(
                Flex::row()
                    .child(
                        Flex::column()
                            .gap(space::XS)
                            .padding(Edges::ZERO)
                            .justify(Justify::SpaceBetween)
                            .mouse_filter(MouseFilter::Ignore)
                            .child(Text::title("igui 图像编辑器", theme))
                            .child(
                                Text::small("用 igui 的 2D / UI 栈画的桌面图像编辑器。", theme)
                                    .tone(Tone::Muted),
                            ),
                    )
                    .child(Divider::vertical(theme))
                    .child(new_window_card),
            )
            .child(
                Flex::row()
                    .gap(space::LG)
                    .padding(Edges::ZERO)
                    .grow(1.0)
                    // 默认 Align::Stretch：让画廊卡片撑满这一行的高度。
                    .mouse_filter(MouseFilter::Ignore)
                    .child(gallery_card),
            );

        let tree = Flex::new()
            .gap(0.0)
            .padding(Edges::ZERO)
            .background(theme.surface(SurfaceLevel::Base))
            .mouse_filter(MouseFilter::Ignore)
            .child(page)
            .into_tree();

        let new_document_button = button_ref.get().expect("新建项目按钮已挂载");
        let entry_buttons: Vec<NodeId> = entry_refs.iter().filter_map(|slot| slot.get()).collect();
        Self {
            tree,
            new_document,
            open_request,
            entry_buttons,
            new_document_button,
        }
    }

    /// 这一帧有没有点过「新建项目」；取走后清掉。
    pub fn take_new_document_request(&self) -> bool {
        self.new_document.replace(false)
    }

    /// 画廊点击请求；取走一次。
    pub fn take_open_request(&self) -> Option<HomeOpenRequest> {
        self.open_request.take()
    }

    pub fn event(&mut self, event: &InputEvent) -> EventResult {
        igui_ui::handle_input(&mut self.tree, event)
    }

    pub fn layout(&mut self, viewport: ViewportSize) {
        self.tree.set_viewport_size(viewport.logical_size());
        igui_ui::layout(&mut self.tree, viewport);
        self.tree.update();
    }

    pub fn paint(&self, ctx: &mut PaintContext) {
        self.tree.paint(ctx);
        igui_ui::paint(&self.tree, ctx);
    }

    /// 画廊第 `index` 条的按钮中心点（测试点击用）。
    #[cfg(test)]
    pub fn entry_center(&self, index: usize) -> Option<Vec2> {
        let id = *self.entry_buttons.get(index)?;
        igui_ui::control(&self.tree, id).map(|control| control.rect.center())
    }

    /// 「新建项目」按钮的心点（逻辑坐标）；测试模拟点击用。
    #[allow(dead_code)]
    pub fn new_document_center(&self) -> Option<Vec2> {
        igui_ui::control(&self.tree, self.new_document_button).map(|control| control.rect.center())
    }
}

/// 画廊内容：打开中 + 最近工作区；没有就显示空状态。
fn gallery(
    theme: &'static dyn Theme,
    entries: &[HomeEntry],
    request: Rc<Cell<Option<HomeOpenRequest>>>,
    buttons: &mut Vec<NodeRef>,
) -> impl Component {
    let mut list = Flex::column()
        .gap(space::XS)
        .padding(Edges::ZERO)
        .grow(1.0)
        .mouse_filter(MouseFilter::Ignore);
    if entries.is_empty() {
        list = list.child(
            Flex::column()
                .grow(1.0)
                .align(Align::Center)
                .justify(Justify::Center)
                .child(
                    EmptyState::new("画廊", theme)
                        .description("还没有工作区；点「新建项目」开始，保存后就会出现在这里。"),
                ),
        );
    } else {
        for entry in entries {
            let slot = NodeRef::new();
            buttons.push(slot.clone());
            list = list.child(gallery_row(theme, entry, request.clone(), &slot));
        }
    }
    list
}

/// 画廊里的一行：名字 + 副标题 + 打开 / 切换按钮。
fn gallery_row(
    theme: &'static dyn Theme,
    entry: &HomeEntry,
    request: Rc<Cell<Option<HomeOpenRequest>>>,
    slot: &NodeRef,
) -> impl Component {
    let action = match (entry.open_index, &entry.path) {
        (Some(index), _) => Some(HomeOpenRequest::Workspace(index)),
        (None, Some(path)) => Some(HomeOpenRequest::Recent(path.clone())),
        _ => None,
    };
    let label = if entry.open_index.is_some() {
        "切换"
    } else {
        "打开"
    };
    let mut button = Button::secondary(label, theme);
    if let Some(action) = action {
        button = button.on_click(move |_tree, _id| request.set(Some(action.clone())));
    }
    let button = button.ref_(slot);

    UICard::new(theme)
        .background(Color::TRANSPARENT)
        .padding(Edges::ZERO)
        .gap(space::XXS)
        .child(
            Flex::row()
                .align(Align::Center)
                .justify(Justify::SpaceBetween)
                .padding(Edges::ZERO)
                .mouse_filter(MouseFilter::Ignore)
                .child(
                    Flex::column()
                        .gap(space::XXXS)
                        .padding(Edges::ZERO)
                        .mouse_filter(MouseFilter::Ignore)
                        .child(Text::small(entry.name.clone(), theme))
                        .child(Text::caption(entry.detail.clone(), theme).tone(Tone::Muted)),
                )
                .child(button),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use igui_core::{PointerButton, Size};

    fn viewport() -> ViewportSize {
        ViewportSize::new(Size::new(900.0, 620.0))
    }

    fn click(view: &mut HomeView, position: Vec2) {
        view.event(&InputEvent::PointerDown {
            position,
            button: PointerButton::Left,
        });
        view.event(&InputEvent::PointerUp {
            position,
            button: PointerButton::Left,
        });
    }

    #[test]
    fn the_new_project_button_raises_one_request() {
        let mut view = HomeView::new(crate::theme::editor_theme(false), Vec::new());
        view.layout(viewport());
        let center = view.new_document_center().expect("新建项目按钮");
        click(&mut view, center);
        assert!(view.take_new_document_request(), "点按钮应留下一个请求");
        assert!(!view.take_new_document_request(), "请求只能被取走一次");
    }

    #[test]
    fn the_home_page_shows_both_entries() {
        let mut view = HomeView::new(crate::theme::editor_theme(false), Vec::new());
        view.layout(viewport());
        let mut ctx = PaintContext::new();
        view.paint(&mut ctx);
        let texts: Vec<String> = ctx
            .into_draw_list()
            .commands()
            .iter()
            .filter_map(|command| match command {
                igui_render::DrawCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|text| text == "画廊"), "texts = {texts:?}");
        assert!(
            texts.iter().any(|text| text == "新建项目"),
            "texts = {texts:?}"
        );
    }

    /// 画廊列出条目，点击写打开请求。
    #[test]
    fn clicking_a_gallery_entry_requests_that_workspace() {
        let entries = vec![HomeEntry {
            name: "示例".into(),
            detail: "8 × 6 · 2 图层".into(),
            open_index: None,
            path: Some(PathBuf::from("/tmp/示例.igui")),
        }];
        let mut view = HomeView::new(crate::theme::editor_theme(false), entries);
        view.layout(viewport());
        let center = view.entry_center(0).expect("画廊条目按钮");
        click(&mut view, center);
        assert_eq!(
            view.take_open_request(),
            Some(HomeOpenRequest::Recent(PathBuf::from("/tmp/示例.igui")))
        );
    }
}
