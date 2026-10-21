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

use igui_core::{EventResult, InputEvent, NodeId, Vec2, ViewportSize};
use igui_render::PaintContext;
use igui_theme::Theme;
use igui_ui::TextMeasurer;

#[cfg(test)]
use igui_core::Rect;
#[cfg(test)]
use igui_scene::SceneTree;

use super::window::Window;
use crate::document::Color;

mod form;
mod widgets;

#[cfg(test)]
mod tests;

use form::{background_name, build_content, FormWiring};

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
/// 4096：够放下 4K（3840）和 A4 300dpi（2480×3508），再大 CPU 合成就顶不住了。
const MAX_SIDE: u32 = 4096;
const SIDE_STEP: u32 = 1;

/// 预设尺寸：先是像素画的小方块，再是常见的高分辨率画布（显示 / 打印 / 贴图）。
///
/// 大尺寸走 CPU 合成：笔触期间每帧要遍历整张文档，4K 会比 512² 慢两个数量级，
/// 所以选项栏下面有一句提示。
const PRESETS: [(u32, u32); 12] = [
    // 像素画
    (16, 16),
    (32, 32),
    (64, 64),
    (128, 128),
    (256, 256),
    (512, 512),
    // 高分辨率
    (1920, 1080),
    (1080, 1920),
    (2560, 1440),
    (3840, 2160),
    (2048, 2048),
    (2480, 3508),
];

/// 背景色预设（名字，颜色）。第一个是透明。
const BACKGROUNDS: [(&str, Color); 4] = [
    ("透明", Color::TRANSPARENT),
    ("白色", Color::WHITE),
    ("黑色", Color::BLACK),
    ("红色", Color::RED),
];

/// 初始窗口尺寸 / 拖动缩放下限（逻辑像素）。
const WINDOW_SIZE: Vec2 = Vec2::new(460.0, 470.0);
const WINDOW_MIN: Vec2 = Vec2::new(380.0, 360.0);

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
                    tree,
                    content,
                    footer,
                    theme,
                    &FormWiring {
                        choices: &choices,
                        dirty: &dirty,
                        result: &result,
                        nodes: &nodes,
                    },
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
