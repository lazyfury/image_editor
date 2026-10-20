//! 钢笔：多击建路径（点 = 尖角，拖 = 平滑锚点），回车 / Esc 收尾。
//!
//! 工具本身只攒一条 [`Path`] 并把它变成 [`Selection`]；预览绘制在
//! `ui::paint`，指针 / 键位接线在 `ui::tools` / `ui::input`。像素操作
//! （填充 / 描边）由视图执行，所以一笔路径**本身不进撤销栈** —— 和选区一样。
//!
//! 交互（PS 的兼容子集）：
//! - 单击 → 尖角锚点；按下拖动 → 对称控制柄的平滑锚点。
//! - 点回第一个锚点 → 闭合收尾（不新增锚点）。
//! - `Enter` 收尾（开放路径）、`Esc` 取消、`Backspace` 删最后一个锚点。

use igui_core::Vec2;

use super::tool::{PointerEvent, Tool, ToolContext};
use crate::canvas::{flatten_segment, Anchor, Path, Selection};

/// 路径 → 选区用的展平容差（文档像素）。
///
/// 0.25px 在 100% 缩放下看不出来；再小只是多出采样点。
pub const RASTER_TOLERANCE: f32 = 0.25;

/// 命中锚点 / 「点回起点闭合」的默认半径（文档像素）。视图按缩放同步。
pub const DEFAULT_RADIUS: f32 = 8.0;

/// 钢笔工具。
#[derive(Debug, Clone, PartialEq)]
pub struct PenTool {
    path: Path,
    /// 命中锚点 / 闭合判定的半径（文档像素，视图按 `8 / zoom` 同步）。
    pub radius: f32,
    /// 正在拖控制柄的锚点下标（按下后还没松手）。
    dragging: Option<usize>,
    /// 这一条路径已经收尾（Enter / 闭合）：下一次点击开始新路径。
    finished: bool,
    /// 悬停位置（橡皮筋预览 + 闭合提示用）。
    hover: Option<Vec2>,
}

impl PenTool {
    pub fn new() -> Self {
        Self {
            path: Path::new(),
            radius: DEFAULT_RADIUS,
            dragging: None,
            finished: false,
            hover: None,
        }
    }

    /// 当前的路径（预览 / 选项栏统计用）。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 是否正在拖控制柄（视图据此把 `PointerMove` 转发过来）。
    pub fn is_dragging(&self) -> bool {
        self.dragging.is_some()
    }

    /// 记录悬停位置（不按键的移动，画橡皮筋）。
    pub fn set_hover(&mut self, position: Option<Vec2>) {
        self.hover = position;
    }

    /// 橡皮筋：最后一个锚点到悬停点的展平折线（含两端）。
    ///
    /// 只在一笔画到一半、有悬停点时才有；闭合提示由视图看
    /// [`Self::can_close`] 自己决定怎么画。
    pub fn rubber_band(&self, tolerance: f32) -> Option<Vec<Vec2>> {
        if self.path.is_closed() {
            return None;
        }
        let hover = self.hover?;
        let last = self.path.anchors().last()?;
        let tip = Anchor::corner(hover);
        let points = flatten_segment(last, &tip, tolerance);
        (points.len() >= 2).then_some(points)
    }

    /// 光标是不是压在第一个锚点上（画闭合提示 / 决定下一次点击闭合）。
    pub fn can_close(&self) -> bool {
        let Some(hover) = self.hover else {
            return false;
        };
        self.path.len() >= 2 && self.path.hit_anchor(hover, self.radius) == Some(0)
    }

    /// 收尾：定格当前路径（闭合的保持闭合），把它留给选项栏的动作。
    ///
    /// 收尾之后**下一次点画布就开始新路径**（老路径留着做选区 / 填充 / 描边，
    /// 直到你点下去为止）。
    pub fn finish(&mut self) -> bool {
        self.dragging = None;
        let had = !self.path.is_empty();
        self.finished = had;
        had
    }

    /// 取消：丢掉整条路径。
    pub fn cancel(&mut self) -> bool {
        let had = !self.path.is_empty();
        self.path.clear();
        self.dragging = None;
        self.finished = false;
        had
    }

    /// 删掉最后一个锚点（`Backspace`）。
    pub fn undo_anchor(&mut self) -> bool {
        self.dragging = None;
        // 删点等于「继续编辑」，可以接着往上加。
        self.finished = false;
        self.path.undo_anchor()
    }

    /// 展平成闭合多边形，再做选区；退化（点太少 / 围不出像素）返回 `None`。
    ///
    /// 开放路径也按「首尾相连」闭合（PS 建立选区时同样如此）。
    pub fn polygon(&self, document: (u32, u32)) -> Option<Selection> {
        if self.path.len() < 2 {
            return None;
        }
        let points = self.path.flatten_closed(RASTER_TOLERANCE);
        let selection = Selection::polygon(&points, document);
        (!selection.is_empty()).then_some(selection)
    }

    /// 沿路径的展平折线（描边用；闭合路径带绕回那段）。
    pub fn flattened(&self, tolerance: f32) -> Vec<Vec2> {
        self.path.flatten(tolerance)
    }
}

impl Default for PenTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for PenTool {
    fn name(&self) -> &'static str {
        "钢笔"
    }

    fn on_pointer_down(&mut self, _ctx: &mut ToolContext, event: PointerEvent) {
        self.hover = Some(event.position);
        // 点回第一个锚点 -> 闭合收尾，不新增锚点（收尾过的路径也可以这样闭合）。
        if self.path.len() >= 2 && self.path.hit_anchor(event.position, self.radius) == Some(0) {
            self.path.close();
            self.dragging = None;
            self.finished = true;
            return;
        }
        // 上一条路径已经收尾：这一下点空白处就是开新路径。
        if self.finished {
            self.path.clear();
            self.finished = false;
        }
        self.path.push(Anchor::corner(event.position));
        self.dragging = Some(self.path.len() - 1);
    }

    fn on_pointer_move(&mut self, _ctx: &mut ToolContext, event: PointerEvent) {
        self.hover = Some(event.position);
        let Some(index) = self.dragging else {
            return;
        };
        let Some(anchor) = self.path.anchor_mut(index) else {
            return;
        };
        // 拖出来的方向就是出去的控制柄，进来那侧对称。
        *anchor = Anchor::smooth(anchor.point, event.position - anchor.point);
    }

    fn on_pointer_up(&mut self, _ctx: &mut ToolContext, event: PointerEvent) {
        self.hover = Some(event.position);
        self.dragging = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, History};

    fn context<'a>(document: &'a mut Document, history: &'a mut History) -> ToolContext<'a> {
        ToolContext { document, history }
    }

    fn left(position: Vec2) -> PointerEvent {
        PointerEvent {
            position,
            button: igui_core::PointerButton::Left,
        }
    }

    fn at(x: f32, y: f32) -> Vec2 {
        Vec2::new(x, y)
    }

    /// 在钢笔上点一下（按下 + 松手，不拖动）。
    fn click(pen: &mut PenTool, document: &mut Document, history: &mut History, position: Vec2) {
        let mut ctx = context(document, history);
        pen.on_pointer_down(&mut ctx, left(position));
        pen.on_pointer_up(&mut ctx, left(position));
    }

    #[test]
    fn a_click_adds_a_corner_anchor() {
        let (mut document, mut history) = (Document::new("d", 32, 32), History::new());
        let mut pen = PenTool::new();
        click(&mut pen, &mut document, &mut history, at(4.0, 4.0));
        click(&mut pen, &mut document, &mut history, at(20.0, 4.0));
        assert_eq!(pen.path().len(), 2);
        assert!(pen
            .path()
            .anchors()
            .iter()
            .all(|anchor| anchor.handle_in == Vec2::ZERO && anchor.handle_out == Vec2::ZERO));
        // 直线连接：展平只有两个点。
        assert_eq!(pen.flattened(RASTER_TOLERANCE).len(), 2);
    }

    #[test]
    fn dragging_out_of_a_new_anchor_sets_symmetric_handles() {
        let (mut document, mut history) = (Document::new("d", 32, 32), History::new());
        let mut pen = PenTool::new();
        let mut ctx = context(&mut document, &mut history);
        pen.on_pointer_down(&mut ctx, left(at(10.0, 10.0)));
        pen.on_pointer_move(&mut ctx, left(at(16.0, 10.0)));
        assert!(pen.is_dragging());
        pen.on_pointer_up(&mut ctx, left(at(16.0, 10.0)));
        assert!(!pen.is_dragging());

        let anchor = pen.path().anchors()[0];
        assert_eq!(anchor.handle_out, at(6.0, 0.0));
        assert_eq!(anchor.handle_in, at(-6.0, 0.0), "进来那侧对称");
    }

    #[test]
    fn clicking_the_first_anchor_closes_the_path_instead_of_adding_one() {
        let (mut document, mut history) = (Document::new("d", 32, 32), History::new());
        let mut pen = PenTool::new();
        click(&mut pen, &mut document, &mut history, at(8.0, 8.0));
        click(&mut pen, &mut document, &mut history, at(28.0, 8.0));
        click(&mut pen, &mut document, &mut history, at(28.0, 28.0));
        // 回到起点附近（半径内）。
        click(&mut pen, &mut document, &mut history, at(9.0, 9.0));
        assert_eq!(pen.path().len(), 3, "闭合不该再落一个锚点");
        assert!(pen.path().is_closed());
        assert!(pen.path().is_closed());
    }

    #[test]
    fn a_two_point_path_has_no_polygon() {
        // 两个点围不出面积：不给空选区（视图据此不覆盖已有选区）。
        let (mut document, mut history) = (Document::new("d", 32, 32), History::new());
        let mut pen = PenTool::new();
        click(&mut pen, &mut document, &mut history, at(4.0, 4.0));
        click(&mut pen, &mut document, &mut history, at(20.0, 4.0));
        assert_eq!(pen.polygon((32, 32)), None);
    }

    #[test]
    fn a_triangle_becomes_a_selection_with_area() {
        let (mut document, mut history) = (Document::new("d", 32, 32), History::new());
        let mut pen = PenTool::new();
        click(&mut pen, &mut document, &mut history, at(4.0, 4.0));
        click(&mut pen, &mut document, &mut history, at(28.0, 4.0));
        click(&mut pen, &mut document, &mut history, at(4.0, 28.0));
        let selection = pen.polygon((32, 32)).expect("三角形应该有面积");
        // 直角边 24 的三角形：≈ 24 * 24 / 2 = 288 像素（半开半闭的边界差几像素）。
        let area = selection.area();
        assert!((270..=300).contains(&area), "面积 {area}");
    }

    #[test]
    fn the_rubber_band_runs_from_the_last_anchor_to_the_hover() {
        let (mut document, mut history) = (Document::new("d", 32, 32), History::new());
        let mut pen = PenTool::new();
        click(&mut pen, &mut document, &mut history, at(4.0, 4.0));
        let mut ctx = context(&mut document, &mut history);
        pen.on_pointer_move(&mut ctx, left(at(20.0, 4.0)));
        let band = pen.rubber_band(RASTER_TOLERANCE).expect("有悬停就有橡皮筋");
        assert_eq!(band.first().copied(), Some(at(4.0, 4.0)));
        assert_eq!(band.last().copied(), Some(at(20.0, 4.0)));
    }

    #[test]
    fn clicking_near_the_first_anchor_is_announced_as_closable() {
        let (mut document, mut history) = (Document::new("d", 32, 32), History::new());
        let mut pen = PenTool::new();
        click(&mut pen, &mut document, &mut history, at(8.0, 8.0));
        click(&mut pen, &mut document, &mut history, at(28.0, 8.0));
        pen.set_hover(Some(at(10.0, 10.0)));
        assert!(pen.can_close(), "半径内应提示可闭合");
        pen.set_hover(Some(at(20.0, 20.0)));
        assert!(!pen.can_close());
    }

    #[test]
    fn a_finished_path_starts_a_new_one_on_the_next_click() {
        let (mut document, mut history) = (Document::new("d", 32, 32), History::new());
        let mut pen = PenTool::new();
        click(&mut pen, &mut document, &mut history, at(4.0, 4.0));
        click(&mut pen, &mut document, &mut history, at(20.0, 4.0));
        assert!(pen.finish());
        // 收尾后的第一次点击：丢掉老路径，从 1 个锚点重新开始。
        click(&mut pen, &mut document, &mut history, at(4.0, 20.0));
        assert_eq!(pen.path().len(), 1);
        assert!(!pen.path().is_closed());
    }

    #[test]
    fn closing_the_path_also_finishes_it() {
        let (mut document, mut history) = (Document::new("d", 32, 32), History::new());
        let mut pen = PenTool::new();
        click(&mut pen, &mut document, &mut history, at(4.0, 4.0));
        click(&mut pen, &mut document, &mut history, at(20.0, 4.0));
        click(&mut pen, &mut document, &mut history, at(4.0, 5.0));
        assert!(pen.path().is_closed());
        // 再点一下：闭合路径已经收尾，应开新路径。
        click(&mut pen, &mut document, &mut history, at(24.0, 24.0));
        assert_eq!(pen.path().len(), 1);
        assert!(!pen.path().is_closed());
    }

    #[test]
    fn cancel_and_backspace_are_different_edits() {
        let (mut document, mut history) = (Document::new("d", 32, 32), History::new());
        let mut pen = PenTool::new();
        click(&mut pen, &mut document, &mut history, at(4.0, 4.0));
        click(&mut pen, &mut document, &mut history, at(20.0, 4.0));
        assert!(pen.undo_anchor());
        assert_eq!(pen.path().len(), 1, "Backspace 只删最后一个点");
        assert!(pen.cancel());
        assert!(pen.path().is_empty(), "Esc 丢掉整条路径");
        assert!(!pen.cancel(), "空路径再取消是 no-op");
    }
}
