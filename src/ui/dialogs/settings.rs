//! 「属性」对话框：**完整的工具属性**。
//!
//! 选项栏只放得下一行的紧凑子集；这里把笔刷 / 笔尖 / 油漆桶 / 记录四组属性排成
//! 竖列，每行「标签 + − 值 +」或一个开关。控件和选项栏共用：
//!
//! - 开关（像素 / 方形 / 对称 / 连续）读同一批 [`OptionCells`]，两处永远一致；
//! - `−` / `+` 塞同一批 [`Intent::Brush`](crate::ui::Intent::Brush) /
//!   [`Intent::Bucket`](crate::ui::Intent::Bucket)，由 `EditorView::update` 应用；
//! - 数值文本由宿主每帧用 [`SettingsSnapshot`] 刷（[`SettingsWindow::sync`]），
//!   窗口自己不存状态。
//!
//! 「预设」一行显示当前笔刷命中的预设名（不命中就是「自定义」）。

use std::cell::Cell;
use std::rc::Rc;

use igui_components::{Button, Component, Flex, NodeRef, Text};
use igui_core::{Color, Edges, EventResult, InputEvent, Vec2, ViewportSize};
use igui_render::PaintContext;
use igui_scene::SceneTree;
use igui_theme::{radius, space, Theme, Tone};
use igui_ui::{Align, InteractState, MouseFilter, SizeBasis, SurfaceStyle, TextMeasurer};

use crate::tools::Symmetry;
use crate::ui::options_bar::{BrushAdjust, BucketAdjust};
use crate::ui::{Intent, Intents};

use super::super::window::{Window, FOOTER_BUTTON_MIN_WIDTH};
use super::OptionCells;

/// 窗口尺寸 / 最小尺寸。
const WINDOW_SIZE: Vec2 = Vec2::new(500.0, 600.0);
const WINDOW_MIN: Vec2 = Vec2::new(380.0, 360.0);
/// 标签列宽度：值 / 按钮从同一列开始，行与行对齐。
const LABEL_WIDTH: f32 = 84.0;
/// 数值列宽度：`−` 和 `+` 之间固定宽，数字变了按钮不左右跳。
const VALUE_WIDTH: f32 = 52.0;
/// 一行的高度。
const ROW_HEIGHT: f32 = 26.0;

/// 窗口每帧要显示的一组值（`EditorView::settings_snapshot` 给）。
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsSnapshot {
    /// 当前笔刷命中的预设名（不命中是「自定义」）。
    pub preset: String,
    pub size: f32,
    pub opacity: f32,
    pub hardness: f32,
    pub smooth: f32,
    pub spacing: f32,
    pub tolerance: u8,
    pub symmetry: Symmetry,
    /// 「记录整段编辑过程」开着没有。
    pub record: bool,
}

impl Default for SettingsSnapshot {
    fn default() -> Self {
        Self {
            preset: "自定义".to_string(),
            size: 1.0,
            opacity: 1.0,
            hardness: 1.0,
            smooth: 0.5,
            spacing: 0.1,
            tolerance: 32,
            symmetry: Symmetry::Off,
            record: false,
        }
    }
}

/// 窗口里要回写的文本槽。
#[derive(Default)]
struct Refs {
    preset: NodeRef,
    size: NodeRef,
    hardness: NodeRef,
    smooth: NodeRef,
    spacing: NodeRef,
    opacity: NodeRef,
    tolerance: NodeRef,
    symmetry: NodeRef,
    record: NodeRef,
}

/// 「属性」窗口（完整属性）。
pub struct SettingsWindow {
    window: Window,
    refs: Refs,
    /// 页脚「关闭」按下的标记（窗口自己的关闭请求由 `Window` 管）。
    closed: Rc<Cell<bool>>,
    /// 「记录编辑过程」开关的显示值；点一下就翻，下一帧再和快照对齐。
    record: Rc<Cell<bool>>,
    /// 上一次同步的快照：没变就不刷文本（`set_text` 会标脏）。
    shown: Option<SettingsSnapshot>,
}

impl SettingsWindow {
    /// 建窗口。`cells` 与选项栏是同一批开关，`intents` 与选项栏是同一个队列。
    pub fn new(theme: &'static dyn Theme, cells: OptionCells, intents: Intents) -> Self {
        let mut refs = Refs::default();
        let closed: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        let record: Rc<Cell<bool>> = Rc::new(Cell::new(false));

        let content = {
            let pr = &mut refs;
            Flex::column()
                .gap(space::XS)
                .padding(Edges::ZERO)
                .mouse_filter(MouseFilter::Ignore)
                .child(section(theme, "笔刷"))
                .child(label_row(theme, "预设").child(Text::small("", theme).ref_(&pr.preset)))
                .child(stepper_row(
                    theme,
                    "大小",
                    intents.clone(),
                    BrushAdjust::SizeDown,
                    BrushAdjust::SizeUp,
                    &pr.size,
                ))
                .child(stepper_row(
                    theme,
                    "硬度",
                    intents.clone(),
                    BrushAdjust::HardnessDown,
                    BrushAdjust::HardnessUp,
                    &pr.hardness,
                ))
                .child(stepper_row(
                    theme,
                    "平滑",
                    intents.clone(),
                    BrushAdjust::SmoothDown,
                    BrushAdjust::SmoothUp,
                    &pr.smooth,
                ))
                .child(stepper_row(
                    theme,
                    "间距",
                    intents.clone(),
                    BrushAdjust::SpacingDown,
                    BrushAdjust::SpacingUp,
                    &pr.spacing,
                ))
                .child(stepper_row(
                    theme,
                    "不透明度",
                    intents.clone(),
                    BrushAdjust::OpacityDown,
                    BrushAdjust::OpacityUp,
                    &pr.opacity,
                ))
                .child(section(theme, "笔尖"))
                .child(label_row(theme, "像素模式").child(toggle(
                    theme,
                    "硬边",
                    cells.pixel.clone(),
                )))
                .child(label_row(theme, "笔尖形状").child(toggle(
                    theme,
                    "方形",
                    cells.square.clone(),
                )))
                .child(symmetry_row(theme, &cells, &pr.symmetry))
                .child(section(theme, "油漆桶 / 魔棒"))
                .child(bucket_row(theme, intents.clone(), &pr.tolerance))
                .child(label_row(theme, "连续").child(toggle(
                    theme,
                    "只填连通区域",
                    cells.contiguous.clone(),
                )))
                .child(section(theme, "记录"))
                .child(record_row(theme, &record, &pr.record, intents.clone()))
                .child(
                    Text::caption("改动立即生效，并写进配置目录的 preferences。", theme)
                        .tone(Tone::Muted),
                )
        };

        let footer_close = closed.clone();
        let window = Window::new(theme, "属性", WINDOW_SIZE, WINDOW_MIN, {
            move |tree: &mut SceneTree, content_node, footer| {
                content.build(tree, content_node);
                Button::secondary("关闭", theme)
                    .min_size(FOOTER_BUTTON_MIN_WIDTH, 0.0)
                    .on_click(move |_tree, _id| footer_close.set(true))
                    .build(tree, footer);
            }
        });

        Self {
            window,
            refs,
            closed,
            record,
            shown: None,
        }
    }

    /// 用后端真实字体度量排版。
    pub fn set_text_measurer(&mut self, measurer: Rc<dyn TextMeasurer>) {
        self.window.set_text_measurer(measurer);
    }

    /// 把最新的属性值写进标签（值没变就跳过）。
    pub fn sync(&mut self, snapshot: &SettingsSnapshot) {
        if self.shown.as_ref() == Some(snapshot) {
            return;
        }
        self.shown = Some(snapshot.clone());
        self.record.set(snapshot.record);
        let texts = [
            (&self.refs.preset, snapshot.preset.clone()),
            (&self.refs.size, format!("{:.0} px", snapshot.size)),
            (&self.refs.hardness, percent(snapshot.hardness)),
            (&self.refs.smooth, percent(snapshot.smooth)),
            (&self.refs.spacing, percent(snapshot.spacing)),
            (&self.refs.opacity, percent(snapshot.opacity)),
            (&self.refs.tolerance, format!("{}", snapshot.tolerance)),
            (&self.refs.symmetry, snapshot.symmetry.label().to_string()),
            (&self.refs.record, on_off(snapshot.record).to_string()),
        ];
        for (slot, text) in texts {
            if let Some(id) = slot.get() {
                self.window.set_text(id, text);
            }
        }
    }

    pub fn event(&mut self, event: &InputEvent) -> EventResult {
        self.window.event(event)
    }

    pub fn layout(&mut self, viewport: ViewportSize) {
        self.window.layout(viewport);
    }

    pub fn paint(&self, ctx: &mut PaintContext) {
        self.window.paint(ctx);
    }

    /// 关闭请求（Esc / 关闭按钮 / 点窗口外 / 页脚「关闭」）。
    pub fn take_close(&self) -> bool {
        self.closed.replace(false) || self.window.take_close_request()
    }
}

fn percent(value: f32) -> String {
    format!("{:.0}%", value * 100.0)
}

fn on_off(value: bool) -> &'static str {
    if value {
        "开"
    } else {
        "关"
    }
}

/// 小节标题。
fn section(theme: &'static dyn Theme, title: &str) -> impl Component {
    Text::small(title, theme)
        .color(theme.palette().foreground)
        .mouse_filter(MouseFilter::Ignore)
}

/// 一行 `标签（固定宽度，弱化） + 后面的控件`。
fn label_row(theme: &'static dyn Theme, label: &str) -> Flex {
    Flex::row()
        .align(Align::Center)
        .gap(space::SM)
        .padding(Edges::ZERO)
        .basis(SizeBasis::Px(ROW_HEIGHT))
        .mouse_filter(MouseFilter::Ignore)
        .child(
            Text::caption(label, theme)
                .tone(Tone::Muted)
                .min_size(LABEL_WIDTH, 0.0)
                .mouse_filter(MouseFilter::Ignore),
        )
}

/// 一行 `标签 + − 值 +`。
fn stepper_row(
    theme: &'static dyn Theme,
    label: &str,
    intents: Intents,
    down: BrushAdjust,
    up: BrushAdjust,
    slot: &NodeRef,
) -> Flex {
    let down_intents = intents.clone();
    let up_intents = intents;
    label_row(theme, label)
        .child(
            Button::ghost("−", theme)
                .on_click(move |_tree, _id| down_intents.push(Intent::Brush(down))),
        )
        .child(Text::small("", theme).min_size(VALUE_WIDTH, 0.0).ref_(slot))
        .child(
            Button::ghost("+", theme)
                .on_click(move |_tree, _id| up_intents.push(Intent::Brush(up))),
        )
}

/// 油漆桶 / 魔棒的容差（和选项栏同一个 `BucketAdjust`）。
fn bucket_row(theme: &'static dyn Theme, intents: Intents, slot: &NodeRef) -> Flex {
    let down_intents = intents.clone();
    let up_intents = intents;
    label_row(theme, "容差")
        .child(Button::ghost("−", theme).on_click(move |_tree, _id| {
            down_intents.push(Intent::Bucket(BucketAdjust::ToleranceDown))
        }))
        .child(Text::small("", theme).min_size(VALUE_WIDTH, 0.0).ref_(slot))
        .child(
            Button::ghost("+", theme).on_click(move |_tree, _id| {
                up_intents.push(Intent::Bucket(BucketAdjust::ToleranceUp))
            }),
        )
}

/// 对称：一个「切换」按钮 + 当前模式。
fn symmetry_row(theme: &'static dyn Theme, cells: &OptionCells, slot: &NodeRef) -> Flex {
    let symmetry = cells.symmetry.clone();
    label_row(theme, "对称")
        .child(
            Button::ghost("切换", theme)
                .on_click(move |_tree, _id| symmetry.set(symmetry.get().next())),
        )
        .child(Text::small("", theme).ref_(slot))
}

/// 一行：`记录编辑过程`开关 + 开 / 关。
fn record_row(
    theme: &'static dyn Theme,
    record: &Rc<Cell<bool>>,
    slot: &NodeRef,
    intents: Intents,
) -> Flex {
    let click = record.clone();
    label_row(theme, "编辑过程")
        .child(
            Button::ghost("记录（延时动画）", theme).on_click(move |_tree, _id| {
                let next = !click.get();
                click.set(next);
                intents.push(Intent::RecordTimeline(next));
            }),
        )
        .child(Text::small("", theme).ref_(slot))
}

/// 选项栏同款的布尔开关：底色读共享格，点了直接翻。
fn toggle(theme: &'static dyn Theme, label: &str, state: Rc<Cell<bool>>) -> impl Component {
    let click = state.clone();
    let paint = state.clone();
    Button::ghost(label, theme)
        .on_click(move |_tree, _id| click.set(!click.get()))
        .dynamic_background(move |interact: InteractState| {
            active_style(theme, paint.get(), interact)
        })
}

fn active_style(theme: &'static dyn Theme, active: bool, interact: InteractState) -> SurfaceStyle {
    if active {
        SurfaceStyle::new(theme.palette().selection).radius(radius::SM)
    } else if interact.hovered || interact.pressed {
        SurfaceStyle::new(theme.palette().surface_hover).radius(radius::SM)
    } else {
        SurfaceStyle::new(Color::TRANSPARENT)
    }
}
