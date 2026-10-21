//! 工作区：新建 / 切标签 / 每帧同步（偏好、纹理、缩略图采样）。
//!
//! 打开 / 保存 / 导入 / 导出这些「文件动作」在 `files.rs`；这里只管工作区本身
//! 的生命周期与每帧的编辑器同步。

use super::*;

impl ImageEditorLogic {
    /// 工作区编辑器统一配置：字体度量 + 系统剪贴板 + 顶部给标签条留位置 + 笔刷偏好。
    pub(super) fn configure_editor(&self, editor: &mut EditorView) {
        if let Some(measurer) = self.measurer.clone() {
            editor.set_text_measurer(measurer);
        }
        if let Some(clipboard) = self.clipboard.clone() {
            editor.set_clipboard(clipboard);
        }
        editor.set_tab_band(TAB_STRIP_HEIGHT);
        // 系统菜单栏装了就把窗口里那一行折叠（标签条贴顶，见 `layout`）。
        editor.set_menu_bar_visible(self.native_menu.is_none());
        editor.apply_preferences(&self.preferences.brush);
        // 过程记录（延时动画用）跟随偏好；关着时 `History` 不建录制器。
        editor.set_record_timeline(self.preferences.record_timeline);
        editor.set_image_clipboard(self.image_clipboard.clone());
    }

    /// 把「新建文档」的选择变成一个新工作区（并切到编辑器）。
    pub(super) fn open_editor(&mut self, ctx: &FrameContext<'_>, spec: NewDocumentSpec) {
        let state = AppState::with_document(spec.width, spec.height, spec.background);
        let mut editor = EditorView::new(self.theme, state);
        self.configure_editor(&mut editor);

        // 文档纹理放大时用最近邻：硬边像素而不是模糊插值。首帧之前上传，
        // 否则视图画的文档图像会先退回白色兜底。
        if let Some(backend) = ctx.service::<SharedBackend>().cloned() {
            backend
                .borrow_mut()
                .set_texture_filter(crate::canvas::DOCUMENT_TEXTURE, TextureFilter::Nearest);
            if let Some(pixels) = editor.take_texture_upload() {
                upload_texture(&backend, &pixels);
            }
        }

        tracing::info!(
            target: "image_editor",
            name = editor.document_name().as_str(),
            width = spec.width,
            height = spec.height,
            "document_created"
        );
        self.workspaces.push(Workspace::new(editor));
        self.screen = Screen::Editor;
        self.title_dirty = true;
    }

    /// 处理标签条请求。
    pub(super) fn handle_tab_action(&mut self, action: WorkspaceTabAction) {
        match action {
            WorkspaceTabAction::Switch(index) => {
                self.workspaces.set_active(index);
                // 后端只有一张 DOCUMENT_TEXTURE；切过去时让新工作区重新合成上传，
                // 否则画布还停在上一个工作区的图像上。
                if let Some(workspace) = self.workspaces.active_mut() {
                    workspace.editor.mark_texture_dirty();
                }
                self.title_dirty = true;
            }
            WorkspaceTabAction::Close(index) => self.request_close(index),
            WorkspaceTabAction::New => self.open_new_document_modal(),
            WorkspaceTabAction::Home => self.go_home(),
        }
    }

    /// 每帧同步当前工作区：路径、`update`、笔刷偏好回写、纹理上传与采样方式。
    pub(super) fn sync_active_workspace(&mut self, ctx: &FrameContext<'_>) {
        if let Some(workspace) = self.workspaces.active_mut() {
            let path = workspace.path.clone();
            workspace.editor.set_workspace_path(path.as_deref());
            workspace.editor.update();
            // 属性窗口能改「记录编辑过程」；以编辑器为准，变了就写回配置。
            let record = workspace.editor.records_timeline();
            let record_changed = self.preferences.record_timeline != record;
            if record_changed {
                self.preferences.record_timeline = record;
            }
            if let Some(brush) = workspace.editor.take_preferences() {
                self.preferences.brush = brush;
                if let Err(error) = crate::io::save_preferences(&self.preferences) {
                    tracing::warn!(
                        target: "image_editor",
                        %error,
                        "preferences_write_failed"
                    );
                }
            } else if record_changed {
                if let Err(error) = crate::io::save_preferences(&self.preferences) {
                    tracing::warn!(
                        target: "image_editor",
                        %error,
                        "preferences_write_failed"
                    );
                }
            }
            if let Some(backend) = ctx.service::<SharedBackend>().cloned() {
                let scale = backend.borrow().scale_factor();
                workspace.editor.set_scale_factor(scale);
                // 缩小用线性、放大用最近邻：4K 适配窗口是 0.18 倍，最近邻会把
                // 细笔迹直接采没（看着像毛刺）。
                let smoothing = workspace.editor.image_smoothing();
                if smoothing != self.image_smoothing {
                    self.image_smoothing = smoothing;
                    backend.borrow_mut().set_texture_filter(
                        crate::canvas::DOCUMENT_TEXTURE,
                        match smoothing {
                            crate::canvas::ImageSmoothing::Nearest => TextureFilter::Nearest,
                            crate::canvas::ImageSmoothing::Linear => TextureFilter::Linear,
                        },
                    );
                }
                if let Some(pixels) = workspace.editor.take_texture_upload() {
                    upload_texture(&backend, &pixels);
                }
            }
        }
    }
}
