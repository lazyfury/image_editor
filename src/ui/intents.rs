//! 回调留下的意图队列。
//!
//! 控件回调（`Button::on_click`、菜单项、工具栏长按……）拿不到 `&mut EditorView`，
//! 所以只往这个共享队列里塞一个 [`Intent`]；[`EditorView::update`] 每帧一并取走、
//! 按到达顺序处理，宿主认识的动作（[`Intent::File`]）留在队列里等宿主取。
//!
//! 用一个队列而不是十几条 `Rc<Cell<Option<T>>>`：动作的先后顺序天然保留，新增
//! 一个动作只动这一个 `enum`，而「谁在哪一段被处理」在 `update` 里一眼能看出来。
//!
//! [`EditorView::update`]: crate::ui::EditorView::update

use std::cell::RefCell;
use std::rc::Rc;

use igui_core::{NodeId, Vec2};

use crate::app::state::ActiveTool;
use crate::document::LayerId;
use crate::ui::layer_actions::LayerAction;
use crate::ui::menu::MenuAction;
use crate::ui::options_bar::{BrushAdjust, BucketAdjust, GradientAdjust, PenAction};
use crate::ui::FileRequest;

/// 一次用户动作。
///
/// 变体的分组就是处理它的地方：工具参数在 `update` 开头、菜单 / 重命名在各自的
/// 一段、[`Intent::Tool`] 在工具栏轮询之后、[`Intent::File`] 由宿主取走。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Intent {
    /// 工具按钮 / 浮层 / 快捷键选中了一个工具。
    Tool(ActiveTool),
    /// 点了一个菜单标题：打开它的下拉。
    OpenMenu(usize),
    /// 点了一个菜单项。
    MenuAction(MenuAction),
    /// 点了选项栏的「预设」：打开预设下拉，锚在这个节点上。
    OpenBrushPresets(NodeId),
    /// 选了第几个笔刷预设。
    BrushPreset(usize),
    /// 选项栏的画笔 `−` / `+`。
    Brush(BrushAdjust),
    /// 选项栏的油漆桶 `−` / `+`。
    Bucket(BucketAdjust),
    /// 选项栏的渐变类型切换。
    Gradient(GradientAdjust),
    /// 点了选项栏的「编辑…」：打开渐变编辑器。
    OpenGradientEditor,
    /// 钢笔选项栏的动作（选区 / 填充 / 描边 / 清除）。
    Pen(PenAction),
    /// 点了图层面板的「重命名」，锚在这个按钮上。
    RenameLayer(NodeId),
    /// 右键点了某个图层行：在指针处打开上下文菜单。
    LayerMenu { target: LayerId, position: Vec2 },
    /// 上下文菜单 / 菜单栏 / 面板按钮里的图层动作。
    LayerAction {
        target: LayerId,
        action: LayerAction,
    },
    /// 预览面板的采样方式（线性 / 最近邻）。
    PreviewFilter(crate::canvas::PreviewFilter),
    /// 打开一个模拟窗口（属性 / 关于 / 快捷键；宿主执行）。
    Dialog(crate::ui::DialogKind),
    /// 打开 / 关闭「记录整段编辑过程」（属性窗口的开关）。
    RecordTimeline(bool),
    /// 菜单 / 快捷键的文件动作（宿主弹原生对话框并执行）。
    File(FileRequest),
}

/// 意图队列（`Rc` 共享：每个回调拿一份克隆）。
#[derive(Clone, Default)]
pub struct Intents(Rc<RefCell<Vec<Intent>>>);

impl Intents {
    pub fn new() -> Self {
        Self::default()
    }

    /// 塞一个意图（回调里用）。
    pub fn push(&self, intent: Intent) {
        self.0.borrow_mut().push(intent);
    }

    /// 取走视图自己能处理的意图；[`Intent::File`] / [`Intent::Dialog`] 留给宿主。
    pub(crate) fn take(&self) -> Vec<Intent> {
        self.take_where(|intent| !matches!(intent, Intent::File(_) | Intent::Dialog(_)))
    }

    /// 取走第一个「打开模拟窗口」的意图（宿主用）。
    pub fn take_dialog(&self) -> Option<crate::ui::DialogKind> {
        let mut queue = self.0.borrow_mut();
        let index = queue
            .iter()
            .position(|intent| matches!(intent, Intent::Dialog(_)))?;
        match queue.remove(index) {
            Intent::Dialog(kind) => Some(kind),
            _ => None,
        }
    }

    /// 取走满足 `keep` 的意图，其余按原顺序留在队列里。
    pub(crate) fn take_where(&self, keep: impl Fn(&Intent) -> bool) -> Vec<Intent> {
        let mut queue = self.0.borrow_mut();
        let (taken, rest): (Vec<_>, Vec<_>) = queue.drain(..).partition(|intent| keep(intent));
        *queue = rest;
        taken
    }

    /// 取走第一个文件动作（宿主用），其余意图留在队列里。
    pub fn take_file(&self) -> Option<FileRequest> {
        let mut queue = self.0.borrow_mut();
        let index = queue
            .iter()
            .position(|intent| matches!(intent, Intent::File(_)))?;
        match queue.remove(index) {
            Intent::File(request) => Some(request),
            // `position` 只挑 `File`，走不到这里。
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intents_keep_the_order_they_were_pushed() {
        let intents = Intents::new();
        intents.push(Intent::Tool(ActiveTool::Eraser));
        intents.push(Intent::Brush(BrushAdjust::SizeUp));
        intents.push(Intent::File(FileRequest::Save));
        assert_eq!(
            intents.take(),
            vec![
                Intent::Tool(ActiveTool::Eraser),
                Intent::Brush(BrushAdjust::SizeUp)
            ]
        );
    }

    #[test]
    fn the_host_takes_file_intents_without_disturbing_the_rest() {
        let intents = Intents::new();
        intents.push(Intent::File(FileRequest::Save));
        intents.push(Intent::Tool(ActiveTool::Pen));
        intents.push(Intent::File(FileRequest::ExportPng));

        assert_eq!(intents.take_file(), Some(FileRequest::Save));
        assert_eq!(intents.take_file(), Some(FileRequest::ExportPng));
        assert_eq!(intents.take_file(), None);
        assert_eq!(intents.take(), vec![Intent::Tool(ActiveTool::Pen)]);
    }

    #[test]
    fn take_where_only_removes_the_matching_intents() {
        let intents = Intents::new();
        intents.push(Intent::MenuAction(MenuAction::Undo));
        intents.push(Intent::Tool(ActiveTool::Move));

        let taken = intents.take_where(|intent| matches!(intent, Intent::MenuAction(_)));
        assert_eq!(taken, vec![Intent::MenuAction(MenuAction::Undo)]);
        assert_eq!(intents.take(), vec![Intent::Tool(ActiveTool::Move)]);
    }
}
