//! 窗口宿主：用 `igui_app` 运行时 + `igui_winit` 插件组装**单窗口**应用。
//!
//! 视图（[`crate::ui::HomeView`] / [`crate::ui::EditorView`]）完全不碰平台；
//! 这一层把它们接进 `igui_app` 的插件运行时：
//!
//! ```text
//! winit events -> InputEvent -> AppLogic -> view -> DrawList -> WgpuPresenter -> surface
//! ```
//!
//! **单窗口 + 应用内模态框**：首屏是主页；点「新建项目」不再开第二个原生窗口
//! （macOS 15 上多 surface 会崩），而是在主窗口里打开「新建文档」**模态浮层** ——
//! 宿主画 scrim + 居中面板，输入先给浮层并全部吞掉。创建之后编辑器仍在同一个窗口。
//!
//! `--frames N` 由 [`FrameLimitPlugin`] 在 N 帧后结束进程，让真实渲染管线能在
//! 无头环境里被验证。

use std::rc::Rc;

use igui_app::{
    App, AppBuilder, AppConfig, AppLogic, EventContext, FrameContext, FrameObserver, InitContext,
    LifecycleObserver, Plugin,
};
use igui_backend_wgpu::{FontConfig, FontMetrics, FontMode, TextureFilter};
use igui_core::{
    Color, EventResult, InputEvent, Key, Modifiers, Rect, Size, Transform2D, Vec2, ViewportSize,
};
use igui_render::{DrawList, PaintContext};
use igui_theme::{SurfaceLevel, Theme};
use igui_ui::TextMeasurer;
use igui_winit::{
    ClipboardPlugin, GpuConfig, ImePlugin, KeyboardPlugin, PointerPlugin, SharedBackend,
    SharedWindow, WgpuPlugin, WindowConfig, WinitPlugin,
};

use crate::app::state::AppState;
use crate::document::PixelBuffer;
use crate::ui::{EditorView, HomeView, NewDocumentResult, NewDocumentSpec, NewDocumentView};

/// 主窗口（也是唯一的窗口）的初始大小。
const WINDOW_WIDTH: f64 = 1280.0;
const WINDOW_HEIGHT: f64 = 800.0;

/// 「新建文档」模态面板的逻辑尺寸。
const MODAL_WIDTH: f32 = 360.0;
const MODAL_HEIGHT: f32 = 480.0;

/// 模态后面的遮罩：压暗底层，也表明它不可点。
const SCRIM: Color = Color::new(0.0, 0.0, 0.0, 0.45);

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
    modal: Option<Modal>,
    modifiers: Modifiers,
    /// 文档换了标题，下一帧写回原生窗口。
    title_dirty: bool,
}

/// 主窗口当前显示哪一屏。
enum Screen {
    Home(HomeView),
    Editor(EditorView),
}

/// 「新建文档」模态浮层：一棵独立的视图树 + 它在窗口里的位置。
struct Modal {
    view: NewDocumentView,
    /// 面板左上角在窗口逻辑坐标里的位置（每帧 `layout` 更新）。
    origin: Vec2,
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

    /// 主页点过「新建项目」就打开模态浮层（同时只允许一个）。
    fn open_modal_if_requested(&mut self) {
        if self.modal.is_some() {
            return;
        }
        let requested =
            matches!(&self.screen, Screen::Home(home) if home.take_new_document_request());
        if !requested {
            return;
        }
        let mut view = NewDocumentView::new(self.theme);
        if let Some(measurer) = self.measurer.clone() {
            view.set_text_measurer(measurer);
        }
        self.modal = Some(Modal {
            view,
            origin: Vec2::ZERO,
        });
        tracing::info!(target: "image_editor", "new_document_modal_opened");
    }

    /// 模态浮层的创建 / 取消出结果：创建 -> 换成编辑器；取消 -> 只关掉浮层。
    fn finish_modal(&mut self, ctx: &FrameContext<'_>) {
        let result = self
            .modal
            .as_ref()
            .and_then(|modal| modal.view.take_result());
        let Some(result) = result else {
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
        // `TextMeasurePlugin` 的替代品已经在 `resumed` 时把度量放进服务。
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

        // 模态优先：Esc 取消；其余事件平移到面板坐标后交给浮层，并全部吞掉，
        // 底层 UI 收不到任何输入。
        if self.modal.is_some() {
            if let InputEvent::KeyDown { key: Key::Escape } = event {
                self.modal = None;
                return EventResult::Handled;
            }
            let origin = self
                .modal
                .as_ref()
                .map(|modal| modal.origin)
                .unwrap_or(Vec2::ZERO);
            if let Some(local) = translated_input(event, -origin) {
                if let Some(modal) = self.modal.as_mut() {
                    let _ = modal.view.event(&local);
                }
            }
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
        if let Some(modal) = self.modal.as_mut() {
            modal.view.update();
        }

        self.open_modal_if_requested();
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
            let panel = Size::new(MODAL_WIDTH, MODAL_HEIGHT);
            modal.origin = centered_origin(viewport, panel);
            modal.view.layout(ViewportSize::new(panel));
        }
    }

    fn paint(&mut self, ctx: &FrameContext<'_>, paint: &mut PaintContext) {
        match &self.screen {
            Screen::Home(home) => home.paint(paint),
            Screen::Editor(editor) => editor.paint(paint),
        }
        if let Some(modal) = self.modal.as_ref() {
            let viewport = ctx.viewport().logical_size();
            paint.fill_rect(Rect::from_min_size(Vec2::ZERO, viewport), SCRIM);
            paint.save();
            paint.transform(Transform2D::from_translation(modal.origin));
            modal.view.paint(paint);
            paint.restore();
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

/// 面板在窗口里居中；窗口比面板小时贴左上（不裁到负坐标）。
fn centered_origin(viewport: ViewportSize, panel: Size) -> Vec2 {
    let size = viewport.logical_size();
    Vec2::new(
        ((size.width - panel.width) / 2.0).max(0.0),
        ((size.height - panel.height) / 2.0).max(0.0),
    )
}

/// 把窗口坐标的指针事件平移成模态面板的局部坐标；其它事件原样返回。
fn translated_input(event: &InputEvent, offset: Vec2) -> Option<InputEvent> {
    let at = |position: Vec2| position + offset;
    Some(match event {
        InputEvent::PointerDown { position, button } => InputEvent::PointerDown {
            position: at(*position),
            button: *button,
        },
        InputEvent::PointerUp { position, button } => InputEvent::PointerUp {
            position: at(*position),
            button: *button,
        },
        InputEvent::PointerMove { position } => InputEvent::PointerMove {
            position: at(*position),
        },
        InputEvent::DoubleClick { position } => InputEvent::DoubleClick {
            position: at(*position),
        },
        InputEvent::Wheel { position, delta } => InputEvent::Wheel {
            position: at(*position),
            delta: *delta,
        },
        other => other.clone(),
    })
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
    use igui_core::PointerButton;

    #[test]
    fn the_modal_is_centered_and_clamps_to_the_viewport() {
        let viewport = ViewportSize::new(Size::new(800.0, 600.0));
        let origin = centered_origin(viewport, Size::new(360.0, 480.0));
        assert_eq!(origin, Vec2::new(220.0, 60.0));

        let small = centered_origin(
            ViewportSize::new(Size::new(200.0, 100.0)),
            Size::new(360.0, 480.0),
        );
        assert_eq!(small, Vec2::ZERO);
    }

    #[test]
    fn modal_pointer_events_are_translated_into_panel_space() {
        let event = InputEvent::PointerDown {
            position: Vec2::new(100.0, 100.0),
            button: PointerButton::Left,
        };
        let local = translated_input(&event, Vec2::new(-20.0, -30.0)).expect("pointer event");
        assert_eq!(local.position(), Some(Vec2::new(80.0, 70.0)));
    }
}
