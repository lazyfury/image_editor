//! 全量过程记录：把每一步编辑（**包括之后被撤销、或被撤销上限挤掉的**）按时间
//! 顺序留下来，给「延时动画 / 过程回放」用。
//!
//! 和撤销栈的区别：撤销栈是为了「退回去」，只保留最近 [`History::DEFAULT_LIMIT`]
//! 步；这里是**流水账**，只进不出（到上限丢最旧的），每步自带时间戳与像素补丁。
//!
//! 默认关闭（占内存：一个补丁 = 区域面积 × 4 字节）。开关在偏好里
//! （`EditorPreferences::record_timeline`），由宿主在开/关工作区时套到
//! [`History`](super::History) 上。
//!
//! 落盘格式见 [`crate::io::timeline`]。

use std::time::{Duration, Instant};

use super::super::color::Color;
use super::super::id::LayerId;
use super::super::region::PixelRegion;
use super::Command;

/// 一步里的像素改动：哪一层、哪块区域、变成了什么（`after`）。
#[derive(Debug, Clone, PartialEq)]
pub struct StepPatch {
    pub layer: LayerId,
    pub region: PixelRegion,
    /// 区域内的像素，行优先，长度 = `region.area()`。
    pub pixels: Vec<Color>,
}

/// 记录下来的一步。
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedStep {
    /// 相对录制开始的时间（延时动画按它排节奏）。
    pub at: Duration,
    /// 命令名字（时间线 UI / 调试用）。
    pub label: String,
    /// 像素改动；图层结构类命令暂时没有补丁（回放时当作一次「切」）。
    pub patch: Option<StepPatch>,
}

/// 流水账录制的默认步数上限（到上限丢最旧的）。
pub const DEFAULT_STEP_LIMIT: usize = 4096;

/// 全量过程记录器。
#[derive(Debug)]
pub struct Recorder {
    started: Instant,
    steps: Vec<RecordedStep>,
    limit: usize,
}

impl Recorder {
    pub fn new() -> Self {
        Self::with_limit(DEFAULT_STEP_LIMIT)
    }

    pub fn with_limit(limit: usize) -> Self {
        Self {
            started: Instant::now(),
            steps: Vec::new(),
            limit,
        }
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn steps(&self) -> &[RecordedStep] {
        &self.steps
    }

    /// 记一步（由 [`History::execute`](super::History::execute) 调用）。
    pub(super) fn record(&mut self, command: &dyn Command) {
        let patch = command.replay().map(|replay| StepPatch {
            layer: replay.layer,
            region: replay.region,
            pixels: replay.pixels.to_vec(),
        });
        // 时间戳记到毫秒：回放用不上更细，也让内存里的值与 `.igtl` 里的值一致
        // （否则「写盘再读回来」永远差一点零头）。
        let at = Duration::from_millis(self.started.elapsed().as_millis() as u64);
        self.steps.push(RecordedStep {
            at,
            label: command.label().to_string(),
            patch,
        });
        if self.limit > 0 && self.steps.len() > self.limit {
            self.steps.remove(0);
        }
    }
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{
        Color, Document, History, Layer, LayerMetaCommand, PaintCommand, PixelBuffer,
    };

    fn document() -> Document {
        Document::new("d", 8, 8)
    }

    /// 在 (2,2) 点一个像素并提交一条 `PaintCommand`。
    fn paint(history: &mut History, document: &mut Document, color: Color) {
        let id = document.active_layer.expect("有背景层");
        let before = document.layer(id).expect("层在").pixels.clone();
        document
            .layer_mut(id)
            .expect("层在")
            .pixels
            .set_pixel(2, 2, color);
        let after = document.layer(id).expect("层在").pixels.clone();
        let command = PaintCommand::capture(id, &before, &after, "画笔").expect("有变化");
        history.execute(Box::new(command), document);
    }

    #[test]
    fn steps_keep_execute_order_with_their_patches() {
        let mut document = document();
        let mut history = History::new();
        history.record_timeline(true);
        paint(&mut history, &mut document, Color::RED);
        paint(&mut history, &mut document, Color::WHITE);

        let steps = history.recorder().expect("在录制").steps();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].label, "画笔");
        let first = steps[0].patch.as_ref().expect("笔触应有补丁");
        assert_eq!(first.region.area(), 1, "只改了一个像素");
        assert_eq!(first.pixels, vec![Color::RED], "记的是改后的像素");
        let second = steps[1].patch.as_ref().expect("笔触应有补丁");
        assert_eq!(second.pixels, vec![Color::WHITE]);
        assert!(steps[1].at >= steps[0].at, "时间戳不能倒退");
    }

    /// 撤销不会把已经发生过的事从流水账里删掉 —— 这正是「全量」的意义。
    #[test]
    fn undoing_a_step_keeps_it_in_the_log() {
        let mut document = document();
        let mut history = History::new();
        history.record_timeline(true);
        paint(&mut history, &mut document, Color::RED);
        paint(&mut history, &mut document, Color::WHITE);
        history.undo(&mut document);
        history.undo(&mut document);

        let steps = history.recorder().expect("在录制").steps();
        assert_eq!(steps.len(), 2, "两次笔触都还在：{steps:?}");
        assert_eq!((history.undo_len(), history.redo_len()), (0, 2));
    }

    /// 撤销上限挤掉的步骤仍然留在流水账里。
    #[test]
    fn steps_dropped_by_the_undo_limit_stay_in_the_log() {
        let mut document = document();
        let mut history = History::with_limit(1);
        history.record_timeline(true);
        paint(&mut history, &mut document, Color::RED);
        paint(&mut history, &mut document, Color::WHITE);

        assert_eq!(history.undo_len(), 1, "撤销栈只留 1 步");
        assert_eq!(history.recorder().expect("在录制").len(), 2);
    }

    /// 图层结构类命令没有像素补丁，但也要留一条带 label 的步。
    #[test]
    fn structural_commands_are_logged_without_a_patch() {
        let mut document = document();
        let mut history = History::new();
        history.record_timeline(true);
        let before = LayerMetaCommand::capture(&document);
        let layer = Layer::new("上层", PixelBuffer::new(8, 8));
        let index = document.layers.len();
        let active = document.active_layer;
        document.layers.push(layer);
        document.touch();
        let after = LayerMetaCommand::capture(&document);
        history.execute(
            Box::new(LayerMetaCommand::new(before, after, "新建图层")),
            &mut document,
        );
        let _ = (index, active);

        let steps = history.recorder().expect("在录制").steps();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].label, "新建图层");
        assert!(steps[0].patch.is_none());
    }

    #[test]
    fn disabling_the_recorder_drops_what_was_recorded() {
        let mut document = document();
        let mut history = History::new();
        history.record_timeline(true);
        paint(&mut history, &mut document, Color::RED);
        history.record_timeline(false);
        assert!(history.recorder().is_none());
        paint(&mut history, &mut document, Color::WHITE);
        assert!(history.recorder().is_none(), "关掉之后不再记录");
    }

    #[test]
    fn the_step_limit_drops_the_oldest_steps() {
        let mut recorder = Recorder::with_limit(2);
        let mut document = document();
        let mut history = History::new();
        history.record_timeline(true);
        // 直接测 Recorder 的上限（History 里那台是默认上限）。
        for color in [Color::RED, Color::WHITE, Color::BLACK] {
            let id = document.active_layer.expect("背景层");
            let before = document.layer(id).expect("层在").pixels.clone();
            document
                .layer_mut(id)
                .expect("层在")
                .pixels
                .set_pixel(3, 3, color);
            let after = document.layer(id).expect("层在").pixels.clone();
            if let Some(command) = PaintCommand::capture(id, &before, &after, "画笔") {
                recorder.record(&command);
            }
        }
        assert_eq!(recorder.len(), 2, "上限 2");
        let last = recorder.steps().last().expect("有步");
        assert_eq!(
            last.patch.as_ref().expect("补丁").pixels,
            vec![Color::BLACK]
        );
    }
}
