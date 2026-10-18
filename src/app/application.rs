//! 窗口宿主：用 `igui_app` 运行时 + `igui_winit` 插件组装**单窗口**应用。
//!
//! 视图（[`crate::ui::HomeView`] / [`crate::ui::EditorView`]）完全不碰平台；
//! 这一层把它们接进 `igui_app` 的插件运行时：
//!
//! ```text
//! winit events -> InputEvent -> AppLogic -> view -> DrawList -> WgpuPresenter -> surface
//! ```
//!
//! **单窗口 + 应用内模态窗口**：首屏是主页；点「新建项目」不再开第二个原生
//! 窗口（macOS 15 上多 surface 会崩），而是在主窗口里弹出一扇模拟桌面的浮动
//! 窗口（[`crate::ui::window::Window`]）：标题栏可拖动移动，四条边 / 角可拖动
//! 缩放，带关闭按钮与遮罩，Esc / 点窗口外取消，输入全被它吞掉。
//!
//! `--frames N` 由 [`FrameLimitPlugin`] 在 N 帧后结束进程，让真实渲染管线能在
//! 无头环境里被验证。

use std::rc::Rc;

use igui_app::{
    App, AppBuilder, AppConfig, AppLogic, EventContext, FrameContext, FrameObserver, InitContext,
    LifecycleObserver, Plugin,
};
use igui_backend_wgpu::{FontConfig, FontMetrics, FontMode, TextureFilter};
use igui_core::{Color, EventResult, InputEvent, Key, Modifiers};
use igui_render::{DrawList, PaintContext};
use igui_theme::{SurfaceLevel, Theme};
use igui_ui::TextMeasurer;
use igui_winit::{
    ClipboardPlugin, GpuConfig, ImePlugin, KeyboardPlugin, PointerPlugin, SharedBackend,
    SharedWindow, WgpuPlugin, WindowConfig, WinitPlugin,
};

use crate::app::state::AppState;
use crate::app::workspaces::{Workspace, Workspaces};
use crate::document::PixelBuffer;
use crate::ui::{
    EditorView, HomeEntry, HomeOpenRequest, HomeView, NewDocumentModal, NewDocumentResult,
    NewDocumentSpec, WorkspaceRequest, WorkspaceTabAction, WorkspaceTabs, TAB_STRIP_HEIGHT,
};

/// 主窗口（也是唯一的窗口）的初始大小。
const WINDOW_WIDTH: f64 = 1280.0;
const WINDOW_HEIGHT: f64 = 800.0;

/// 命令行选项（由 [`crate::main`] 解析）。
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// 渲染这么多帧后退出（`None` = 一直跑）。
    pub frames: Option<u32>,
    /// 使用浅色主题（默认深色）。
    pub light: bool,
    /// 使用内置点阵字体（不含中文；默认系统字体）。
    pub pixel_font: bool,
}

/// 用 `igui_app` 运行时打开窗口跑编辑器。
pub fn run(options: Options) {
    build_app(options).run();
}

/// 组装插件运行时：平台插件（winit / wgpu / 输入 / 文本度量）+ 应用逻辑。
fn build_app(options: Options) -> App {
    let theme = crate::theme::editor_theme(options.light);
    let font_mode = if options.pixel_font {
        FontMode::Pixel
    } else {
        FontMode::System
    };

    let mut builder = App::new(AppConfig {
        title: "igui 图像编辑器".into(),
        size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        ..Default::default()
    })
    .plugin(WinitPlugin::new(WindowConfig {
        title: "igui 图像编辑器".into(),
        size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        // 图层 / 文件路径要能输入中文，所以打开平台 IME。
        ime: true,
        ..Default::default()
    }))
    .plugin(WgpuPlugin::new(GpuConfig {
        clear_color: theme_background(theme),
        font: FontConfig {
            mode: font_mode,
            device_pixel_rasterization: true,
            default_family: None,
            default_face: crate::fonts::bundled_face(),
        },
        ..Default::default()
    }))
    // 用编辑器自己的 `ascent` 校正注册度量（见 `EditorTextMeasurer`）。
    .plugin(EditorTextMeasurePlugin)
    .plugin(PointerPlugin)
    .plugin(KeyboardPlugin)
    .plugin(ImePlugin)
    .plugin(ClipboardPlugin);

    if let Some(frames) = options.frames {
        builder = builder.plugin(FrameLimitPlugin { frames });
    }

    builder.logic(ImageEditorLogic::new(theme)).build()
}

/// 应用逻辑：主页 / 多工作区编辑器 +「新建文档」模态框，都在这一个 `AppLogic` 里。
struct ImageEditorLogic {
    theme: &'static dyn Theme,
    /// 后端真实字体度量（`init` 时从服务里取；模态 / 工作区 / 标签条共享）。
    measurer: Option<Rc<dyn TextMeasurer>>,
    screen: Screen,
    /// 打开中的工作区；回主页时也保留，方便再切回来。
    workspaces: Workspaces,
    /// 顶部工作区标签条。
    tabs: WorkspaceTabs,
    /// 「新建文档」模态窗口；有它时输入全交给它。
    modal: Option<NewDocumentModal>,
    modifiers: Modifiers,
    /// 文档换了标题，下一帧写回原生窗口。
    title_dirty: bool,
}

/// 主窗口当前显示哪一屏。编辑器的工作区在 `ImageEditorLogic::workspaces`。
enum Screen {
    Home(HomeView),
    Editor,
}

/// 主页画廊的条目：打开中的工作区 + 磁盘上的最近工作区（按路径去重）。
fn home_entries(workspaces: &Workspaces) -> Vec<HomeEntry> {
    let mut entries: Vec<HomeEntry> = workspaces
        .tabs()
        .iter()
        .enumerate()
        .map(|(index, tab)| HomeEntry {
            name: tab.name.clone(),
            detail: if tab.dirty {
                "打开中 · 未保存".to_string()
            } else {
                "打开中".to_string()
            },
            open_index: Some(index),
            path: None,
        })
        .collect();
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
        entries.push(HomeEntry {
            name,
            detail,
            open_index: None,
            path: Some(path),
        });
    }
    entries
}

impl ImageEditorLogic {
    fn new(theme: &'static dyn Theme) -> Self {
        let workspaces = Workspaces::new();
        let home = HomeView::new(theme, home_entries(&workspaces));
        Self {
            theme,
            measurer: None,
            screen: Screen::Home(home),
            workspaces,
            tabs: WorkspaceTabs::new(theme),
            modal: None,
            modifiers: Modifiers::NONE,
            title_dirty: false,
        }
    }

    /// Ctrl/Cmd 按下：此时字母键是快捷键，不该再当文本送进输入框。
    fn shortcut_modifier(&self) -> bool {
        self.modifiers.ctrl || self.modifiers.meta
    }

    /// 主页点过「新建项目」就打开模态框（同时只允许一个）。
    fn open_modal_if_requested(&mut self) {
        let requested =
            matches!(&self.screen, Screen::Home(home) if home.take_new_document_request());
        if requested {
            self.open_new_document_modal();
        }
    }

    /// 打开「新建文档」模态窗口。
    fn open_new_document_modal(&mut self) {
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
    fn sync_modal(&mut self) {
        if let Some(modal) = self.modal.as_mut() {
            modal.update();
        }
    }

    /// 模态出结果（创建）或被动关闭（Esc / 点外）后收尾。
    fn finish_modal(&mut self, ctx: &FrameContext<'_>) {
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

    /// 工作区编辑器统一配置：字体度量 + 顶部给标签条留位置。
    fn configure_editor(&self, editor: &mut EditorView) {
        if let Some(measurer) = self.measurer.clone() {
            editor.set_text_measurer(measurer);
        }
        editor.set_top_inset(TAB_STRIP_HEIGHT);
    }

    /// 把「新建文档」的选择变成一个新工作区（并切到编辑器）。
    fn open_editor(&mut self, ctx: &FrameContext<'_>, spec: NewDocumentSpec) {
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

    /// 回到主页（工作区保留在内存里）。
    fn go_home(&mut self) {
        let mut home = HomeView::new(self.theme, home_entries(&self.workspaces));
        if let Some(measurer) = self.measurer.clone() {
            home.set_text_measurer(measurer);
        }
        self.screen = Screen::Home(home);
        self.title_dirty = true;
    }

    /// 主页画廊的点击请求。
    fn handle_home_requests(&mut self, ctx: &FrameContext<'_>) {
        let request = match &self.screen {
            Screen::Home(home) => home.take_open_request(),
            Screen::Editor => None,
        };
        match request {
            Some(HomeOpenRequest::Workspace(index)) => {
                self.workspaces.set_active(index);
                self.screen = Screen::Editor;
                self.title_dirty = true;
            }
            Some(HomeOpenRequest::Recent(path)) => self.open_workspace(ctx, &path),
            None => {}
        }
    }

    /// 处理标签条请求。
    fn handle_tab_action(&mut self, action: WorkspaceTabAction) {
        match action {
            WorkspaceTabAction::Switch(index) => self.workspaces.set_active(index),
            WorkspaceTabAction::Close(index) => {
                self.workspaces.remove(index);
                if self.workspaces.is_empty() {
                    self.go_home();
                } else {
                    self.title_dirty = true;
                }
            }
            WorkspaceTabAction::New => self.open_new_document_modal(),
            WorkspaceTabAction::Home => self.go_home(),
        }
    }

    /// 文件面板留下的工作区保存 / 打开请求。
    fn handle_workspace_requests(&mut self, ctx: &FrameContext<'_>) {
        let request = self
            .workspaces
            .active()
            .and_then(|workspace| workspace.editor.take_workspace_request());
        let Some(request) = request else {
            return;
        };
        let path = self
            .workspaces
            .active()
            .map(|workspace| workspace.editor.io_path())
            .unwrap_or_default();
        match request {
            WorkspaceRequest::Save => self.save_workspace(&path),
            WorkspaceRequest::Open => self.open_workspace(ctx, std::path::Path::new(&path)),
        }
    }

    /// 把当前工作区保存成 `.igui`（路径扩展名换成 `.igui`）。
    fn save_workspace(&mut self, path: &str) {
        if path.trim().is_empty() {
            if let Some(workspace) = self.workspaces.active() {
                workspace.editor.set_message("先点「改路径」设置保存路径");
            }
            return;
        }
        let target = crate::io::workspace_path_from(path);
        let Some(workspace) = self.workspaces.active() else {
            return;
        };
        let document = workspace.editor.document_snapshot();
        match crate::io::write_workspace(&target, &document) {
            Ok(()) => {
                if let Some(workspace) = self.workspaces.active_mut() {
                    workspace.mark_saved(target.clone());
                }
                let _ = crate::io::record_recent(&target);
                if let Some(workspace) = self.workspaces.active() {
                    workspace
                        .editor
                        .set_message(format!("已保存工作区：{}", target.display()));
                }
                tracing::info!(
                    target: "image_editor",
                    path = target.to_string_lossy().as_ref(),
                    "workspace_saved"
                );
                self.title_dirty = true;
            }
            Err(error) => {
                if let Some(workspace) = self.workspaces.active() {
                    workspace.editor.set_message(format!("保存失败：{error}"));
                }
            }
        }
    }

    /// 从 `.igui` 打开一个新工作区。
    fn open_workspace(&mut self, ctx: &FrameContext<'_>, path: &std::path::Path) {
        if path.as_os_str().is_empty() {
            if let Some(workspace) = self.workspaces.active() {
                workspace.editor.set_message("先点「改路径」设置工作区路径");
            }
            return;
        }
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
    fn apply_title(&self, ctx: &FrameContext<'_>) {
        let title = match &self.screen {
            Screen::Editor => match self.workspaces.active() {
                Some(workspace) => {
                    format!("{} — igui 图像编辑器", workspace.editor.document_name())
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

impl AppLogic for ImageEditorLogic {
    fn init(&mut self, ctx: &InitContext<'_>) {
        // 度量插件已经在 `resumed` 时把度量放进服务。
        self.measurer = ctx.service::<Rc<dyn TextMeasurer>>().cloned();
        if let Some(measurer) = self.measurer.clone() {
            self.tabs.set_text_measurer(measurer.clone());
            if let Screen::Home(home) = &mut self.screen {
                home.set_text_measurer(measurer.clone());
            }
            if let Some(workspace) = self.workspaces.active_mut() {
                workspace.editor.set_text_measurer(measurer);
            }
        }
    }

    fn event(&mut self, _ctx: &EventContext<'_>, event: &InputEvent) -> EventResult {
        if let InputEvent::ModifiersChanged(modifiers) = event {
            self.modifiers = *modifiers;
        }

        // 模态窗口优先：在时它吞掉全部输入，底层 UI 收不到。
        if let Some(modal) = self.modal.as_mut() {
            modal.event(event);
            return EventResult::Handled;
        }

        if self.shortcut_modifier() && matches!(event, InputEvent::TextInput { .. }) {
            return EventResult::Handled;
        }

        let command = self.shortcut_modifier();
        let shift = self.modifiers.shift;
        match &mut self.screen {
            Screen::Home(home) => home.event(event),
            Screen::Editor => {
                if self.tabs.event(event).is_handled() {
                    return EventResult::Handled;
                }
                if let Some(workspace) = self.workspaces.active_mut() {
                    if history_shortcut(&mut workspace.editor, command, shift, event) {
                        return EventResult::Handled;
                    }
                    return workspace.editor.event(event);
                }
                EventResult::Ignored
            }
        }
    }

    fn update(&mut self, ctx: &FrameContext<'_>) {
        while let Some(action) = self.tabs.take_request() {
            self.handle_tab_action(action);
        }
        if let Some(workspace) = self.workspaces.active_mut() {
            workspace.editor.update();
            if let Some(pixels) = workspace.editor.take_texture_upload() {
                if let Some(backend) = ctx.service::<SharedBackend>().cloned() {
                    upload_texture(&backend, &pixels);
                }
            }
        }

        self.open_modal_if_requested();
        self.sync_modal();
        self.finish_modal(ctx);

        if matches!(self.screen, Screen::Home(_)) {
            self.handle_home_requests(ctx);
        }
        if matches!(self.screen, Screen::Editor) {
            self.handle_workspace_requests(ctx);
            self.tabs
                .set_tabs(&self.workspaces.tabs(), self.workspaces.active_index());
        }

        if self.title_dirty {
            self.title_dirty = false;
            self.apply_title(ctx);
        }
    }

    fn layout(&mut self, ctx: &FrameContext<'_>) {
        let viewport = ctx.viewport();
        match &mut self.screen {
            Screen::Home(home) => home.layout(viewport),
            Screen::Editor => {
                if let Some(workspace) = self.workspaces.active_mut() {
                    workspace.editor.layout(viewport);
                }
                self.tabs.layout(viewport);
            }
        }
        if let Some(modal) = self.modal.as_mut() {
            modal.layout(viewport);
        }
    }

    fn paint(&mut self, _ctx: &FrameContext<'_>, paint: &mut PaintContext) {
        match &self.screen {
            Screen::Home(home) => home.paint(paint),
            Screen::Editor => {
                if let Some(workspace) = self.workspaces.active() {
                    workspace.editor.paint(paint);
                }
                self.tabs.paint(paint);
            }
        }
        if let Some(modal) = self.modal.as_ref() {
            modal.paint(paint);
        }
    }

    /// 编辑器在等工具栏长按阈值时要持续出帧（winit 侧靠它 `request_redraw`）。
    fn needs_frame(&self) -> bool {
        matches!(self.screen, Screen::Editor)
            && self
                .workspaces
                .active()
                .is_some_and(|workspace| workspace.editor.needs_frame())
    }
}

/// Ctrl/Cmd+Z 撤销、Shift+Ctrl/Cmd+Z（或 Ctrl+Y）重做；命中返回 `true`。
fn history_shortcut(
    editor: &mut EditorView,
    command: bool,
    shift: bool,
    event: &InputEvent,
) -> bool {
    if !command {
        return false;
    }
    let InputEvent::KeyDown {
        key: Key::Character(ch),
    } = event
    else {
        return false;
    };
    match ch.to_ascii_lowercase() {
        'z' if shift => {
            editor.redo();
            true
        }
        'z' => {
            editor.undo();
            true
        }
        'y' => {
            editor.redo();
            true
        }
        _ => false,
    }
}

/// 把后端文档纹理上传到 GPU（数据来自 `EditorView` 的合成结果）。
fn upload_texture(backend: &SharedBackend, pixels: &PixelBuffer) {
    if let Err(error) = backend.borrow_mut().update_texture(
        crate::canvas::DOCUMENT_TEXTURE,
        pixels.width,
        pixels.height,
        &pixels.data,
    ) {
        eprintln!("document texture upload failed: {error}");
    }
}

/// 窗口清屏色 = 主题的底层背景（`igui_ui` 之外的地方由后端填）。
fn theme_background(theme: &'static dyn Theme) -> Color {
    theme.surface(SurfaceLevel::Base)
}

/// 注册后端真实字体度量，但用 [`EditorTextMeasurer`] 校正 `ascent`。
///
/// 这是 `igui_winit::TextMeasurePlugin` 的替代品：同样的服务键
///（`Rc<dyn TextMeasurer>`），只是度量实现换成了会做视觉居中的一个。
struct EditorTextMeasurePlugin;

impl Plugin for EditorTextMeasurePlugin {
    fn name(&self) -> &'static str {
        "image-editor-text-measure"
    }

    fn build(&self, app: &mut AppBuilder) {
        app.add_lifecycle_observer(EditorTextMeasureLifecycle);
    }
}

struct EditorTextMeasureLifecycle;

impl LifecycleObserver for EditorTextMeasureLifecycle {
    fn resumed(&mut self, app: &mut App) {
        if app.services().get::<Rc<dyn TextMeasurer>>().is_some() {
            return;
        }
        let Some(backend) = app.services().get::<SharedBackend>().cloned() else {
            return;
        };
        let metrics = backend.borrow().text_metrics();
        let measurer: Rc<dyn TextMeasurer> = Rc::new(EditorTextMeasurer::new(metrics));
        app.services_mut().insert(measurer);
    }
}

/// 后端字体度量 -> 布局引擎的 `TextMeasurer`。
///
/// 布局的居中公式用的是字体原生 `ascent`，而不同字体的
/// `ascent / line_height` 分布差别很大；这里覆盖 `ascent` 为
/// `line_height/2 + cap/2`，让**字形视觉中心**落在行框中心。对度量正常的字体
/// 这个值和原生 `ascent` 基本一致，只有度量异常的字体（随包的中文老宋体）才会
/// 被修正。
struct EditorTextMeasurer {
    metrics: FontMetrics,
}

impl EditorTextMeasurer {
    fn new(metrics: FontMetrics) -> Self {
        Self { metrics }
    }
}

/// 大写字高相对字号的比例（约 0.7 em），用来把字形视觉居中到行框。
const CAP_HEIGHT_RATIO: f32 = 0.7;

impl TextMeasurer for EditorTextMeasurer {
    fn advance(&self, ch: char, font_size: f32) -> f32 {
        self.metrics.advance(ch, font_size)
    }

    fn line_height(&self, font_size: f32) -> f32 {
        self.metrics.line_height(font_size)
    }

    fn ascent(&self, font_size: f32) -> f32 {
        if self.metrics.is_system() {
            self.metrics.line_height(font_size) / 2.0 + font_size * CAP_HEIGHT_RATIO / 2.0
        } else {
            // 点阵字体自带居中（见 `Font::ascent` 的 Bitmap 分支），保持原样。
            self.metrics.ascent(font_size)
        }
    }

    fn measure_run(&self, text: &str, font_size: f32) -> f32 {
        self.metrics.measure_run(text, font_size)
    }
}

/// `--frames N`：跑够 N 帧就把进程结束掉（真实渲染管线自检用）。
struct FrameLimitPlugin {
    frames: u32,
}

impl Plugin for FrameLimitPlugin {
    fn name(&self) -> &'static str {
        "image-editor-frame-limit"
    }

    fn build(&self, app: &mut AppBuilder) {
        app.add_frame_observer(FrameLimiter {
            remaining: self.frames,
        });
    }
}

struct FrameLimiter {
    remaining: u32,
}

impl FrameObserver for FrameLimiter {
    fn after_frame(&mut self, _app: &App, _list: &DrawList) {
        self.remaining = self.remaining.saturating_sub(1);
        if self.remaining == 0 {
            eprintln!("--frames 跑完了，退出");
            // 运行时没有退出钩子；这个标志只用于验证，直接结束进程即可。
            std::process::exit(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_shortcut_requires_a_command_modifier() {
        let mut editor = EditorView::new(crate::theme::editor_theme(false), AppState::default());
        let z = InputEvent::KeyDown {
            key: Key::Character('z'),
        };
        assert!(
            !history_shortcut(&mut editor, false, false, &z),
            "没有 Ctrl/Cmd 时不该当快捷键"
        );
        assert!(
            history_shortcut(&mut editor, true, false, &z),
            "Ctrl/Cmd+Z 应命中撤销"
        );
    }
}
