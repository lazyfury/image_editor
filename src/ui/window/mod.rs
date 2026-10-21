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
//! 结构固定为三段：**标题栏**（贴边、只有标题左边距 + 关闭按钮贴右；底色比窗口
//! 正文浅一档，上沿跟着外框圆角）、**内容区**（`grow`，自带 padding）和**页脚**
//! （按钮右对齐、垂直居中，有 [`FOOTER_MIN_HEIGHT`] 的最小高度）。
//!
//! 外框右下角画三道 45° 斜线（[`GRIP_LINES`]），提示「这里可以拖动缩放」——
//! 缩放命中带仍是贴边的 [`RESIZE_BORDER`](geometry::RESIZE_BORDER) 宽那一圈。
//!
//! [`NewDocumentModal`](crate::ui::NewDocumentModal) 与
//! [`ConfirmDialog`](crate::ui::ConfirmDialog) 是它的使用者。
//!
//! 文件分工：`mod.rs` = 窗口状态 + `layout` / `event` / `paint`；
//! `chrome.rs` = 三段结构的外观（标题栏底色 / 页脚最小高度 / 缩放手柄）；
//! `gesture.rs` = 拖动 / 缩放手势；`geometry.rs` = 纯几何（命中带 / 改矩形 /
//! 钳进视口，可单测）；`tests.rs` = 行为测试。

use std::cell::Cell;
use std::rc::Rc;

use igui_components::{set_text, update_control, Component, Flex};
use igui_core::{
    Color, Edges, EventResult, InputEvent, Key, NodeId, PointerButton, Rect, Size, Vec2,
    ViewportSize,
};
use igui_render::PaintContext;
use igui_scene::SceneTree;
use igui_theme::{SurfaceLevel, Theme};
use igui_ui::{MouseFilter, TextMeasurer};

mod chrome;
mod geometry;
mod gesture;
#[cfg(test)]
mod tests;

use geometry::clamp_to_viewport;
use gesture::Gesture;

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
/// 页脚最小高度：内容再挤也留出这一条，底部操作区不会缩没。
pub const FOOTER_MIN_HEIGHT: f32 = 52.0;
/// 标题栏底色：比窗口底色高一档（`Base` -> `Raised`），把标题区从内容里分出来。
const HEADER_SURFACE: SurfaceLevel = SurfaceLevel::Raised;
/// 右下角「可缩放」提示：三道 45° 斜线、每道比上一道长 [`GRIP_STEP`]。
const GRIP_LINES: usize = 3;
const GRIP_STEP: f32 = 3.5;
/// 斜线离窗口右下角的间距（要小于页脚的水平 padding，别压到按钮）。
const GRIP_INSET: f32 = 4.0;
/// 模态遮罩。
const SCRIM: Color = Color::new(0.0, 0.0, 0.0, 0.35);
/// 窗口底色。用**最底层**而不是 `Raised`：`Button` 的 secondary / ghost 填充
/// 就是 `surface_raised`，窗口也是 `Raised` 的话按钮只剩描边、看着比实心按钮小。
const WINDOW_SURFACE: SurfaceLevel = SurfaceLevel::Base;
/// 外框圆角（标题栏上沿跟着它）。比主题的 `radius::PANEL`（10）小一档：这个窗口
/// 是「应用内的桌面窗口」，太大看着像气泡。
const WINDOW_RADIUS: f32 = igui_theme::radius::MD;

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
        let (frame, content_ref, footer_ref) =
            chrome::frame(theme, title, size, min_size, close.clone());

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

    fn apply_offsets(&mut self) {
        let rect = self.rect.get();
        update_control(&mut self.tree, self.frame, |data| {
            data.anchors = Edges::ZERO;
            data.offsets = Edges::new(rect.left(), rect.top(), rect.right(), rect.bottom());
        });
    }
}
