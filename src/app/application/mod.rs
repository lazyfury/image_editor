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

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use igui_app::{
    App, AppBuilder, AppConfig, AppLogic, EventContext, FrameContext, FrameObserver, InitContext,
    LifecycleObserver, Plugin,
};
use igui_backend_wgpu::{FontConfig, FontMetrics, FontMode, TextureFilter};
use igui_core::{Color, EventResult, InputEvent, Key, Modifiers};
use igui_render::{DrawList, PaintContext};
use igui_theme::{SurfaceLevel, Theme};
use igui_ui::{Clipboard, TextMeasurer};
use igui_winit::{
    ClipboardPlugin, GpuConfig, ImePlugin, KeyboardPlugin, PointerPlugin, SharedBackend,
    SharedWindow, WgpuPlugin, WindowConfig, WinitPlugin,
};

use crate::app::state::AppState;
use crate::app::workspaces::{Workspace, Workspaces};
use crate::document::PixelBuffer;
use crate::io::{DialogOptions, FileDialog, FileFilter, NativeFileDialog};
use crate::ui::{
    ConfirmDialog, ConfirmResult, EditorView, FileRequest, HomeEntry, HomeOpenRequest, HomeView,
    NewDocumentModal, NewDocumentResult, NewDocumentSpec, WorkspaceTabAction, WorkspaceTabs,
    TAB_STRIP_HEIGHT,
};

mod files;
mod plugins;
mod shortcuts;

use plugins::*;
use shortcuts::*;

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
    .plugin(ClipboardPlugin)
    .plugin(crate::app::drop_files::DropFilesPlugin);

    if let Some(frames) = options.frames {
        builder = builder.plugin(FrameLimitPlugin { frames });
    }

    let dialog: Rc<dyn FileDialog> = Rc::new(NativeFileDialog);
    let logic =
        ImageEditorLogic::new(theme, dialog).with_preferences(crate::io::load_preferences());
    builder.logic(logic).build()
}

/// 应用逻辑：主页 / 多工作区编辑器 +「新建文档」模态框，都在这一个 `AppLogic` 里。
struct ImageEditorLogic {
    theme: &'static dyn Theme,
    /// 后端真实字体度量（`init` 时从服务里取；模态 / 工作区 / 标签条共享）。
    measurer: Option<Rc<dyn TextMeasurer>>,
    clipboard: Option<Rc<RefCell<dyn Clipboard>>>,
    screen: Screen,
    /// 打开中的工作区；回主页时也保留，方便再切回来。
    workspaces: Workspaces,
    /// 原生文件对话框（测试注入 `NoopFileDialog`）。
    dialog: Rc<dyn FileDialog>,
    /// 顶部工作区标签条。
    tabs: WorkspaceTabs,
    /// 「新建文档」模态窗口；有它时输入全交给它。
    modal: Option<NewDocumentModal>,
    /// 「保存 / 不保存 / 取消」确认框（关闭脏工作区时）。
    confirm: Option<ConfirmDialog>,
    /// 正在等待确认关闭的工作区下标。
    pending_close: Option<usize>,
    modifiers: Modifiers,
    /// 笔刷偏好（启动读配置、编辑器改了就写回去）。
    preferences: crate::io::BrushPreferences,
    /// 上一次设给后端的文档纹理采样方式（变了才调 `set_texture_filter`）。
    image_smoothing: crate::canvas::ImageSmoothing,
    /// 像素剪贴板：所有工作区共用一块（跨工作区复制粘贴）。
    image_clipboard: Rc<RefCell<Option<crate::tools::PixelClipboard>>>,
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
    fn new(theme: &'static dyn Theme, dialog: Rc<dyn FileDialog>) -> Self {
        let workspaces = Workspaces::new();
        let home = HomeView::new(theme, home_entries(&workspaces));
        Self {
            theme,
            measurer: None,
            clipboard: None,
            screen: Screen::Home(home),
            workspaces,
            dialog,
            tabs: WorkspaceTabs::new(theme),
            modal: None,
            confirm: None,
            pending_close: None,
            modifiers: Modifiers::NONE,
            preferences: crate::io::BrushPreferences::default(),
            image_smoothing: crate::canvas::ImageSmoothing::Linear,
            image_clipboard: Rc::new(RefCell::new(None)),
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

    /// 带上从配置读出来的笔刷偏好（宿主装配时用；测试用默认值）。
    fn with_preferences(mut self, preferences: crate::io::BrushPreferences) -> Self {
        self.preferences = preferences.clamped();
        self
    }

    /// 工作区编辑器统一配置：字体度量 + 系统剪贴板 + 顶部给标签条留位置 + 笔刷偏好。
    fn configure_editor(&self, editor: &mut EditorView) {
        if let Some(measurer) = self.measurer.clone() {
            editor.set_text_measurer(measurer);
        }
        if let Some(clipboard) = self.clipboard.clone() {
            editor.set_clipboard(clipboard);
        }
        editor.set_tab_band(TAB_STRIP_HEIGHT);
        editor.apply_preferences(&self.preferences);
        editor.set_image_clipboard(self.image_clipboard.clone());
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

    /// 处理标签条请求。
    fn handle_tab_action(&mut self, action: WorkspaceTabAction) {
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
}

impl AppLogic for ImageEditorLogic {
    fn init(&mut self, ctx: &InitContext<'_>) {
        // 度量插件已经在 `resumed` 时把度量放进服务。
        self.measurer = ctx.service::<Rc<dyn TextMeasurer>>().cloned();
        self.clipboard = ctx.service::<Rc<RefCell<dyn Clipboard>>>().cloned();
        if let Some(measurer) = self.measurer.clone() {
            self.tabs.set_text_measurer(measurer.clone());
            if let Screen::Home(home) = &mut self.screen {
                home.set_text_measurer(measurer.clone());
            }
            if let Some(workspace) = self.workspaces.active_mut() {
                workspace.editor.set_text_measurer(measurer);
            }
        }
        if let Some(clipboard) = self.clipboard.clone() {
            if let Some(workspace) = self.workspaces.active_mut() {
                workspace.editor.set_clipboard(clipboard);
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
        // 确认框同样模态。
        if let Some(confirm) = self.confirm.as_mut() {
            confirm.event(event);
            return EventResult::Handled;
        }

        if self.shortcut_modifier() && matches!(event, InputEvent::TextInput { .. }) {
            return EventResult::Handled;
        }

        let command = self.shortcut_modifier();
        let shift = self.modifiers.shift;
        // ⌘N 在任何一屏都能新建文档。
        if command && command_key(event) == Some('n') {
            self.open_new_document_modal();
            return EventResult::Handled;
        }
        match &mut self.screen {
            Screen::Home(home) => home.event(event),
            Screen::Editor => {
                if self.tabs.event(event).is_handled() {
                    return EventResult::Handled;
                }
                if let Some(workspace) = self.workspaces.active_mut() {
                    // Shift / Alt 是选区工具的加选 / 减选（视图不认识 winit 事件）。
                    workspace.editor.set_modifiers(self.modifiers);
                    if history_shortcut(&mut workspace.editor, command, shift, event) {
                        return EventResult::Handled;
                    }
                    if selection_shortcut(&mut workspace.editor, command, shift, event) {
                        return EventResult::Handled;
                    }
                    if clipboard_shortcut(&mut workspace.editor, command, event) {
                        return EventResult::Handled;
                    }
                    if let Some(request) = file_shortcut(command, shift, event) {
                        workspace.editor.request_file(request);
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

        self.open_modal_if_requested();
        self.sync_modal();
        self.finish_modal(ctx);
        self.finish_confirm();

        // 以下都可能改文档 / 激活工作区（导入、打开、拖放、画廊切换），所以先处理，
        // 最后统一 `update` + 上传纹理，新内容当帧就能显示。
        if matches!(self.screen, Screen::Home(_)) {
            self.handle_home_requests(ctx);
        }
        if matches!(self.screen, Screen::Editor) {
            self.handle_file_requests(ctx);
        }
        self.handle_dropped_files(ctx);

        if let Some(workspace) = self.workspaces.active_mut() {
            let path = workspace.path.clone();
            workspace.editor.set_workspace_path(path.as_deref());
            workspace.editor.update();
            if let Some(preferences) = workspace.editor.take_preferences() {
                self.preferences = preferences;
                if let Err(error) = crate::io::save_preferences(&preferences) {
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

        if matches!(self.screen, Screen::Editor) {
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
                // 菜单栏在最上：标签条钉在菜单栏下面（编辑器给它留了 `TAB_STRIP_HEIGHT`
                // 的带子）。菜单栏高度由布局算出来，所以每帧同步一次。
                if let Some(workspace) = self.workspaces.active() {
                    if let Some(rect) = workspace.editor.menu_bar_rect() {
                        self.tabs.set_top(rect.bottom());
                    }
                }
                self.tabs.layout(viewport);
            }
        }
        if let Some(modal) = self.modal.as_mut() {
            modal.layout(viewport);
        }
        if let Some(confirm) = self.confirm.as_mut() {
            confirm.layout(viewport);
        }
    }

    fn paint(&mut self, _ctx: &FrameContext<'_>, paint: &mut PaintContext) {
        paint_screen(
            &self.screen,
            &self.workspaces,
            &self.tabs,
            self.modal.as_ref(),
            self.confirm.as_ref(),
            paint,
        );
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

/// 确认框的结果：点了按钮用按钮的；被动关闭（关窗口 / Esc / 点外）= 取消；
/// 什么都没发生返回 `None`（确认框保持打开）。
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

/// 一帧的绘制顺序：**先标签条（窗口 chrome）→ 再内容 → 最后浮层**。
///
/// 标签条必须画在编辑器之前：菜单下拉 / 浮层属于编辑器的 `Overlays`，要盖在
/// 标签条**上面**（菜单栏在最上、标签条在它下面，下拉会跨过那条带子）。
fn paint_screen(
    screen: &Screen,
    workspaces: &Workspaces,
    tabs: &WorkspaceTabs,
    modal: Option<&NewDocumentModal>,
    confirm: Option<&ConfirmDialog>,
    paint: &mut PaintContext,
) {
    match screen {
        Screen::Home(home) => home.paint(paint),
        Screen::Editor => {
            // 标签条是 chrome：先画它，编辑器的浮层（菜单下拉）才能盖在上面。
            tabs.paint(paint);
            if let Some(workspace) = workspaces.active() {
                workspace.editor.paint(paint);
            }
        }
    }
    if let Some(modal) = modal {
        modal.paint(paint);
    }
    if let Some(confirm) = confirm {
        confirm.paint(paint);
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
#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::RefCell;

    /// 脚本化的假对话框：`open` / `save` 各一个返回队列。
    #[derive(Default)]
    struct FakeFileDialog {
        open: RefCell<std::collections::VecDeque<PathBuf>>,
        save: RefCell<std::collections::VecDeque<PathBuf>>,
    }

    impl FakeFileDialog {
        fn saving(path: PathBuf) -> Self {
            let dialog = Self::default();
            dialog.save.borrow_mut().push_back(path);
            dialog
        }
    }

    impl FileDialog for FakeFileDialog {
        fn pick_open(&self, _options: &DialogOptions) -> Option<PathBuf> {
            self.open.borrow_mut().pop_front()
        }

        fn pick_save(&self, _options: &DialogOptions) -> Option<PathBuf> {
            self.save.borrow_mut().pop_front()
        }
    }

    fn logic_with_dialog(dialog: Rc<dyn FileDialog>) -> ImageEditorLogic {
        let theme = crate::theme::editor_theme(false);
        let mut logic = ImageEditorLogic::new(theme, dialog);
        logic
            .workspaces
            .push(Workspace::new(EditorView::new(theme, AppState::default())));
        logic
    }

    /// 标签条是 chrome，必须画在编辑器之前：菜单下拉属于编辑器浮层，要盖住它。
    #[test]
    fn the_tab_strip_is_painted_below_the_editor_overlays() {
        use igui_core::{InputEvent, PointerButton, Size, ViewportSize};
        use igui_render::{DrawCommand, PaintContext};

        let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
        // 从主页切到编辑器（正常流程走「新建文档」，这里直接切屏）。
        logic.screen = Screen::Editor;
        let viewport = ViewportSize::new(Size::new(1280.0, 800.0));
        let tabs_state = vec![crate::ui::WorkspaceTab {
            name: "A".into(),
            dirty: false,
        }];
        let workspace = logic.workspaces.active_mut().expect("一个工作区");
        workspace.editor.set_tab_band(crate::ui::TAB_STRIP_HEIGHT);
        workspace.editor.layout(viewport);
        let tabs_top = workspace.editor.menu_bar_rect().expect("菜单栏").bottom();
        logic.tabs.set_tabs(&tabs_state, 0);
        logic.tabs.set_top(tabs_top);
        logic.tabs.layout(viewport);

        // 打开「文件」菜单（编辑器的浮层），下拉会跨过标签条那条带子。
        let title = workspace.editor.menu_center(0).expect("文件菜单标题");
        for event in [
            InputEvent::PointerDown {
                position: title,
                button: PointerButton::Left,
            },
            InputEvent::PointerUp {
                position: title,
                button: PointerButton::Left,
            },
        ] {
            workspace.editor.event(&event);
        }
        workspace.editor.update();
        workspace.editor.layout(viewport);

        let mut paint = PaintContext::new();
        paint_screen(
            &logic.screen,
            &logic.workspaces,
            &logic.tabs,
            logic.modal.as_ref(),
            logic.confirm.as_ref(),
            &mut paint,
        );
        let texts: Vec<String> = paint
            .draw_list()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        let tab_index = texts
            .iter()
            .position(|text| text == "主页")
            .expect("标签条");
        let item_index = texts
            .iter()
            .position(|text| text.contains("导入 PNG"))
            .expect("文件菜单项");
        assert!(
            tab_index < item_index,
            "标签条要先画，菜单浮层才能盖住它：tabs #{tab_index} / item #{item_index}"
        );
    }

    fn temp_workspace(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "image_editor_ws_{}_{n}_{tag}.igui",
            std::process::id()
        ))
    }

    #[test]
    fn save_as_uses_the_dialog_and_clears_the_dirty_flag() {
        let path = temp_workspace("saveas");
        let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::saving(path.clone())));
        let index = logic.workspaces.active_index();
        assert!(logic.save_workspace_at(index, true), "另存为应写盘");
        assert!(path.exists());
        let workspace = logic.workspaces.get(index).unwrap();
        assert!(!workspace.dirty(), "保存后不脏");
        assert_eq!(workspace.path.as_deref(), Some(path.as_path()));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cancelling_save_as_leaves_the_workspace_unsaved() {
        let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
        let index = logic.workspaces.active_index();
        assert!(!logic.save_workspace_at(index, true), "取消应返回 false");
        assert!(
            logic.workspaces.get(index).unwrap().path.is_none(),
            "取消后仍没有路径"
        );
    }

    #[test]
    fn save_without_a_path_falls_back_to_save_as() {
        let path = temp_workspace("save");
        let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::saving(path.clone())));
        let index = logic.workspaces.active_index();
        assert!(
            logic.save_workspace_at(index, false),
            "没路径时保存应回退到另存为"
        );
        assert!(path.exists());
        let _ = std::fs::remove_file(&path);
    }

    /// 确认框的结果：没点按钮前保持打开，关窗口算取消。
    #[test]
    fn resolve_confirm_keeps_waiting_until_the_user_chooses() {
        assert_eq!(resolve_confirm(None, false), None, "还没选：保持打开");
        assert_eq!(
            resolve_confirm(None, true),
            Some(ConfirmResult::Cancel),
            "关窗口 = 取消"
        );
        assert_eq!(
            resolve_confirm(Some(ConfirmResult::Discard), false),
            Some(ConfirmResult::Discard)
        );
    }

    /// 把第 0 个工作区弄脏（保存 revision 与当前对不上）。
    fn make_dirty(logic: &mut ImageEditorLogic) {
        let workspace = logic.workspaces.get_mut(0).unwrap();
        workspace.saved_revision = workspace.saved_revision.wrapping_add(1);
    }

    #[test]
    fn closing_a_dirty_workspace_opens_a_confirmation_and_waits() {
        let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
        make_dirty(&mut logic);
        logic.request_close(0);
        assert!(logic.confirm.is_some(), "脏工作区关闭应先弹确认");
        logic.finish_confirm();
        assert!(logic.confirm.is_some(), "没点按钮时确认框不该消失");
        assert_eq!(logic.workspaces.tabs().len(), 1, "还没关");
    }

    #[test]
    fn discarding_closes_without_saving() {
        let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
        make_dirty(&mut logic);
        logic.request_close(0);
        logic
            .confirm
            .as_ref()
            .unwrap()
            .choose(ConfirmResult::Discard);
        logic.finish_confirm();
        assert!(logic.confirm.is_none(), "确认框应关闭");
        assert!(logic.workspaces.is_empty(), "应直接丢弃并关闭");
    }

    #[test]
    fn cancelling_keeps_the_workspace() {
        let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
        make_dirty(&mut logic);
        logic.request_close(0);
        logic
            .confirm
            .as_ref()
            .unwrap()
            .choose(ConfirmResult::Cancel);
        logic.finish_confirm();
        assert!(logic.confirm.is_none());
        assert_eq!(logic.workspaces.tabs().len(), 1, "取消后工作区还在");
    }

    #[test]
    fn a_clean_workspace_closes_without_a_confirmation() {
        let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
        logic.request_close(0);
        assert!(logic.confirm.is_none(), "干净工作区不用确认");
        assert!(logic.workspaces.is_empty(), "直接关");
    }

    #[test]
    fn file_shortcuts_map_to_intents() {
        let s = InputEvent::KeyDown {
            key: Key::Character('s'),
        };
        assert_eq!(file_shortcut(true, false, &s), Some(FileRequest::Save));
        assert_eq!(file_shortcut(true, true, &s), Some(FileRequest::SaveAs));
        assert_eq!(
            file_shortcut(false, false, &s),
            None,
            "没有 Ctrl/Cmd 不算快捷键"
        );
        let o = InputEvent::KeyDown {
            key: Key::Character('o'),
        };
        assert_eq!(file_shortcut(true, false, &o), Some(FileRequest::Open));
    }

    /// 不用随包字体时应直接用字体原生 `ascent`（cap 公式会把系统字体往下压）。
    #[test]
    fn a_non_bundled_measurer_keeps_the_native_ascent() {
        use igui_font::{FontConfig, FontMetrics, FontServer};

        let metrics = FontMetrics::new(std::rc::Rc::new(FontServer::load_with(
            FontConfig::default(),
        )));
        let native = metrics.ascent(13.0);
        assert_eq!(
            EditorTextMeasurer::new(metrics, false).ascent(13.0),
            native,
            "系统字体应直接用原生 ascent"
        );
    }

    #[test]
    fn clipboard_shortcuts_cut_copy_and_paste() {
        let mut editor = EditorView::new(crate::theme::editor_theme(false), AppState::default());
        let key = |ch: char| InputEvent::KeyDown {
            key: Key::Character(ch),
        };
        // 没有选区 / 剪贴板时仍是 handled（别把字符漏给输入框），但不改像素。
        assert!(
            !clipboard_shortcut(&mut editor, false, &key('c')),
            "没 Ctrl/Cmd 不算"
        );
        assert!(clipboard_shortcut(&mut editor, true, &key('c')));
        assert!(!editor.has_clipboard());

        // 选区 + 复制 -> 剪贴板有内容，粘贴后能贴回去。
        editor.select_all();
        assert!(clipboard_shortcut(&mut editor, true, &key('c')));
        assert!(editor.has_clipboard());
        assert!(clipboard_shortcut(&mut editor, true, &key('x')), "剪切");
        assert!(clipboard_shortcut(&mut editor, true, &key('v')), "粘贴");
    }

    #[test]
    fn selection_shortcuts_select_all_and_invert() {
        let mut editor = EditorView::new(crate::theme::editor_theme(false), AppState::default());
        let a = InputEvent::KeyDown {
            key: Key::Character('a'),
        };
        assert!(
            !selection_shortcut(&mut editor, false, false, &a),
            "没有 Ctrl/Cmd 时 A 不是全选"
        );
        assert!(selection_shortcut(&mut editor, true, false, &a));
        assert_eq!(
            editor.selection().map(|selection| selection.area()),
            Some(128 * 128)
        );

        let i = InputEvent::KeyDown {
            key: Key::Character('i'),
        };
        assert!(
            !selection_shortcut(&mut editor, true, false, &i),
            "反选要 Shift"
        );
        assert!(selection_shortcut(&mut editor, true, true, &i));
        assert!(editor.selection().is_none(), "全选反选 = 取消选择");
    }

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
