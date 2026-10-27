//! 撤销 / 重做：`Command` 模式 + 两条栈（[`History`]）。
//!
//! 约定：`Command::execute` 把命令的效果写回文档（首次应用与 redo 都走它），
//! `Command::undo` 撤回效果。命令记录的是**差异区域的前后像素**，不是整层
//! 快照 —— 一笔的撤销内存跟笔画覆盖的大小成正比。
//!
//! `History` 是**编辑会话**状态（不随文档保存 / 克隆），所以放在 `AppState`
//! 而不是 `Document` 里；命令本身属于文档领域，所以这个模块在 `document/`。
//!
//! 命令按领域分文件：`paint.rs`（像素笔触）、`layers.rs`（图层增删 / 顺序 /
//! 元数据 / 裁剪）；这里只有接口与栈本身。

use std::fmt;

use super::color::Color;
use super::document::Document;
use super::id::LayerId;
use super::region::PixelRegion;

mod layers;
mod paint;
mod recorder;

#[cfg(test)]
mod tests;

pub use layers::{
    AddLayerCommand, CropLayerCommand, GroupCommand, LayerMetaCommand, LayerStackMeta, MaskCommand,
    MoveLayerCommand, MoveTarget, RemoveLayerCommand, SetLayerPositionCommand, UngroupCommand,
};
pub use paint::PaintCommand;
pub use recorder::{RecordedStep, Recorder, StepPatch, DEFAULT_STEP_LIMIT};

/// 一条可撤销的编辑。
pub trait Command: fmt::Debug {
    /// 首次应用或 redo：把效果写回 `document`。
    fn execute(&mut self, document: &mut Document);
    /// 撤销效果（恢复到 `execute` 之前）。
    fn undo(&mut self, document: &mut Document);
    /// 状态栏 / 日志里的短名字。
    fn label(&self) -> &'static str;
    /// 图层缓冲区重新对齐（内容索引整体平移 `(dx, dy)`，非负）时，同步平移
    /// 这条命令记录的区域。默认 no-op；只有记录像素坐标的命令需要实现。
    fn translate_for(&mut self, _layer: LayerId, _dx: i32, _dy: i32) {}

    /// 这条命令改动的像素区域（测试 / 自检核对「撤销区域贴着笔迹」用）。
    fn region(&self) -> Option<PixelRegion> {
        None
    }

    /// 过程回放（延时动画）要的数据：这一步把哪块像素改成了什么。
    ///
    /// 默认 `None`（图层结构类命令暂时只留 label，回放时当作一次「切」）。
    fn replay(&self) -> Option<ReplayPatch<'_>> {
        None
    }
}

/// 一条命令的像素补丁（回放用）：区域内的 `after` 像素，行优先。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplayPatch<'a> {
    pub layer: LayerId,
    pub region: PixelRegion,
    pub pixels: &'a [Color],
}

/// 撤销 / 重做栈。超过上限时丢弃最旧的命令，内存有界。
pub struct History {
    undo: Vec<Box<dyn Command>>,
    redo: Vec<Box<dyn Command>>,
    limit: usize,
    /// 全量过程记录（延时动画用）；默认关，由偏好打开。
    recorder: Option<Recorder>,
}

impl History {
    /// 默认保留的撤销步数。
    pub const DEFAULT_LIMIT: usize = 32;

    pub fn new() -> Self {
        Self::with_limit(Self::DEFAULT_LIMIT)
    }

    pub fn with_limit(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit,
            recorder: None,
        }
    }

    /// 执行一条**新**命令：应用它、压入 undo 栈、清空 redo 栈。
    ///
    /// 交互式编辑（画笔）在落笔时已经改过像素，这里会再把同样的像素写一遍；
    /// 命令写的是记录下来的字节，所以是幂等的。
    pub fn execute(&mut self, mut command: Box<dyn Command>, document: &mut Document) {
        command.execute(document);
        if let Some(recorder) = &mut self.recorder {
            recorder.record(command.as_ref());
        }
        self.undo.push(command);
        self.redo.clear();
        self.trim();
    }

    /// 撤销一步，返回命令名字；没有可撤销的返回 `None`。
    pub fn undo(&mut self, document: &mut Document) -> Option<&'static str> {
        let mut command = self.undo.pop()?;
        command.undo(document);
        let label = command.label();
        self.redo.push(command);
        Some(label)
    }

    /// 重做一步，返回命令名字；没有可重做的返回 `None`。
    pub fn redo(&mut self, document: &mut Document) -> Option<&'static str> {
        let mut command = self.redo.pop()?;
        command.execute(document);
        let label = command.label();
        self.undo.push(command);
        self.trim();
        Some(label)
    }

    /// 最近一条可撤销命令记录的像素区域（测试 / 自检验证撤销区域用）。
    pub fn undo_region(&self) -> Option<PixelRegion> {
        self.undo.last().and_then(|command| command.region())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// 打开 / 关闭全量过程记录（延时动画用）。关掉会丢弃已记录的步骤。
    pub fn record_timeline(&mut self, enabled: bool) {
        match (enabled, self.recorder.is_some()) {
            (true, false) => self.recorder = Some(Recorder::new()),
            (false, true) => self.recorder = None,
            _ => {}
        }
    }

    /// 正在记录的过程流水账（只读）。
    pub fn recorder(&self) -> Option<&Recorder> {
        self.recorder.as_ref()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    /// 撤销栈里的命令名，从最旧到最新（历史面板显示用）。
    pub fn undo_labels(&self) -> Vec<&'static str> {
        self.undo.iter().map(|command| command.label()).collect()
    }

    /// 重做栈里的命令名，**下一个要重做的在最前**。
    pub fn redo_labels(&self) -> Vec<&'static str> {
        self.redo
            .iter()
            .rev()
            .map(|command| command.label())
            .collect()
    }

    /// 图层缓冲区重新对齐后，平移该图层上所有命令记录的区域，使它们在新的
    /// buffer 坐标系里仍指向同一块内容。`dx` / `dy` 非负。
    pub fn translate_layer(&mut self, layer: LayerId, dx: i32, dy: i32) {
        if dx == 0 && dy == 0 {
            return;
        }
        for command in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            command.translate_for(layer, dx, dy);
        }
    }

    fn trim(&mut self) {
        let excess = self.undo.len().saturating_sub(self.limit);
        if excess > 0 {
            self.undo.drain(0..excess);
        }
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

/// 克隆出来的历史是空的：撤销栈是编辑会话状态，不随文档复制。
/// 克隆只带撤销栈：录制是「当前会话」的行为，不跟着文档状态走。
impl Clone for History {
    fn clone(&self) -> Self {
        Self::with_limit(self.limit)
    }
}

impl PartialEq for History {
    fn eq(&self, other: &Self) -> bool {
        self.limit == other.limit
            && self.undo.len() == other.undo.len()
            && self.redo.len() == other.redo.len()
    }
}

impl fmt::Debug for History {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("History")
            .field("undo", &self.undo.len())
            .field("redo", &self.redo.len())
            .field("limit", &self.limit)
            .finish()
    }
}
