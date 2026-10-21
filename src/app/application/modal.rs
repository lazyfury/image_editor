//! 「新建文档」模态框与「保存 / 不保存 / 取消」确认框的生命周期。
//!
//! 两者的共同点：**在时吞掉全部输入**（见 `mod.rs::event`），结果由宿主取走后
//! 在这里收尾。模态本身（拖动 / 缩放 / 遮罩）在 `ui/window` + `ui/new_document`。

use super::*;

impl ImageEditorLogic {
    /// 主页点过「新建项目」就打开模态框（同时只允许一个）。
    pub(super) fn open_modal_if_requested(&mut self) {
        let requested =
            matches!(&self.screen, Screen::Home(home) if home.take_new_document_request());
        if requested {
            self.open_new_document_modal();
        }
    }

    /// 打开「新建文档」模态窗口。
    pub(super) fn open_new_document_modal(&mut self) {
        if self.modal.is_some() {
            return;
        }
        let mut modal = NewDocumentModal::new(self.theme);
        if let Some(measurer) = self.measurer.clone() {
            modal.set_text_measurer(measurer);
        }
        self.modal = Some(modal);
        tracing::info!(target: "image_editor", "new_document_modal_opened");
    }

    /// 把模态里的选择变化同步到它的标签。
    pub(super) fn sync_modal(&mut self) {
        if let Some(modal) = self.modal.as_mut() {
            modal.update();
        }
    }

    /// 模态出结果（创建）或被动关闭（Esc / 点外）后收尾。
    pub(super) fn finish_modal(&mut self, ctx: &FrameContext<'_>) {
        let Some(modal) = self.modal.as_ref() else {
            return;
        };
        let Some(result) = modal.take_result() else {
            return;
        };
        self.modal = None;
        if let NewDocumentResult::Create(spec) = result {
            self.open_editor(ctx, spec);
        }
    }
}
