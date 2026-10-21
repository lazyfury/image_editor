//! 主页：画廊条目（打开中的工作区 + 最近文件）、回到主页、画廊点击请求、缩略图上传。

use super::*;

/// 缩略图最长边（像素）：画廊的框是 56 逻辑像素，2 倍屏也够。
const THUMBNAIL_MAX_EDGE: u32 = 96;

/// 主页画廊的条目：打开中的工作区 + 磁盘上的最近工作区（按路径去重）。
///
/// 每条顺带要一张缩略图：像素放进 `uploads` 队列，纹理 id 由条目下标决定
/// （[`thumbnail_texture`]）。宿主每帧把队列灌进后端
/// （[`ImageEditorLogic::sync_home_thumbnails`]）—— 视图层不认识后端。
pub(super) fn home_entries(
    workspaces: &Workspaces,
    uploads: &mut Vec<(TextureId, PixelBuffer)>,
) -> Vec<HomeEntry> {
    let mut entries: Vec<HomeEntry> = Vec::new();
    for (index, tab) in workspaces.tabs().iter().enumerate() {
        // 打开中的工作区用内存里的文档（可能还没保存过），比磁盘文件新。
        let thumbnail = workspaces
            .get(index)
            .map(|workspace| workspace.editor.thumbnail(THUMBNAIL_MAX_EDGE))
            .map(|pixels| queued_thumbnail(entries.len(), pixels, uploads));
        entries.push(HomeEntry {
            name: tab.name.clone(),
            detail: if tab.dirty {
                "打开中 · 未保存".to_string()
            } else {
                "打开中".to_string()
            },
            open_index: Some(index),
            path: None,
            thumbnail,
        });
    }
    let open_paths = workspaces.paths();
    for path in crate::io::load_recent() {
        if open_paths.contains(&path) {
            continue;
        }
        let summary = crate::io::read_workspace_summary(&path);
        let name = match &summary {
            Ok(summary) => summary.name.clone(),
            Err(_) => path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("工作区")
                .to_string(),
        };
        let detail = match &summary {
            Ok(summary) => format!(
                "{} × {} · {} 图层",
                summary.width, summary.height, summary.layers
            ),
            Err(_) => path.display().to_string(),
        };
        let thumbnail =
            file_thumbnail(&path).map(|pixels| queued_thumbnail(entries.len(), pixels, uploads));
        entries.push(HomeEntry {
            name,
            detail,
            open_index: None,
            path: Some(path),
            thumbnail,
        });
    }
    entries
}

/// 文档 → 画廊缩略图像素（整图合成后等比缩小到 [`THUMBNAIL_MAX_EDGE`]）。
pub(super) fn thumbnail_of(document: &Document) -> PixelBuffer {
    let mut target = RenderTarget::new(document.width, document.height);
    CpuRenderer.render(document, &mut target);
    target.pixels.scaled_to_fit(THUMBNAIL_MAX_EDGE)
}

/// 磁盘上工作区的缩略图：先读缓存，没有就合成一次并写回缓存。
fn file_thumbnail(path: &Path) -> Option<PixelBuffer> {
    if let Some(cached) = crate::io::load_thumbnail(path) {
        return Some(cached);
    }
    let document = crate::io::read_workspace(path).ok()?;
    let pixels = thumbnail_of(&document);
    if let Err(error) = crate::io::store_thumbnail(path, &pixels) {
        tracing::warn!(target: "image_editor", %error, "thumbnail_cache_write_failed");
    }
    Some(pixels)
}

/// 把一张缩略图挂到第 `index` 条上，并排队等上传。
fn queued_thumbnail(
    index: usize,
    pixels: PixelBuffer,
    uploads: &mut Vec<(TextureId, PixelBuffer)>,
) -> HomeThumbnail {
    let texture = thumbnail_texture(index);
    let thumbnail = HomeThumbnail {
        texture,
        width: pixels.width,
        height: pixels.height,
    };
    uploads.push((texture, pixels));
    thumbnail
}

impl ImageEditorLogic {
    /// 回到主页（工作区保留在内存里）。
    pub(super) fn go_home(&mut self) {
        let mut uploads = Vec::new();
        let entries = home_entries(&self.workspaces, &mut uploads);
        let mut home = HomeView::new(self.theme, entries);
        if let Some(measurer) = self.measurer.clone() {
            home.set_text_measurer(measurer);
        }
        self.thumbnail_uploads = uploads;
        self.screen = Screen::Home(home);
        self.title_dirty = true;
    }

    /// 把画廊缩略图的像素灌进后端（缩略图总是缩小显示，用线性采样）。
    ///
    /// 队列只在重建主页时补满；没有后端（无头）时就先攒着，等拿到再传。
    pub(super) fn sync_home_thumbnails(&mut self, ctx: &FrameContext<'_>) {
        if self.thumbnail_uploads.is_empty() {
            return;
        }
        let Some(backend) = ctx.service::<SharedBackend>().cloned() else {
            return;
        };
        for (texture, pixels) in std::mem::take(&mut self.thumbnail_uploads) {
            let mut backend = backend.borrow_mut();
            backend.set_texture_filter(texture, TextureFilter::Linear);
            if let Err(error) =
                backend.update_texture(texture, pixels.width, pixels.height, &pixels.data)
            {
                tracing::warn!(target: "image_editor", %error, "thumbnail_upload_failed");
            }
        }
    }

    /// 主页画廊的点击请求。
    pub(super) fn handle_home_requests(&mut self, ctx: &FrameContext<'_>) {
        let request = match &self.screen {
            Screen::Home(home) => home.take_open_request(),
            Screen::Editor => None,
        };
        match request {
            Some(HomeOpenRequest::Workspace(index)) => {
                self.workspaces.set_active(index);
                if let Some(workspace) = self.workspaces.active_mut() {
                    workspace.editor.mark_texture_dirty();
                }
                self.screen = Screen::Editor;
                self.title_dirty = true;
            }
            Some(HomeOpenRequest::Recent(path)) => self.open_workspace(ctx, &path),
            None => {}
        }
    }
}
