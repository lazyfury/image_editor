//! 应用内「窗口」组件：模拟桌面的浮动窗口 —— 标题栏可拖动移动，四条边 / 角
//! 可拖动缩放，右上一个关闭按钮。
//!
//! 它自己持有一棵 `SceneTree`（跟 [`crate::ui::HomeView`] /
//! [`crate::ui::EditorView`] 一样只负责排布 / 绘制 / 输入），由宿主在底层视图
//! 之后 `layout` / `paint`，并在有窗口时把输入优先交给它。
//!
//! 拖动 / 缩放是在宿主这一层按几何命中做的，而不是靠子控件的 `on_click`：
//! 标题栏与边框是「空」的（`MouseFilter::Ignore`），指针落在上面时窗口自己起一个
//! [`Gesture`]；落在内容控件（按钮 / 色块）上则转发给树，不抢交互。
//!
//! 结构固定为三段：**标题栏**（贴边、只有标题左边距 + 关闭按钮贴右）、
//! **内容区**（`grow`，自带 padding）和**页脚**（按钮右对齐）。
//!
//! [`NewDocumentModal`](crate::ui::NewDocumentModal) 与
//! [`ConfirmDialog`](crate::ui::ConfirmDialog) 是它的使用者。

use std::cell::Cell;
use std::rc::Rc;

use igui_components::{set_text, update_control, Component, Flex, NodeRef, Text};
use igui_core::{
    Color, Edges, EventResult, InputEvent, Key, NodeId, PointerButton, Rect, Size, Vec2,
    ViewportSize,
};
use igui_render::PaintContext;
use igui_scene::SceneTree;
use igui_theme::{radius, space, SurfaceLevel, Theme};
use igui_ui::{Align, InteractState, Justify, MouseFilter, SizeBasis, SurfaceStyle, TextMeasurer};

mod geometry;

use geometry::{clamp_to_viewport, resize_edges, resize_rect, ResizeEdges};

/// 标题栏高度（逻辑像素）。
const TITLE_BAR_HEIGHT: f32 = 34.0;
/// 标题文字左边距。
const TITLE_PADDING_X: f32 = 12.0;
/// 关闭按钮边长（正方，占满标题栏高度）。
const CLOSE_BUTTON: f32 = 34.0;
/// 关闭按钮悬停时的底色（Windows 风格的红）。
const CLOSE_HOVER: Color = Color::new(0.86, 0.22, 0.20, 1.0);
/// 页脚按钮的最小宽度：让「保存 / 不保存 / 取消」看起来一样大
/// （否则宽度会跟着文字长度与字形宽度跑，长短不一）。
pub const FOOTER_BUTTON_MIN_WIDTH: f32 = 72.0;
/// 模态遮罩。
const SCRIM: Color = Color::new(0.0, 0.0, 0.0, 0.35);
/// 窗口底色。用**最底层**而不是 `Raised`：`Button` 的 secondary / ghost 填充
/// 就是 `surface_raised`，窗口也是 `Raised` 的话按钮只剩描边、看着比实心按钮小。
const WINDOW_SURFACE: SurfaceLevel = SurfaceLevel::Base;

/// 指针按下的窗口手势。
#[derive(Clone, Copy, Default, PartialEq, Debug)]
enum Gesture {
    #[default]
    None,
    /// 标题栏拖动：记录按下时的窗口矩形与指针，移动时整体平移。
    Move {
        start_rect: Rect,
        start_pointer: Vec2,
    },
    /// 边框缩放：记录抓的是哪几条边。
    Resize {
        edges: ResizeEdges,
        start_rect: Rect,
        start_pointer: Vec2,
    },
}

/// 一个浮动窗口。
pub struct Window {
    tree: SceneTree,
    /// 窗口外框（定位 / 尺寸都由它的 `offsets` 决定）。
    frame: NodeId,
    /// 内容区 / 页脚节点（布局回归测试用）。
    #[cfg_attr(not(test), allow(dead_code))]
    content: NodeId,
    #[cfg_attr(not(test), allow(dead_code))]
    footer: NodeId,
    /// 关闭请求：关闭按钮、Esc、点窗口外都会置位，宿主取走一次。
    close: Rc<Cell<bool>>,
    /// 当前窗口矩形（逻辑坐标）。
    rect: Cell<Rect>,
    /// 初始尺寸（居中用）。
    initial_size: Vec2,
    /// 拖动缩放的下限。
    min_size: Vec2,
    /// 最近一次 `layout` 的视口，事件里钳制用。
    viewport: Cell<ViewportSize>,
    /// 是否已经居中定位过（首帧之后不再自动居中）。
    placed: Cell<bool>,
    gesture: Cell<Gesture>,
}

impl Window {
    /// 建一个窗口：标题栏 + 内容区 + 页脚。
    ///
    /// `size` 是初始尺寸（首帧在视口里居中），`min_size` 是拖动缩放的
    /// 下限；实际尺寸还会被内容固有尺寸撑大。`build` 在树建好后拿到
    /// `(tree, content, footer)`：正文加进 `content`，操作按钮加进 `footer`。
    pub fn new(
        theme: &'static dyn Theme,
        title: &str,
        size: Vec2,
        min_size: Vec2,
        build: impl FnOnce(&mut SceneTree, NodeId, NodeId),
    ) -> Self {
        let close: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        let palette = theme.palette();
        let content_ref = NodeRef::new();
        let footer_ref = NodeRef::new();

        // 标题栏：外框不设 padding，标题只留左边距，关闭按钮贴右，下沿一条细线。
        let header = Flex::row()
            .basis(SizeBasis::Px(TITLE_BAR_HEIGHT))
            .grow(0.0)
            .gap(0.0)
            .align(Align::Center)
            .padding(Edges::new(TITLE_PADDING_X, 0.0, 0.0, 0.0))
            .mouse_filter(MouseFilter::Ignore)
            .foreground(move |ctx, rect, _| {
                // 标题栏下沿一条细线，把可拖区域画清楚。
                ctx.draw_line(
                    Vec2::new(rect.left(), rect.bottom() - 0.5),
                    Vec2::new(rect.right(), rect.bottom() - 0.5),
                    1.0,
                    palette.border_subtle,
                );
            })
            .child(
                Text::small(title, theme)
                    .color(palette.foreground)
                    .grow(1.0)
                    // 标题不能抢指针，否则抓标题文字拖不动窗口。
                    .mouse_filter(MouseFilter::Ignore),
            )
            .child(close_button(theme, close.clone()));

        // 内容区：撑满标题栏与页脚之间的空间。
        let content = Flex::column()
            .grow(1.0)
            .gap(space::LG)
            .padding(Edges::all(space::LG))
            .mouse_filter(MouseFilter::Ignore)
            .ref_(&content_ref);

        // 页脚：操作按钮右对齐，上沿一条细线。
        let footer = Flex::row()
            .grow(0.0)
            .justify(Justify::End)
            .gap(space::SM)
            .padding(Edges::symmetric(space::LG, space::MD))
            .mouse_filter(MouseFilter::Ignore)
            .foreground(move |ctx, rect, _| {
                ctx.draw_line(
                    Vec2::new(rect.left(), rect.top() + 0.5),
                    Vec2::new(rect.right(), rect.top() + 0.5),
                    1.0,
                    palette.border_subtle,
                );
            })
            .ref_(&footer_ref);

        let frame = Flex::column()
            .anchors(Edges::ZERO)
            .offsets(Edges::new(0.0, 0.0, size.x, size.y))
            .min_size(min_size.x, min_size.y)
            // `Flex` 默认 padding 16 / gap 8；外框必须显式清零，否则标题栏、
            // 页脚都会被缩进，和内容区的 padding 重复。
            .padding(Edges::ZERO)
            .gap(0.0)
            .surface(
                SurfaceStyle::new(theme.surface(WINDOW_SURFACE))
                    .border(palette.border)
                    .radius(radius::PANEL),
            )
            .mouse_filter(MouseFilter::Ignore)
            .child(header)
            .child(content)
            .child(footer);

        let mut tree = SceneTree::new();
        let layer = tree.add_child(
            tree.root(),
            Flex::column()
                .padding(Edges::ZERO)
                .gap(0.0)
                .mouse_filter(MouseFilter::Ignore),
        );
        let frame = tree.add_child(layer, frame);
        let content_id = content_ref.get().expect("内容区已挂载");
        let footer_id = footer_ref.get().expect("页脚已挂载");
        build(&mut tree, content_id, footer_id);

        Self {
            tree,
            frame,
            content: content_id,
            footer: footer_id,
            close,
            rect: Cell::new(Rect::from_min_size(Vec2::ZERO, Size::new(size.x, size.y))),
            initial_size: size,
            min_size,
            viewport: Cell::new(ViewportSize::default()),
            placed: Cell::new(false),
            gesture: Cell::new(Gesture::None),
        }
    }

    /// 用后端真实字体度量排版。
    pub fn set_text_measurer(&mut self, measurer: Rc<dyn TextMeasurer>) {
        igui_ui::set_text_measurer(&mut self.tree, measurer);
    }

    /// 这一帧有没有收到关闭请求（关闭按钮 / Esc / 点窗口外）；取走后清掉。
    pub fn take_close_request(&self) -> bool {
        self.close.replace(false)
    }

    /// 当前窗口矩形（布局之后有效）。
    #[cfg(test)]
    pub fn rect(&self) -> Rect {
        self.rect.get()
    }

    /// 底层树（测试读布局矩形用）。
    #[cfg(test)]
    pub fn tree(&self) -> &SceneTree {
        &self.tree
    }

    /// 改写窗口里某个文本节点（不重建树）。
    pub fn set_text(&mut self, id: NodeId, text: impl Into<String>) {
        set_text(&mut self.tree, id, text);
    }

    pub fn layout(&mut self, viewport: ViewportSize) {
        self.viewport.set(viewport);
        self.tree.set_viewport_size(viewport.logical_size());

        if self.placed.replace(true) {
            self.rect
                .set(clamp_to_viewport(self.rect.get(), viewport.logical_size()));
        } else {
            let rect = Rect::from_center_size(
                viewport.logical_rect().center(),
                Size::new(self.initial_size.x, self.initial_size.y),
            );
            self.rect
                .set(clamp_to_viewport(rect, viewport.logical_size()));
        }

        self.apply_offsets();
        igui_ui::layout(&mut self.tree, viewport);

        // 内容固有尺寸可能把外框撑得比请求的大；以实际外框为准，但保持中心不变
        // （否则窗口会从左上角往下 / 往右长，看起来没居中）。
        if let Some(control) = igui_ui::control(&self.tree, self.frame) {
            let actual = control.rect;
            if actual != self.rect.get() {
                let center = self.rect.get().center();
                let recentered = Rect::from_center_size(center, actual.size);
                self.rect
                    .set(clamp_to_viewport(recentered, viewport.logical_size()));
                self.apply_offsets();
                igui_ui::layout(&mut self.tree, viewport);
            }
        }
        self.tree.update();
    }

    /// 处理一个事件。窗口是模态的：**所有**输入都被它吞掉。
    pub fn event(&mut self, event: &InputEvent) -> EventResult {
        if let InputEvent::KeyDown { key: Key::Escape } = event {
            self.close.set(true);
            return EventResult::Handled;
        }

        match event {
            InputEvent::PointerMove { position } => {
                if self.apply_gesture(*position) {
                    return EventResult::Handled;
                }
            }
            InputEvent::PointerUp { .. } => {
                if !matches!(self.gesture.get(), Gesture::None) {
                    self.gesture.set(Gesture::None);
                    return EventResult::Handled;
                }
            }
            InputEvent::PointerDown { position, button }
                if *button == PointerButton::Left && self.begin_gesture(*position) =>
            {
                return EventResult::Handled;
            }
            _ => {}
        }

        // 内容控件（按钮 / 色块 / 输入框）照常处理；其余输入到此为止。
        igui_ui::handle_input(&mut self.tree, event);
        EventResult::Handled
    }

    pub fn paint(&self, ctx: &mut PaintContext) {
        ctx.fill_rect(self.viewport.get().logical_rect(), SCRIM);
        self.tree.paint(ctx);
        igui_ui::paint(&self.tree, ctx);
    }

    /// 指针在窗口里的落点：点交互控件返回 `false`（交给树），否则起手势。
    fn begin_gesture(&mut self, position: Vec2) -> bool {
        let rect = self.rect.get();
        if !rect.contains(position) {
            self.close.set(true);
            return true;
        }
        if igui_ui::hit_test(&self.tree, position).is_some() {
            return false;
        }
        if let Some(edges) = resize_edges(rect, position) {
            self.gesture.set(Gesture::Resize {
                edges,
                start_rect: rect,
                start_pointer: position,
            });
            return true;
        }
        if position.y <= rect.top() + TITLE_BAR_HEIGHT {
            self.gesture.set(Gesture::Move {
                start_rect: rect,
                start_pointer: position,
            });
            return true;
        }
        false
    }

    /// 有手势时按指针更新矩形；返回是否消费了这次移动。
    fn apply_gesture(&mut self, position: Vec2) -> bool {
        let viewport = self.viewport.get().logical_size();
        match self.gesture.get() {
            Gesture::Move {
                start_rect,
                start_pointer,
            } => {
                let moved = start_rect.translate(position - start_pointer);
                self.rect.set(clamp_to_viewport(moved, viewport));
                true
            }
            Gesture::Resize {
                edges,
                start_rect,
                start_pointer,
            } => {
                let resized = resize_rect(
                    start_rect,
                    edges,
                    position - start_pointer,
                    self.min_size,
                    viewport,
                );
                self.rect.set(resized);
                true
            }
            Gesture::None => false,
        }
    }

    fn apply_offsets(&mut self) {
        let rect = self.rect.get();
        update_control(&mut self.tree, self.frame, |data| {
            data.anchors = Edges::ZERO;
            data.offsets = Edges::new(rect.left(), rect.top(), rect.right(), rect.bottom());
        });
    }
}

/// 右上角的关闭按钮：悬停变红，前景画一个叉。
fn close_button(theme: &'static dyn Theme, close: Rc<Cell<bool>>) -> impl Component {
    let foreground = theme.palette().foreground;
    let on_accent = theme.palette().on_accent;
    Flex::new()
        .min_size(CLOSE_BUTTON, CLOSE_BUTTON)
        .basis(SizeBasis::Px(CLOSE_BUTTON))
        .shrink(0.0)
        .align(Align::Center)
        .justify(Justify::Center)
        .mouse_filter(MouseFilter::Stop)
        .on_click(move |_tree, _id| close.set(true))
        .dynamic_background(move |state| {
            if hovered(state) {
                SurfaceStyle::new(CLOSE_HOVER)
            } else {
                SurfaceStyle::new(Color::TRANSPARENT)
            }
        })
        .foreground(move |ctx, rect, state| {
            let color = if hovered(state) {
                on_accent
            } else {
                foreground
            };
            let center = rect.center();
            let arm = 5.0;
            ctx.draw_line(
                center + Vec2::new(-arm, -arm),
                center + Vec2::new(arm, arm),
                1.5,
                color,
            );
            ctx.draw_line(
                center + Vec2::new(arm, -arm),
                center + Vec2::new(-arm, arm),
                1.5,
                color,
            );
        })
}

fn hovered(state: InteractState) -> bool {
    state.hovered || state.pressed
}

#[cfg(test)]
mod tests {
    use super::*;
    use igui_components::Button;
    use std::cell::RefCell;

    fn viewport() -> ViewportSize {
        ViewportSize::new(Size::new(800.0, 600.0))
    }

    /// 一个带「点我」按钮的窗口，按钮回调累计点击次数。
    fn window_with_button() -> (Window, Rc<RefCell<u32>>, NodeRef) {
        let clicks: Rc<RefCell<u32>> = Rc::new(RefCell::new(0));
        let counter = clicks.clone();
        let button_ref = NodeRef::new();
        let slot = button_ref.clone();
        let window = Window::new(
            crate::theme::editor_theme(false),
            "标题",
            Vec2::new(420.0, 320.0),
            Vec2::new(320.0, 240.0),
            move |tree, content, _footer| {
                let counter = counter.clone();
                tree.add_child(
                    content,
                    Flex::column().padding(Edges::ZERO).child(
                        Button::secondary("点我", crate::theme::editor_theme(false))
                            .on_click(move |_tree, _id| *counter.borrow_mut() += 1)
                            .ref_(&slot),
                    ),
                );
            },
        );
        (window, clicks, button_ref)
    }

    fn press(window: &mut Window, position: Vec2) {
        window.event(&InputEvent::PointerDown {
            position,
            button: PointerButton::Left,
        });
    }

    fn release(window: &mut Window, position: Vec2) {
        window.event(&InputEvent::PointerUp {
            position,
            button: PointerButton::Left,
        });
    }

    fn click(window: &mut Window, position: Vec2) {
        press(window, position);
        release(window, position);
        window.layout(viewport());
    }

    #[test]
    fn the_window_starts_centered() {
        let (mut window, _, _) = window_with_button();
        window.layout(viewport());
        let rect = window.rect();
        let offset = rect.center() - viewport().logical_rect().center();
        assert!(
            offset.x.abs() < 2.0 && offset.y.abs() < 2.0,
            "窗口应大致居中：{offset:?}"
        );
        assert!(rect.size.width >= 420.0, "size = {:?}", rect.size);
        assert!(rect.size.height >= 320.0, "size = {:?}", rect.size);
    }

    #[test]
    fn dragging_the_title_bar_moves_the_window() {
        let (mut window, _, _) = window_with_button();
        window.layout(viewport());
        let before = window.rect();
        let grab = Vec2::new(before.center().x, before.top() + 10.0);

        press(&mut window, grab);
        window.event(&InputEvent::PointerMove {
            position: grab + Vec2::new(-40.0, 30.0),
        });
        window.layout(viewport());
        let after = window.rect();

        assert_eq!(after.origin, before.origin + Vec2::new(-40.0, 30.0));
        assert_eq!(after.size, before.size, "拖动不该改尺寸");
        release(&mut window, grab + Vec2::new(-40.0, 30.0));
    }

    #[test]
    fn dragging_the_bottom_right_corner_resizes_the_window() {
        let (mut window, _, _) = window_with_button();
        window.layout(viewport());
        let before = window.rect();
        let grab = Vec2::new(before.right() - 2.0, before.bottom() - 2.0);

        press(&mut window, grab);
        window.event(&InputEvent::PointerMove {
            position: grab + Vec2::new(50.0, 40.0),
        });
        window.layout(viewport());
        let after = window.rect();

        assert_eq!(after.origin, before.origin, "右下角缩放不动左上角");
        assert_eq!(
            after.size,
            Size::new(before.size.width + 50.0, before.size.height + 40.0)
        );
        release(&mut window, grab + Vec2::new(50.0, 40.0));
    }

    #[test]
    fn resizing_from_the_left_edge_moves_the_left_edge_only() {
        let (mut window, _, _) = window_with_button();
        window.layout(viewport());
        let before = window.rect();
        let grab = Vec2::new(before.left() + 2.0, before.center().y);

        press(&mut window, grab);
        window.event(&InputEvent::PointerMove {
            position: grab + Vec2::new(20.0, 0.0),
        });
        window.layout(viewport());
        let after = window.rect();

        assert_eq!(after.left(), before.left() + 20.0);
        assert_eq!(after.right(), before.right());
    }

    #[test]
    fn resizing_cannot_go_below_the_minimum() {
        let (mut window, _, _) = window_with_button();
        window.layout(viewport());
        let before = window.rect();
        let grab = Vec2::new(before.right() - 2.0, before.bottom() - 2.0);

        press(&mut window, grab);
        window.event(&InputEvent::PointerMove {
            position: grab - Vec2::new(10_000.0, 10_000.0),
        });
        window.layout(viewport());
        let after = window.rect();

        assert!(after.size.width >= 320.0 - 0.5, "size = {:?}", after.size);
        assert!(after.size.height >= 240.0 - 0.5, "size = {:?}", after.size);
    }

    #[test]
    fn a_press_on_window_content_reaches_the_buttons() {
        let (mut window, clicks, button) = window_with_button();
        window.layout(viewport());
        let rect = window.rect();
        let button = igui_ui::control(&window.tree, button.get().expect("按钮已挂载"))
            .map(|control| control.rect)
            .expect("按钮 rect");
        let position = button.center();
        assert!(rect.contains(position));
        click(&mut window, position);

        assert_eq!(*clicks.borrow(), 1, "按钮应收到点击");
        assert_eq!(window.rect().origin, rect.origin, "点按钮不该拖动窗口");
    }

    #[test]
    fn a_press_outside_the_window_requests_close() {
        let (mut window, _, _) = window_with_button();
        window.layout(viewport());
        let outside = Vec2::new(5.0, 5.0);
        assert!(!window.rect().contains(outside));
        press(&mut window, outside);
        assert!(window.take_close_request());
        assert!(!window.take_close_request(), "请求只能取走一次");
    }

    #[test]
    fn escape_requests_close() {
        let (mut window, _, _) = window_with_button();
        window.layout(viewport());
        window.event(&InputEvent::KeyDown { key: Key::Escape });
        assert!(window.take_close_request());
    }

    #[test]
    fn the_close_button_requests_close() {
        let (mut window, _, _) = window_with_button();
        window.layout(viewport());
        let rect = window.rect();
        let close = Vec2::new(
            rect.right() - CLOSE_BUTTON / 2.0,
            rect.top() + TITLE_BAR_HEIGHT / 2.0,
        );
        click(&mut window, close);
        assert!(window.take_close_request());
    }

    /// 窗口底色必须与 secondary / ghost 按钮的填充不同，否则按钮只剩描边，
    /// 看起来比实心按钮小一圈。
    #[test]
    fn the_window_surface_differs_from_the_button_fill() {
        let theme = crate::theme::editor_theme(false);
        assert_ne!(
            theme.surface(WINDOW_SURFACE),
            theme.palette().surface_raised,
            "对话框底色不能和按钮填充一样"
        );
    }

    /// 内容区紧贴标题栏下沿与外框两边（外框不能自带 padding）。
    #[test]
    fn the_content_is_flush_with_the_header_and_the_frame() {
        let (mut window, _, _) = window_with_button();
        window.layout(viewport());
        let rect = window.rect();
        let content = igui_ui::control(&window.tree, window.content)
            .map(|control| control.rect)
            .expect("内容区 rect");
        let footer = igui_ui::control(&window.tree, window.footer)
            .map(|control| control.rect)
            .expect("页脚 rect");
        // 外框不能自带 padding（`Flex` 默认 16），否则标题栏 / 页脚会被缩进。
        assert_eq!(content.left(), rect.left(), "内容区应贴左边");
        assert_eq!(content.right(), rect.right(), "内容区应贴右边");
        assert_eq!(
            content.top(),
            rect.top() + TITLE_BAR_HEIGHT,
            "内容区应紧贴标题栏下沿"
        );
        assert_eq!(footer.left(), rect.left(), "页脚应贴左边");
        assert_eq!(footer.right(), rect.right(), "页脚应贴右边");
        assert_eq!(footer.bottom(), rect.bottom(), "页脚应贴底边");
    }

    /// 页脚是标题栏与内容区之外的独立底部区域。
    #[test]
    fn the_footer_is_a_separate_bottom_region() {
        let button_ref = NodeRef::new();
        let slot = button_ref.clone();
        let mut window = Window::new(
            crate::theme::editor_theme(false),
            "标题",
            Vec2::new(420.0, 320.0),
            Vec2::new(320.0, 240.0),
            move |tree, _content, footer| {
                tree.add_child(
                    footer,
                    Button::primary("确定", crate::theme::editor_theme(false)).ref_(&slot),
                );
            },
        );
        window.layout(viewport());
        let rect = window.rect();
        let button = igui_ui::control(&window.tree, button_ref.get().expect("按钮已挂载"))
            .map(|control| control.rect)
            .expect("按钮 rect");
        assert!(rect.contains(button.center()));
        assert!(
            button.top() > rect.top() + TITLE_BAR_HEIGHT,
            "页脚按钮应在标题栏下方"
        );
        assert!(
            button.bottom() > rect.bottom() - 60.0,
            "页脚按钮应贴近底边：button={button:?} rect={rect:?}"
        );
    }

    #[test]
    fn the_window_swallows_input_underneath() {
        let (mut window, _, _) = window_with_button();
        window.layout(viewport());
        let result = window.event(&InputEvent::KeyDown {
            key: Key::Character('x'),
        });
        assert!(result.is_handled());
    }
}
