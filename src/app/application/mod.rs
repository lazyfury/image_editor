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
//!
//! 文件分工：`mod.rs` = `Options` / `run` / `build_app` + [`ImageEditorLogic`] 的
//! 字段和 `AppLogic` 实现；`home.rs` = 画廊条目与主页请求；`modal.rs` = 新建文档
//! 模态与关闭确认；`workspace.rs` = 工作区生命周期与每帧同步；`files.rs` = 打开 /
//! 保存 / 导入 / 导出 / 拖放；`shortcuts.rs` = 快捷键映射；`plugins.rs` = 插件与
//! 文本度量；`tests.rs` = 宿主级测试。

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use igui_app::{
    App, AppBuilder, AppConfig, AppLogic, EventContext, FrameContext, FrameObserver, InitContext,
    LifecycleObserver, Plugin,
};
use igui_backend_wgpu::{FontConfig, FontMetrics, FontMode, TextureFilter};
use igui_core::{Color, EventResult, InputEvent, Key, Modifiers};
use igui_render::{DrawList, PaintContext, TextureId};
use igui_theme::{SurfaceLevel, Theme};
use igui_ui::{Clipboard, TextMeasurer};
use igui_winit::{
    ClipboardPlugin, GpuConfig, ImePlugin, KeyboardPlugin, PointerPlugin, SharedBackend,
    SharedWindow, WgpuPlugin, WindowConfig, WinitPlugin,
};

use crate::app::state::AppState;
use crate::app::workspaces::{Workspace, Workspaces};
use crate::document::{Document, PixelBuffer};
use crate::io::{DialogOptions, FileDialog, FileFilter, NativeFileDialog};
use crate::renderer::{CpuRenderer, RenderTarget, Renderer};
use crate::ui::{
    thumbnail_texture, ConfirmDialog, ConfirmResult, EditorView, FileRequest, HomeEntry,
    HomeOpenRequest, HomeThumbnail, HomeView, NewDocumentModal, NewDocumentResult, NewDocumentSpec,
    WorkspaceTabAction, WorkspaceTabs, TAB_STRIP_HEIGHT,
};

mod files;
mod home;
mod modal;
mod plugins;
mod shortcuts;
#[cfg(test)]
mod tests;
mod workspace;

use home::home_entries;
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
    preferences: crate::io::EditorPreferences,
    /// 上一次设给后端的文档纹理采样方式（变了才调 `set_texture_filter`）。
    image_smoothing: crate::canvas::ImageSmoothing,
    /// 像素剪贴板：所有工作区共用一块（跨工作区复制粘贴）。
    image_clipboard: Rc<RefCell<Option<crate::tools::PixelClipboard>>>,
    /// 文档换了标题，下一帧写回原生窗口。
    title_dirty: bool,
    /// 画廊缩略图待上传的像素（每条一张纹理，id 见 `ui::thumbnail_texture`）。
    thumbnail_uploads: Vec<(TextureId, PixelBuffer)>,
}

/// 主窗口当前显示哪一屏。编辑器的工作区在 `ImageEditorLogic::workspaces`。
enum Screen {
    Home(HomeView),
    Editor,
}

impl ImageEditorLogic {
    fn new(theme: &'static dyn Theme, dialog: Rc<dyn FileDialog>) -> Self {
        let workspaces = Workspaces::new();
        let mut thumbnail_uploads = Vec::new();
        let home = HomeView::new(theme, home_entries(&workspaces, &mut thumbnail_uploads));
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
            preferences: crate::io::EditorPreferences::default(),
            image_smoothing: crate::canvas::ImageSmoothing::Linear,
            image_clipboard: Rc::new(RefCell::new(None)),
            title_dirty: false,
            thumbnail_uploads,
        }
    }

    /// Ctrl/Cmd 按下：此时字母键是快捷键，不该再当文本送进输入框。
    fn shortcut_modifier(&self) -> bool {
        self.modifiers.ctrl || self.modifiers.meta
    }

    /// 带上从配置读出来的笔刷偏好（宿主装配时用；测试用默认值）。
    fn with_preferences(mut self, preferences: crate::io::EditorPreferences) -> Self {
        self.preferences = crate::io::EditorPreferences {
            brush: preferences.brush.clamped(),
            record_timeline: preferences.record_timeline,
        };
        self
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
                    if clipboard_shortcut(&mut workspace.editor, command, shift, event) {
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
            self.sync_home_thumbnails(ctx);
            self.handle_home_requests(ctx);
        }
        if matches!(self.screen, Screen::Editor) {
            self.handle_file_requests(ctx);
        }
        self.handle_dropped_files(ctx);

        self.sync_active_workspace(ctx);

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
