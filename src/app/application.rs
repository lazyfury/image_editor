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
use crate::document::PixelBuffer;
use crate::ui::{EditorView, HomeView, NewDocumentModal, NewDocumentResult, NewDocumentSpec};

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

/// 应用逻辑：主页 / 编辑器 +「新建文档」模态框，都在这一个 `AppLogic` 里。
struct ImageEditorLogic {
    theme: &'static dyn Theme,
    /// 后端真实字体度量（`init` 时从服务里取；模态 / 新编辑器共享）。
    measurer: Option<Rc<dyn TextMeasurer>>,
    screen: Screen,
    /// 「新建文档」模态窗口；有它时输入全交给它（编辑器自己的菜单是另一套）。
    modal: Option<NewDocumentModal>,
    modifiers: Modifiers,
    /// 文档换了标题，下一帧写回原生窗口。
    title_dirty: bool,
}

/// 主窗口当前显示哪一屏。
enum Screen {
    Home(HomeView),
    Editor(EditorView),
}

impl ImageEditorLogic {
    fn new(theme: &'static dyn Theme) -> Self {
        Self {
            theme,
            measurer: None,
            screen: Screen::Home(HomeView::new(theme)),
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
        if self.modal.is_some() {
            return;
        }
        let requested =
            matches!(&self.screen, Screen::Home(home) if home.take_new_document_request());
        if !requested {
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

    /// 把「新建文档」的选择变成同窗口里的一个新 [`EditorView`]。
    fn open_editor(&mut self, ctx: &FrameContext<'_>, spec: NewDocumentSpec) {
        let state = AppState::with_document(spec.width, spec.height, spec.background);
        let mut editor = EditorView::new(self.theme, state);
        if let Some(measurer) = self.measurer.clone() {
            editor.set_text_measurer(measurer);
        }

        // 文档纹理放大时用最近邻：硬边像素而不是模糊插值。首帧之前上传，
        // 否则视图画的 `Visual::Image(DOCUMENT_TEXTURE)` 会先退回白色兜底。
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
        self.screen = Screen::Editor(editor);
        self.title_dirty = true;
    }

    /// 文档换了就把「名字 — igui 图像编辑器」写回原生窗口标题。
    fn apply_title(&self, ctx: &FrameContext<'_>) {
        let title = match &self.screen {
            Screen::Editor(editor) => format!("{} — igui 图像编辑器", editor.document_name()),
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
            match &mut self.screen {
                Screen::Home(home) => home.set_text_measurer(measurer),
                Screen::Editor(editor) => editor.set_text_measurer(measurer),
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
            Screen::Editor(editor) => {
                if history_shortcut(editor, command, shift, event) {
                    return EventResult::Handled;
                }
                editor.event(event)
            }
        }
    }

    fn update(&mut self, ctx: &FrameContext<'_>) {
        if let Screen::Editor(editor) = &mut self.screen {
            editor.update();
            if let Some(pixels) = editor.take_texture_upload() {
                if let Some(backend) = ctx.service::<SharedBackend>().cloned() {
                    upload_texture(&backend, &pixels);
                }
            }
        }

        self.open_modal_if_requested();
        self.sync_modal();
        self.finish_modal(ctx);

        if self.title_dirty {
            self.title_dirty = false;
            self.apply_title(ctx);
        }
    }

    fn layout(&mut self, ctx: &FrameContext<'_>) {
        let viewport = ctx.viewport();
        match &mut self.screen {
            Screen::Home(home) => home.layout(viewport),
            Screen::Editor(editor) => editor.layout(viewport),
        }
        if let Some(modal) = self.modal.as_mut() {
            modal.layout(viewport);
        }
    }

    fn paint(&mut self, _ctx: &FrameContext<'_>, paint: &mut PaintContext) {
        match &self.screen {
            Screen::Home(home) => home.paint(paint),
            Screen::Editor(editor) => editor.paint(paint),
        }
        if let Some(modal) = self.modal.as_ref() {
            modal.paint(paint);
        }
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
