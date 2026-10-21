//! 宿主插件：字体度量、帧数上限。

use super::*;

pub(super) struct EditorTextMeasurePlugin;

impl Plugin for EditorTextMeasurePlugin {
    fn name(&self) -> &'static str {
        "image-editor-text-measure"
    }

    fn build(&self, app: &mut AppBuilder) {
        app.add_lifecycle_observer(EditorTextMeasureLifecycle);
    }
}

pub(super) struct EditorTextMeasureLifecycle;

impl LifecycleObserver for EditorTextMeasureLifecycle {
    fn resumed(&mut self, app: &mut App) {
        if app.services().get::<Rc<dyn TextMeasurer>>().is_some() {
            return;
        }
        let Some(backend) = app.services().get::<SharedBackend>().cloned() else {
            return;
        };
        let metrics = backend.borrow().text_metrics();
        // 是否在用随包字体：它字形在 em 框里偏下，需要 cap 修正；系统字体
        // （PingFang SC / SF Pro）用原生 `ascent` 就正好居中。
        let bundled = crate::fonts::bundled_face().is_some();
        let measurer: Rc<dyn TextMeasurer> = Rc::new(EditorTextMeasurer::new(metrics, bundled));
        app.services_mut().insert(measurer);
    }
}

/// 后端字体度量 -> 布局引擎的 `TextMeasurer`。
///
/// 布局把文字基线放在 `行框顶 + ascent`；改 `ascent` 就是改文字的**垂直位置**：
///
/// - **随包的中文老宋体**：字形在 em 框里偏下，用原生 `ascent` 会看着偏高，
///   所以覆盖为 `line_height/2 + cap/2`（把视觉中心压到行框中心）。
/// - **系统字体**（PingFang SC / SF Pro）：原生 `ascent` 已经把 em 框居中，
///   再套 cap 公式反而会往下偏 ~1px，所以直接用原生值。
/// - **点阵字体**：`ascent` 自带居中（见 `Font::ascent` 的 Bitmap 分支）。
///
/// 换句话说：cap 修正是为随包字体专门调的，不是通用公式。
pub(super) struct EditorTextMeasurer {
    metrics: FontMetrics,
    /// 当前用的是随包字体（而不是系统字体）。
    bundled: bool,
}

impl EditorTextMeasurer {
    pub(super) fn new(metrics: FontMetrics, bundled: bool) -> Self {
        Self { metrics, bundled }
    }
}

/// 大写字高相对字号的比例（约 0.7 em），用来把随包字体的字形视觉居中到行框。
const CAP_HEIGHT_RATIO: f32 = 0.7;

impl TextMeasurer for EditorTextMeasurer {
    fn advance(&self, ch: char, font_size: f32) -> f32 {
        self.metrics.advance(ch, font_size)
    }

    fn line_height(&self, font_size: f32) -> f32 {
        self.metrics.line_height(font_size)
    }

    fn ascent(&self, font_size: f32) -> f32 {
        if self.bundled && self.metrics.is_system() {
            self.metrics.line_height(font_size) / 2.0 + font_size * CAP_HEIGHT_RATIO / 2.0
        } else {
            self.metrics.ascent(font_size)
        }
    }

    fn measure_run(&self, text: &str, font_size: f32) -> f32 {
        self.metrics.measure_run(text, font_size)
    }
}

/// `--frames N`：跑够 N 帧就把进程结束掉（真实渲染管线自检用）。
pub(super) struct FrameLimitPlugin {
    pub(super) frames: u32,
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

pub(super) struct FrameLimiter {
    remaining: u32,
}

impl FrameObserver for FrameLimiter {
    fn after_frame(&mut self, app: &App, _list: &DrawList) {
        self.remaining = self.remaining.saturating_sub(1);
        if self.remaining == 0 {
            eprintln!("--frames 跑完了，退出");
            // 运行时没有退出钩子；这个标志只用于验证，直接结束进程即可。
            std::process::exit(0);
        }
        // `igui_winit` 是 `ControlFlow::Wait`：第一帧之后没人要帧就会停住，等窗口
        // 碰巧来个事件才有下一帧。`--frames N` 要的是**确定性**的 N 帧，所以这里
        // 主动要下一帧（和原生菜单的动作入队后叫醒事件循环同一个道理）。
        if let Some(window) = app.services().get::<SharedWindow>() {
            if let Some(window) = window.borrow().as_ref() {
                window.request_redraw();
            }
        }
    }
}
