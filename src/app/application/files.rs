//! 文件 / 工作区操作：保存、打开、导入导出、关闭确认、拖放。

use super::*;

impl ImageEditorLogic {
    /// 起始目录：当前工作区父目录 → `$HOME`。
    pub(super) fn start_dir(&self) -> Option<PathBuf> {
        self.workspaces
            .active()
            .and_then(|workspace| workspace.path.as_ref())
            .and_then(|path| path.parent())
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
    }

    /// 文件面板 / 快捷键留下的文件意图。
    pub(super) fn handle_file_requests(&mut self, ctx: &FrameContext<'_>) {
        let request = self
            .workspaces
            .active()
            .and_then(|workspace| workspace.editor.take_file_request());
        let Some(request) = request else {
            return;
        };
        match request {
            FileRequest::Save => {
                let index = self.workspaces.active_index();
                self.save_workspace_at(index, false);
            }
            FileRequest::SaveAs => {
                let index = self.workspaces.active_index();
                self.save_workspace_at(index, true);
            }
            FileRequest::Open => self.open_via_dialog(ctx),
            FileRequest::ImportPng => self.import_via_dialog(),
            FileRequest::ExportPng => self.export_via_dialog(),
        }
    }

    /// 保存第 `index` 个工作区。`force_dialog`（另存为）或还没路径时弹保存框；
    /// 返回是否真的写盘（用户取消 / 失败返回 `false`）。
    pub(super) fn save_workspace_at(&mut self, index: usize, force_dialog: bool) -> bool {
        let Some(workspace) = self.workspaces.get(index) else {
            return false;
        };
        let target = match (&workspace.path, force_dialog) {
            (Some(path), false) => path.clone(),
            _ => {
                let options = DialogOptions {
                    title: "保存工作区".to_string(),
                    filters: vec![FileFilter::new("工作区", &[crate::io::WORKSPACE_EXTENSION])],
                    default_name: Some(format!("{}.igui", workspace.title())),
                    start_dir: self.start_dir(),
                };
                let Some(path) = self.dialog.pick_save(&options) else {
                    return false;
                };
                path
            }
        };
        let Some(workspace) = self.workspaces.get(index) else {
            return false;
        };
        let document = workspace.editor.document_snapshot();
        match crate::io::write_workspace(&target, &document) {
            Ok(()) => {
                if let Some(workspace) = self.workspaces.get_mut(index) {
                    workspace.mark_saved(target.clone());
                }
                let _ = crate::io::record_recent(&target);
                // 画廊预览：工作区刚更新过，顺手把缩略图缓存重写一份。
                if let Err(error) =
                    crate::io::store_thumbnail(&target, &super::home::thumbnail_of(&document))
                {
                    tracing::warn!(target: "image_editor", %error, "thumbnail_cache_write_failed");
                }
                if let Some(workspace) = self.workspaces.get(index) {
                    workspace
                        .editor
                        .set_message(format!("已保存工作区：{}", target.display()));
                }
                tracing::info!(
                    target: "image_editor",
                    path = target.to_string_lossy().as_ref(),
                    "workspace_saved"
                );
                self.flush_timeline(index);
                self.title_dirty = true;
                true
            }
            Err(error) => {
                if let Some(workspace) = self.workspaces.get(index) {
                    workspace.editor.set_message(format!("保存失败：{error}"));
                }
                false
            }
        }
    }

    /// 弹打开框选一个 `.igui` 并打开。
    pub(super) fn open_via_dialog(&mut self, ctx: &FrameContext<'_>) {
        let options = DialogOptions {
            title: "打开工作区".to_string(),
            filters: vec![FileFilter::new("工作区", &[crate::io::WORKSPACE_EXTENSION])],
            default_name: None,
            start_dir: self.start_dir(),
        };
        if let Some(path) = self.dialog.pick_open(&options) {
            self.open_workspace(ctx, &path);
        }
    }

    /// 弹打开框选一张 PNG，作为新图层导入。
    pub(super) fn import_via_dialog(&mut self) {
        let options = DialogOptions {
            title: "导入 PNG".to_string(),
            filters: vec![FileFilter::new("PNG 图片", &["png"])],
            default_name: None,
            start_dir: self.start_dir(),
        };
        let Some(path) = self.dialog.pick_open(&options) else {
            return;
        };
        self.import_png(&path);
    }

    /// 把一张 PNG 导入当前工作区（新图层）。
    pub(super) fn import_png(&mut self, path: &Path) {
        let Some(workspace) = self.workspaces.active_mut() else {
            return;
        };
        let path_text = path.to_string_lossy().into_owned();
        let message = match workspace.editor.import_png_from(&path_text) {
            Ok((width, height)) => {
                tracing::info!(
                    target: "image_editor",
                    path = path_text.as_str(),
                    width,
                    height,
                    "png_imported"
                );
                format!("已导入 PNG：{path_text}（{width} × {height}）")
            }
            Err(error) => format!("导入失败：{error}"),
        };
        workspace.editor.set_message(message);
    }

    /// 弹保存框选一个 `.png`，导出当前文档的合成。
    pub(super) fn export_via_dialog(&mut self) {
        let Some(workspace) = self.workspaces.active() else {
            return;
        };
        let options = DialogOptions {
            title: "导出 PNG".to_string(),
            filters: vec![FileFilter::new("PNG 图片", &["png"])],
            default_name: Some(format!("{}.png", workspace.editor.document_name())),
            start_dir: self.start_dir(),
        };
        let Some(path) = self.dialog.pick_save(&options) else {
            return;
        };
        let path_text = path.to_string_lossy().into_owned();
        let message = match workspace.editor.export_png_to(&path_text) {
            Ok(()) => {
                tracing::info!(target: "image_editor", path = path_text.as_str(), "png_exported");
                format!("已导出 PNG：{path_text}")
            }
            Err(error) => format!("导出失败：{error}"),
        };
        workspace.editor.set_message(message);
    }

    /// 关闭工作区：有未保存改动先弹确认框。
    pub(super) fn request_close(&mut self, index: usize) {
        let dirty = self
            .workspaces
            .get(index)
            .map(|workspace| workspace.dirty())
            .unwrap_or(false);
        if dirty {
            self.pending_close = Some(index);
            let name = self
                .workspaces
                .get(index)
                .map(|workspace| workspace.title())
                .unwrap_or_default();
            let mut confirm = ConfirmDialog::new(
                self.theme,
                "未保存的改动",
                &format!("「{name}」有未保存的改动。关闭前要保存吗？"),
            );
            if let Some(measurer) = self.measurer.clone() {
                confirm.set_text_measurer(measurer);
            }
            self.confirm = Some(confirm);
        } else {
            self.close_workspace(index);
        }
    }

    /// 真正移除一个工作区。
    pub(super) fn close_workspace(&mut self, index: usize) {
        // 关闭前把过程记录落盘，否则加载到一半的延时动画就随着工作区丢了。
        self.flush_timeline(index);
        // 关的是激活的那一个：后端只有一张 DOCUMENT_TEXTURE，切到“下一个”后必须
        // 重新合成上传，否则画布还停在已关闭工作区的图像上（关标签看起来没反应）。
        let closed_active = self.workspaces.active_index() == index;
        self.workspaces.remove(index);
        if closed_active {
            if let Some(workspace) = self.workspaces.active_mut() {
                workspace.editor.mark_texture_dirty();
            }
        }
        if self.workspaces.is_empty() {
            self.go_home();
        } else {
            self.title_dirty = true;
        }
    }

    /// 把当前记录的过程写到工作区旁边的 `.igtl`（`x.igui` -> `x.igtl`）。
    ///
    /// 这是「全部保存」的口子：只在**保存 / 关闭**这种自然断点写盘，编辑期间
    /// 时间线只在内存里长（每笔一个补丁），不碰磁盘。没有路径（从没保存过）
    /// 或没开记录时静默跳过。
    pub(super) fn flush_timeline(&mut self, index: usize) {
        let Some(workspace) = self.workspaces.get_mut(index) else {
            return;
        };
        let Some(path) = workspace.path.clone() else {
            return;
        };
        let Some(steps) = workspace.editor.timeline() else {
            return;
        };
        let target = crate::io::timeline_path_for(&path);
        let message = match crate::io::write_timeline(&target, &steps) {
            Ok(()) => {
                tracing::info!(
                    target: "image_editor",
                    steps = steps.len(),
                    path = target.to_string_lossy().as_ref(),
                    "timeline_saved"
                );
                format!("已记录 {} 步：{}", steps.len(), target.display())
            }
            Err(error) => format!("时间线写入失败：{error}"),
        };
        workspace.editor.set_message(message);
    }

    /// 确认框出结果（或被动关闭）后收尾。
    /// 确认框出结果（或被动关闭）后收尾；用户还没点就保持打开。
    pub(super) fn finish_confirm(&mut self) {
        let Some(confirm) = self.confirm.as_ref() else {
            return;
        };
        let result = confirm.take_result();
        let closed = confirm.take_close_request();
        // 什么都没发生（还没点）：确认框继续开着，等用户选。
        let Some(outcome) = resolve_confirm(result, closed) else {
            return;
        };
        let Some(index) = self.pending_close else {
            return;
        };
        self.confirm = None;
        self.pending_close = None;
        match outcome {
            ConfirmResult::Save => {
                if self.save_workspace_at(index, false) {
                    self.close_workspace(index);
                }
            }
            ConfirmResult::Discard => self.close_workspace(index),
            ConfirmResult::Cancel => {}
        }
    }

    /// 拖放的文件：有工作区就当新图层导入，没有就从图片开一个新文档。
    pub(super) fn handle_dropped_files(&mut self, ctx: &FrameContext<'_>) {
        let Some(shared) = ctx.service::<crate::app::drop_files::DroppedFiles>() else {
            return;
        };
        let files: Vec<PathBuf> = shared.0.borrow_mut().drain(..).collect();
        for path in files {
            match &self.screen {
                Screen::Editor => self.import_png(&path),
                Screen::Home(_) => self.open_document_from_image(ctx, &path),
            }
        }
    }

    /// 主页拖入一张图：按图片尺寸建一个新文档，再把它作为图层导入。
    pub(super) fn open_document_from_image(&mut self, ctx: &FrameContext<'_>, path: &Path) {
        let Ok(pixels) = crate::io::read_png(path) else {
            // 不是能解码的图片，就当工作区文件试试。
            self.open_workspace(ctx, path);
            return;
        };
        let state = AppState::with_document(
            pixels.width,
            pixels.height,
            crate::document::Color::TRANSPARENT,
        );
        let mut editor = EditorView::new(self.theme, state);
        self.configure_editor(&mut editor);
        if let Some(backend) = ctx.service::<SharedBackend>().cloned() {
            backend
                .borrow_mut()
                .set_texture_filter(crate::canvas::DOCUMENT_TEXTURE, TextureFilter::Nearest);
            if let Some(pixels) = editor.take_texture_upload() {
                upload_texture(&backend, &pixels);
            }
        }
        self.workspaces.push(Workspace::new(editor));
        self.screen = Screen::Editor;
        self.title_dirty = true;
        self.import_png(path);
    }

    /// 从 `.igui` 打开一个新工作区。
    pub(super) fn open_workspace(&mut self, ctx: &FrameContext<'_>, path: &Path) {
        match crate::io::read_workspace(path) {
            Ok(document) => {
                let mut editor = EditorView::new(self.theme, AppState::from_document(document));
                self.configure_editor(&mut editor);
                if let Some(backend) = ctx.service::<SharedBackend>().cloned() {
                    backend.borrow_mut().set_texture_filter(
                        crate::canvas::DOCUMENT_TEXTURE,
                        TextureFilter::Nearest,
                    );
                    if let Some(pixels) = editor.take_texture_upload() {
                        upload_texture(&backend, &pixels);
                    }
                }
                self.workspaces
                    .push(Workspace::from_path(editor, path.to_path_buf()));
                let _ = crate::io::record_recent(path);
                self.screen = Screen::Editor;
                self.title_dirty = true;
                tracing::info!(
                    target: "image_editor",
                    path = path.to_string_lossy().as_ref(),
                    "workspace_opened"
                );
            }
            Err(error) => {
                if let Some(workspace) = self.workspaces.active() {
                    workspace.editor.set_message(format!("打开失败：{error}"));
                }
                tracing::warn!(
                    target: "image_editor",
                    path = path.to_string_lossy().as_ref(),
                    error = %error,
                    "workspace_open_failed"
                );
            }
        }
    }

    /// 文档换了就把「名字 — igui 图像编辑器」写回原生窗口标题。
    pub(super) fn apply_title(&self, ctx: &FrameContext<'_>) {
        let title = match &self.screen {
            Screen::Editor => match self.workspaces.active() {
                Some(workspace) => {
                    let dirty = if workspace.dirty() { "• " } else { "" };
                    format!("{dirty}{} — igui 图像编辑器", workspace.title())
                }
                None => "igui 图像编辑器".to_string(),
            },
            Screen::Home(_) => "igui 图像编辑器".to_string(),
        };
        if let Some(window) = ctx.service::<SharedWindow>() {
            if let Some(window) = window.borrow().as_ref() {
                window.set_title(&title);
            }
        }
    }
}
