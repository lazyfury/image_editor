//! 视图导航：抓手 / 缩放工具 + 空格临时抓手。
//!
//! 平移和缩放都直接操作相机（[`crate::canvas::CanvasCamera`]），不碰文档、不进
//! 撤销、不标脏 —— 和滚轮 / 触控板 / 中键是同一套动作，只是入口不同：
//!
//! - 中键拖动、触控板滚动、`Shift+滚轮`：原有手势，行为不变；
//! - 抓手工具（`H`）拖动：和 MMB 共用 [`ViewportNav`] 里的 `pan` 状态；
//! - 按住空格：临时抓手，**不切换**当前工具，松手/松空格恢复；
//! - 缩放工具（`Z`）点击：以指针为锚放大一档，`Option/Alt` 反向缩小。
//!
//! 指针事件的路由顺序在 [`super::input`]：视图导航优先于文档工具，所以蒙版 /
//! 图层组不会拦它，也不会产生"蒙版只能用画笔编辑"之类的提示。

use igui_core::Vec2;

use crate::app::state::ActiveTool;

use super::{EditorView, ZOOM_STEP};

/// 一次平移拖动的来源；相机动作相同，只影响谁来收尾。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PanSource {
    Middle,
    Hand,
    Space,
}

/// 视图导航的瞬时状态（不进撤销、不碰文档）。
#[derive(Debug, Default)]
pub(super) struct ViewportNav {
    pan: Option<(PanSource, Vec2)>,
    space_held: bool,
}

impl ViewportNav {
    pub(super) fn space_held(&self) -> bool {
        self.space_held
    }

    /// 空格按下：进入临时抓手。重复按键（auto-repeat）保持原状。
    pub(super) fn hold_space(&mut self) {
        self.space_held = true;
    }

    /// 空格松开：退出临时抓手；如果正在用空格拖动，这一拖也就地结束。
    pub(super) fn release_space(&mut self) {
        self.space_held = false;
        if matches!(self.pan, Some((PanSource::Space, _))) {
            self.pan = None;
        }
    }

    fn begin_pan(&mut self, source: PanSource, at: Vec2) {
        if self.pan.is_none() {
            self.pan = Some((source, at));
        }
    }

    /// 跟手平移；返回是否消费了这次移动。
    fn drag_pan(&mut self, at: Vec2) -> Option<Vec2> {
        let (_, last) = self.pan.as_mut()?;
        let delta = at - *last;
        *last = at;
        Some(delta)
    }

    /// 结束指定来源的拖动；来源不匹配（例如中键还按着）就返回 `false`。
    fn end_pan(&mut self, source: PanSource) -> bool {
        if matches!(self.pan, Some((current, _)) if current == source) {
            self.pan = None;
            true
        } else {
            false
        }
    }

    fn panning(&self) -> bool {
        self.pan.is_some()
    }
}

impl EditorView {
    /// 左键按下时的视图导航分支：空格 / 抓手开始平移，缩放工具点一下缩放。
    ///
    /// 返回 `true` 表示这次按下归导航处理，不能再走 `begin_tool`。
    pub(super) fn press_navigation(&mut self, tool: ActiveTool, screen: Vec2) -> bool {
        if self.nav.space_held() {
            self.nav.begin_pan(PanSource::Space, screen);
            return true;
        }
        match tool {
            ActiveTool::Hand => {
                self.nav.begin_pan(PanSource::Hand, screen);
                true
            }
            ActiveTool::Zoom => {
                let factor = if self.modifiers.alt {
                    1.0 / ZOOM_STEP
                } else {
                    ZOOM_STEP
                };
                self.state.borrow_mut().canvas.zoom_at(screen, factor);
                true
            }
            _ => false,
        }
    }

    /// 指针移动：正在平移就跟手（不让文档工具收到）。
    pub(super) fn drag_navigation(&mut self, screen: Vec2) -> bool {
        let Some(delta) = self.nav.drag_pan(screen) else {
            return false;
        };
        self.pan_canvas(delta);
        true
    }

    /// 左键抬起：结束抓手 / 空格平移（中键的抬起走另一个分支）。
    pub(super) fn release_navigation(&mut self) -> bool {
        self.nav.end_pan(PanSource::Hand) || self.nav.end_pan(PanSource::Space)
    }

    /// 中键按下 / 抬起沿用同一套 `pan` 状态。
    pub(super) fn press_middle_pan(&mut self, screen: Vec2) {
        self.nav.begin_pan(PanSource::Middle, screen);
    }

    pub(super) fn release_middle_pan(&mut self) -> bool {
        self.nav.end_pan(PanSource::Middle)
    }

    pub fn space_held(&self) -> bool {
        self.nav.space_held()
    }

    pub(super) fn hold_space(&mut self) {
        self.nav.hold_space();
    }

    pub(super) fn release_space(&mut self) {
        self.nav.release_space();
    }

    /// 是否正在平移（宿主选光标 / 测试用）。
    pub fn panning(&self) -> bool {
        self.nav.panning()
    }
}
