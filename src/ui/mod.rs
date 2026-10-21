//! 编辑器视图：菜单栏 + 工具栏 + 画布 + 图层/属性栏 + 状态栏。
//!
//! 视图是一棵 `SceneTree`：UI 由 `igui_components` 搭、`igui_ui` 排 / 画；
//! 画布（文档图像 + 像素网格 + 选区）由 [`EditorView::paint`] 在画布区域的裁剪
//! 矩形里手动绘制。单树模型下 `igui_ui::paint` 也会画世界 canvas item，所以文档
//! 不能再挂成场景 `Node2D`（那会被画到所有 UI 之上）。宿主
//! （[`crate::app::application`]）拥有窗口、wgpu 后端和纹理上传。
//!
//! ```text
//! Input -> EditorView::event -> EditorView::update -> layout -> paint
//! paint:  clip(canvas_area){ 文档图像 -> 像素网格 -> 选区 }  ->  igui_ui::paint(UI)
//! ```
//!
//! 每块面板一个模块（[`menu`] / [`toolbar`] / [`canvas`] / [`layer_panel`] /
//! [`properties_panel`] / [`status_bar`]），这个文件负责把它们拼成整页、
//! 持有共享状态，并把 `CanvasCamera` 同步到文档节点的变换。

mod accessors;
mod build;
mod canvas;
mod card;
mod clipboard;
mod confirm;
mod file_panel;
mod gradient_editor;
mod history_panel;
mod home;
mod input;
mod intents;
mod io;
mod layer_panel;
mod layout;
pub mod menu;
mod menus;
mod new_document;
mod options_bar;
mod paint;
mod palette;
mod pen;
mod preferences;
mod properties_panel;
mod rename;
mod selection;
mod status_bar;
mod sync;
mod tabs;
mod toolbar;
mod tools;
mod window;
mod workspace_tabs;

pub use confirm::{ConfirmDialog, ConfirmResult};
pub use home::{thumbnail_texture, HomeEntry, HomeOpenRequest, HomeThumbnail, HomeView};
pub use intents::{Intent, Intents};
pub use new_document::{NewDocumentModal, NewDocumentResult, NewDocumentSpec};
pub use options_bar::BrushAdjust;
pub use options_bar::BrushToggle;
pub use options_bar::GradientAdjust;
pub use options_bar::{BucketAdjust, BucketToggle, PenAction};
pub use tabs::SidebarTab;

/// 文件面板 / 快捷键留下的文件意图；宿主取走后弹原生对话框并执行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileRequest {
    /// ⌘S：有路径直接写，没有就走另存为。
    Save,
    /// ⌘⇧S：总是弹保存对话框。
    SaveAs,
    /// ⌘O：弹打开对话框（`.igui`）。
    Open,
    /// 弹打开对话框（`.png`）→ 新图层。
    ImportPng,
    /// ⌘E：弹保存对话框（`.png`）。
    ExportPng,
}

impl FileRequest {
    /// 按钮 / 菜单标签。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Save => "保存",
            Self::SaveAs => "另存为…",
            Self::Open => "打开…",
            Self::ImportPng => "导入 PNG…",
            Self::ExportPng => "导出 PNG…",
        }
    }
}
pub use workspace_tabs::{WorkspaceTab, WorkspaceTabAction, WorkspaceTabs, TAB_STRIP_HEIGHT};

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use igui_components::{set_text, update_control, ListState, NodeRef, Overlays};
use igui_core::{Key, NodeId, PointerButton, Rect, Size, Vec2, ViewportSize};
use igui_scene::SceneTree;
use igui_theme::{space, Theme};
use igui_ui::{SizeBasis, TextMeasurer};

use crate::app::state::{ActiveTool, AppState, HistoryAction};
use crate::app::tool_groups::{group_index, TOOL_GROUPS};
use crate::canvas::{
    document_to_pixel, paint_backdrop, paint_backdrop_in, screen_to_document, CanvasCamera,
};
use crate::document::{CropLayerCommand, LayerId, LayerMetaCommand};
use crate::icons::IconSet;
use crate::renderer::{CpuRenderer, RenderTarget, Renderer};
use crate::tools::{
    BrushShape, BrushTool, MoveTool, PaintBucketTool, PenTool, PointerEvent, RectangleFillTool,
};
use crate::ui::options_bar::{options_hint, tool_has_brush};

/// 工具栏宽度（逻辑像素）。
const TOOLBAR_WIDTH: f32 = 52.0;
/// 工具栏长按展开的阈值（毫秒）。
const LONG_PRESS_MS: u64 = 350;
/// 长按期间允许的指针移动容差（逻辑像素）；超过就当拖动、不展开。
pub const LONG_PRESS_SLOP: f32 = 6.0;
/// 左侧调色盘面板的宽度（可拖）。
const PALETTE_WIDTH: f32 = 172.0;
const PALETTE_MIN: f32 = 156.0;
const PALETTE_MAX: f32 = 250.0;
/// 右侧栏默认 / 最小宽度（逻辑像素）。
const SIDEBAR_WIDTH: f32 = 280.0;
const SIDEBAR_MIN: f32 = 200.0;
/// 画布区域的最小宽度：右栏拖动到再宽也不能把画布挤没。
const CANVAS_MIN: f32 = 160.0;
/// 分隔条的把手宽度（`ResizeHandle` 的默认值）。
const RESIZE_GUTTER: f32 = 6.0;
/// 适配时画布四周留的空白。
const FIT_PADDING: f32 = 24.0;
/// 一个滚轮刻度 / `+`/`-` 的缩放倍率。
const ZOOM_STEP: f32 = 1.25;
/// 「羽化」菜单的半径（文档像素）。以后接半径输入框就把它换掉。
const FEATHER_RADIUS: f32 = 8.0;
/// 笔刷光标圈的最小屏幕半径（1px 笔缩小时也看得见）与中心十字的半长。
const MIN_CURSOR_RADIUS: f32 = 2.0;
const CROSSHAIR_ARM: f32 = 4.0;
/// 钢笔锚点命中 / 「点回起点闭合」的屏幕半径（逻辑像素）。
const PEN_HIT_RADIUS: f32 = 8.0;
/// 钢笔预览的展平容差（屏幕像素，折成文档单位要给 `zoom` 除一下）。
const PEN_PREVIEW_TOLERANCE: f32 = 0.25;
/// 路径预览上锚点方块的边长、控制柄端点半径（逻辑像素）。
const ANCHOR_SIZE: f32 = 5.0;
const HANDLE_DOT: f32 = 2.5;
/// 像素模式下，出现像素网格的最小缩放。
const GRID_MIN_ZOOM: f32 = 6.0;
/// 右侧栏「文件 / 历史 / 属性」标签页面板的默认高度 / 拖拽钳制（逻辑像素）。
const TABS_PANEL_HEIGHT: f32 = 220.0;
const TABS_PANEL_MIN: f32 = 120.0;
const TABS_PANEL_MAX: f32 = 480.0;
/// 「图层」面板保留的最小高度（上面的分隔条不能把它挤没）。
const LAYER_PANEL_MIN: f32 = 120.0;

/// 需要在构建后回写的节点槽位。
#[derive(Default)]
struct Refs {
    /// 菜单栏本身 / 标签条预留行（宿主把工作区标签条画在预留行里）。
    menu_bar: NodeRef,
    tab_band: NodeRef,
    tool: NodeRef,
    zoom: NodeRef,
    message: NodeRef,
    canvas: NodeRef,
    props_name: NodeRef,
    props_detail: NodeRef,
    props_geometry: NodeRef,
    tabs_panel: NodeRef,
    tabs_handle: NodeRef,
    palette_panel: NodeRef,
    palette_picker: NodeRef,
    palette_handle: NodeRef,
    path: NodeRef,
    sidebar: NodeRef,
    sidebar_handle: NodeRef,
}

/// 正在进行的图层重命名：目标图层 + 浮层 + 输入框句柄 + 确定 / 取消格。
struct Rename {
    id: LayerId,
    overlay: igui_components::OverlayId,
    edit: rename::EditSlot,
    confirm: Rc<Cell<bool>>,
    cancel: Rc<Cell<bool>>,
}

/// 状态栏同步快照：(工具, 缩放, 宽, 高, 指针下的文档像素)。
type StatusSnapshot = (ActiveTool, f32, u32, u32, Option<(u32, u32)>);

/// 编辑器视图。
pub struct EditorView {
    tree: SceneTree,
    theme: &'static dyn Theme,
    /// 应用状态。回调只往这里写，[`EditorView::update`] 负责同步到控件。
    state: Rc<RefCell<AppState>>,
    /// 菜单 / 占位按钮留下的提示（回调拿不到 `&mut self`，用共享格转交）。
    message: Rc<RefCell<Option<String>>>,
    tool_label: NodeId,
    zoom_label: NodeId,
    message_label: NodeId,
    /// 分组工具按钮 / 标签节点（下标对应 `TOOL_GROUPS`）。
    tool_group_nodes: Vec<NodeId>,
    tool_face_labels: Vec<NodeId>,
    /// 每个组的「脸」工具（长按展开 / 快捷键都会更新它）。
    tool_faces: Vec<Rc<Cell<ActiveTool>>>,
    /// 工具栏长按状态（组按钮写、`update` 轮询）。
    long_press: Rc<RefCell<toolbar::LongPress>>,
    /// 当前展开的工具浮层属于哪个组；没展开为 `None`。
    open_tool_group: Cell<Option<usize>>,
    /// 长按阈值（测试可调，默认 [`LONG_PRESS_MS`]）。
    long_press_threshold: Duration,
    /// 页面（根 Flex 的子节点，顶部预留靠它的 `offsets` 实现）。
    /// 菜单栏节点（宿主用它把工作区标签条钉在菜单下面）。
    menu_bar: NodeId,
    /// 标签条预留行（菜单栏和选项栏之间；宿主画标签条的那一条）。
    tab_band: NodeId,
    /// 图标包（长按浮层打开时还要用它画行图标）。
    icons: Rc<IconSet>,
    /// 撤销 / 重做按钮节点（Phase 6）。
    history_nodes: Vec<(HistoryAction, NodeId)>,
    palette_nodes: Vec<NodeId>,
    /// 文件面板的按钮节点（Phase 7）。
    file_nodes: Vec<(FileRequest, NodeId)>,
    /// 回调留下的意图（工具 / 菜单 / 选项栏 / 文件……）；`update` 每帧取走视图
    /// 能处理的部分，[`Intent::File`] 留给宿主。
    intents: Intents,
    /// 当前工作区文件路径（只读展示，宿主每帧喂）。
    workspace_path: Rc<RefCell<Option<std::path::PathBuf>>>,
    /// 菜单标题的节点（下拉的锚点），顺序与 [`menu::MENUS`] 一致。
    menu_nodes: Vec<NodeId>,
    /// 当前打开菜单的下标（用于点同一标题时切换成关闭）。
    open_menu: Cell<Option<usize>>,
    /// 覆盖层：承载菜单下拉，画在 UI 之上并优先接收输入。
    overlays: Overlays,
    /// 选项栏的句柄 / 按钮表 / 显示源共享格（见 `options_bar::OptionsState`）。
    options: options_bar::OptionsState,
    /// 上一次「已落盘」的偏好快照：变了才让宿主写配置（见 `take_preferences`）。
    saved_preferences: crate::io::BrushPreferences,
    /// 油漆桶工具（颜色 / 容差 / 连续 / 选区在落笔前同步）。
    paint_bucket: PaintBucketTool,
    /// 矩形填充工具（拖动中暴露预览角点）。
    rect_fill: RectangleFillTool,
    /// 渐变工具的视图侧状态（色带 / 类型 / 反向 / 编辑器开关 + 选项栏节点）。
    gradient: gradient_editor::GradientState,
    /// 可拖动的面板尺寸 + 侧栏标签页状态（[`layout::Layout`]）。
    ///
    /// 和 `EditorView::layout`（排树）不是一回事：这里是「拖成多宽 / 多高、现在
    /// 停在哪一页」的状态，见 `ui/layout.rs`。
    layout: layout::Layout,
    /// 路径标签节点，以及上一次写入的内容（避免每帧刷文本）。
    path_label: NodeId,
    shown_path: Option<String>,
    /// 图标包里索引到的图标数量（自检 / 报告用）。
    icon_count: usize,
    /// 上一次同步到状态栏的内容，避免每帧都 `set_text`。
    shown: Option<StatusSnapshot>,
    /// 指针最近一次落在画布区域里的位置（用于状态栏的像素坐标）。
    pointer: Option<Vec2>,
    /// 透明画布区域控件，用来取 fit 区域和做滚轮 / 平移的命中测试。
    canvas_area: NodeId,
    renderer: CpuRenderer,
    render_target: RenderTarget,
    /// 文档变了、还没上传的合成结果（整图重合成）。
    texture_dirty: bool,
    /// 只有这一块（文档坐标）需要重合成；`texture_dirty` 为真时整图优先。
    dirty_region: Option<crate::document::PixelRegion>,
    /// 显示用的透明棋盘格背景是否开启。
    show_checkerboard: bool,
    /// 是否已做过首次适配。
    fitted: bool,
    viewport: ViewportSize,
    /// 中键平移时上一次的指针位置。
    pan_last: Option<Vec2>,
    /// 画笔 / 橡皮引擎（同一个，靠 `mode` 区分）。
    brush: BrushTool,
    /// 移动工具（Phase 8）。
    move_tool: MoveTool,
    /// 钢笔：多击建路径，收尾后由选项栏做「选区 / 填充 / 描边」。
    pen: PenTool,
    /// 框选拖拽起点（文档坐标）；`Some` = 正在框选。
    select_anchor: Option<Vec2>,
    /// 修饰键（Shift 加选 / Alt 减选）。
    modifiers: igui_core::Modifiers,
    /// 像素剪贴板（宿主跨工作区共享；默认为本视图自己的一块）。
    image_clipboard: Rc<RefCell<Option<crate::tools::PixelClipboard>>>,
    /// 窗口的 device pixel 比例（宿主每帧同步）：最近邻采样时把文档矩形对齐到
    /// 设备像素网格，否则 1:1 缩放也会出现 1px / 2px 交替的锯齿。
    scale_factor: f32,
    /// 选区工具按下时决定的合并方式 + 当时的旧选区。
    merge: crate::canvas::MergeMode,
    merge_base: Option<Rc<crate::canvas::Selection>>,
    /// 套索拖动中收集的折线（文档坐标）；非空 = 正在套索。
    lasso_points: Vec<Vec2>,
    /// 图层列表的共享行数 / 选中行 / 状态。
    layer_count: Rc<Cell<usize>>,
    layer_selected: Rc<Cell<Option<usize>>>,
    layer_state: ListState,
    /// 历史列表的共享行数 / 状态；历史长度变了就刷新。
    history_count: Rc<Cell<usize>>,
    history_state: ListState,
    /// 最近一次见到的 (undo, redo) 长度；变了就刷新历史面板。
    last_history: Option<(usize, usize)>,
    /// 最近一次见到的文档 `revision`；变了就重合成 + 刷新图层列表。
    last_revision: u64,
    rename: Option<Rename>,
    props_name: NodeId,
    props_detail: NodeId,
    props_geometry: NodeId,
    last_active: Option<LayerId>,
}

impl EditorView {
    /// 构建整棵视图树，并做一次初始同步。
    pub fn new(theme: &'static dyn Theme, state: AppState) -> Self {
        build::editor(theme, state)
    }

    // -- 生命周期 --------------------------------------------------------

    /// 用后端真实字体的度量，让排版量到的宽度跟画出来的宽度一致。
    pub fn set_text_measurer(&mut self, measurer: Rc<dyn TextMeasurer>) {
        igui_ui::set_text_measurer(&mut self.tree, measurer.clone());
        self.overlays.set_text_measurer(measurer);
    }

    /// 把系统剪贴板接到文本输入（重命名浮层里的输入框）。
    pub fn set_clipboard(&mut self, clipboard: Rc<RefCell<dyn igui_ui::Clipboard>>) {
        igui_ui::set_clipboard(&mut self.tree, clipboard.clone());
        self.overlays.set_clipboard(clipboard);
    }

    /// 宿主每帧同步修饰键：Shift 加选、Alt 减选（框选 / 套索 / 魔棒）。
    pub fn set_modifiers(&mut self, modifiers: igui_core::Modifiers) {
        self.modifiers = modifiers;
    }

    /// 宿主每帧同步 device pixel 比例（对齐设备像素网格用）。
    pub fn set_scale_factor(&mut self, scale_factor: f32) {
        self.scale_factor = scale_factor.max(0.01);
    }

    /// 文档纹理该用哪种采样：缩小用线性（不然细笔迹会断成毛刺），放大用最近邻。
    pub fn image_smoothing(&self) -> crate::canvas::ImageSmoothing {
        crate::canvas::ImageSmoothing::for_zoom(self.state.borrow().canvas.zoom)
    }

    /// 在菜单栏下面预留 `height` 逻辑像素，给宿主画工作区标签条。
    ///
    /// `0` 就把这一行折叠掉（没有标签条的界面，比如单窗口 / 测试）。
    pub fn set_tab_band(&mut self, height: f32) {
        let height = height.max(0.0);
        let band = self.tab_band;
        if self.tree.is_visible(band) != Some(height > 0.0) {
            self.tree.set_visible(band, height > 0.0);
        }
        update_control(&mut self.tree, band, |data| {
            data.layout.basis = SizeBasis::Px(height);
            data.min_size.height = height;
        });
        igui_ui::mark_dirty(&mut self.tree, band);
    }

    /// 菜单栏占的矩形（宿主据此把标签条钉在它下面）。
    pub fn menu_bar_rect(&self) -> Option<Rect> {
        igui_ui::control(&self.tree, self.menu_bar).map(|control| control.rect)
    }

    /// 当前文档的 revision（工作区脏标记用）。
    pub fn document_revision(&self) -> u64 {
        self.state.borrow().document.revision()
    }

    /// 当前文档的快照（保存工作区用）。
    pub fn document_snapshot(&self) -> crate::document::Document {
        self.state.borrow().document.clone()
    }

    /// 宿主每帧调用：同步状态栏、在文档改动后刷新图层列表并标记重合成。
    pub fn update(&mut self) {
        self.sync_status();
        self.sync_path();
        // 意图取一次，按到达顺序处理：工具参数当场应用（下面的 `sync_options`
        // 要显示新值），菜单 / 重命名 / 工具切换记下来，留到各自的阶段 —— 顺序
        // 与原来那 12 条请求格一模一样。`Intent::File` 留在队列里给宿主。
        let mut request_rename = None;
        let mut request_menu_action = None;
        let mut request_menu = None;
        let mut request_tool = None;
        for intent in self.intents.take() {
            match intent {
                Intent::Brush(adjust) => self.apply_brush_adjust(adjust),
                Intent::OpenBrushPresets(anchor) => self.open_brush_presets(anchor),
                Intent::BrushPreset(index) => {
                    self.overlays.close_all();
                    self.apply_brush_preset(index);
                }
                Intent::Pen(action) => self.apply_pen_action(action),
                Intent::Bucket(adjust) => self.apply_bucket_adjust(adjust),
                Intent::Gradient(adjust) => {
                    self.gradient.kind = match adjust {
                        GradientAdjust::KindPrev => self.gradient.kind.prev(),
                        GradientAdjust::KindNext => self.gradient.kind.next(),
                    };
                }
                Intent::OpenGradientEditor => self.open_gradient_editor(),
                Intent::RenameLayer(anchor) => request_rename = Some(anchor),
                Intent::MenuAction(action) => request_menu_action = Some(action),
                Intent::OpenMenu(index) => request_menu = Some(index),
                Intent::Tool(tool) => request_tool = Some(tool),
                Intent::File(_) => {}
            }
        }
        // 选项栏开关是显示源，`brush` / `paint_bucket` 是绘制源，这里每帧对齐。
        self.brush.hard = self.options.pixel_mode.get();
        self.brush.shape = if self.options.square_mode.get() {
            BrushShape::Square
        } else {
            BrushShape::Round
        };
        self.brush.symmetry = self.options.symmetry_mode.get();
        self.paint_bucket.contiguous = self.options.bucket_contiguous.get();
        // 渐变：未自定义时首尾跟随前景 / 背景；工具读类型 / 反向。
        if !self.gradient.customized.get() {
            let (foreground, background) = {
                let state = self.state.borrow();
                (state.foreground, state.background)
            };
            self.gradient
                .ramp
                .borrow_mut()
                .set_endpoints(foreground, background);
        }
        self.gradient.tool.kind = self.gradient.kind;
        self.gradient.tool.reverse = self.gradient.reverse.get();
        self.sync_options();

        let (revision, active) = {
            let state = self.state.borrow();
            (state.document.revision(), state.document.active_layer)
        };
        if revision != self.last_revision {
            self.last_revision = revision;
            self.texture_dirty = true;
            self.refresh_layers();
            self.sync_properties();
        }
        if active != self.last_active {
            self.last_active = active;
            self.sync_properties();
        }
        let history = {
            let state = self.state.borrow();
            (state.history.undo_len(), state.history.redo_len())
        };
        if self.last_history != Some(history) {
            self.last_history = Some(history);
            self.refresh_history();
        }

        if let Some(anchor) = request_rename {
            self.open_rename(anchor);
        }
        // 重命名浮层：Enter / 「确定」写 `confirm` 格，「取消」写 `cancel` 格；
        // Esc / 点外部由浮层自己关，下面的 `is_empty` 兜底清记录。
        let rename_finish = match &self.rename {
            Some(rename) if rename.confirm.get() => Some(true),
            Some(rename) if rename.cancel.get() => Some(false),
            _ => None,
        };
        if let Some(commit) = rename_finish {
            self.finish_rename(commit);
        }
        if let Some(action) = request_menu_action {
            self.overlays.close_all();
            self.apply_menu_action(action);
        }
        if let Some(index) = request_menu {
            self.open_menu(index);
        }
        // 工具栏：长按展开浮层、应用工具切换意图、同步组的「脸」。
        self.poll_long_press();
        if let Some(tool) = request_tool {
            self.state.borrow_mut().active_tool = tool;
            self.overlays.close_all();
            self.open_tool_group.set(None);
        }
        self.sync_tool_faces();
        // Esc / 点外部已经关掉了菜单 / 浮层时，清掉记录。
        if self.overlays.is_empty() {
            self.open_menu.set(None);
            self.open_tool_group.set(None);
            self.gradient.editor_open.set(false);
            self.rename = None;
        }
        if let Some(message) = self.message.borrow_mut().take() {
            set_text(&mut self.tree, self.message_label, message);
        }
        self.sync_tabs();
    }
}

/// 画布动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CanvasAction {
    ZoomIn,
    ZoomOut,
    Reset,
    Fit,
}

/// 画布快捷键：`+`/`=` 放大、`-` 缩小、`0` 100%、`F` 适配。
fn canvas_shortcut(key: Key) -> Option<CanvasAction> {
    match key {
        Key::Character('+') | Key::Character('=') => Some(CanvasAction::ZoomIn),
        Key::Character('-') => Some(CanvasAction::ZoomOut),
        Key::Character('0') => Some(CanvasAction::Reset),
        Key::Character('f') | Key::Character('F') => Some(CanvasAction::Fit),
        _ => None,
    }
}

/// `[`/`]` 调整笔刷直径。
fn brush_size_shortcut(key: Key) -> Option<f32> {
    match key {
        Key::Character('[') => Some(-2.0),
        Key::Character(']') => Some(2.0),
        _ => None,
    }
}

/// `,`/`.` 调整笔刷不透明度。
fn brush_opacity_shortcut(key: Key) -> Option<f32> {
    match key {
        Key::Character(',') => Some(-0.05),
        Key::Character('.') => Some(0.05),
        _ => None,
    }
}

/// 左键指针事件（这些工具只处理左键）。
fn left_pointer(position: Vec2) -> PointerEvent {
    PointerEvent {
        position,
        button: PointerButton::Left,
    }
}

/// 工具快捷键：`V` 移动、`B` 画笔、`E` 橡皮、`M` 框选、`L` 套索、`I` 吸管。
fn shortcut_tool(key: Key) -> Option<ActiveTool> {
    match key {
        Key::Character('v') | Key::Character('V') => Some(ActiveTool::Move),
        Key::Character('b') | Key::Character('B') => Some(ActiveTool::Brush),
        Key::Character('e') | Key::Character('E') => Some(ActiveTool::Eraser),
        Key::Character('m') | Key::Character('M') => Some(ActiveTool::RectangleSelect),
        Key::Character('l') | Key::Character('L') => Some(ActiveTool::LassoSelect),
        Key::Character('p') | Key::Character('P') => Some(ActiveTool::Pen),
        Key::Character('g') | Key::Character('G') => Some(ActiveTool::PaintBucket),
        Key::Character('w') | Key::Character('W') => Some(ActiveTool::MagicWand),
        Key::Character('n') | Key::Character('N') => Some(ActiveTool::Gradient),
        Key::Character('r') | Key::Character('R') => Some(ActiveTool::RectangleFill),
        Key::Character('u') | Key::Character('U') => Some(ActiveTool::FreeFill),
        Key::Character('i') | Key::Character('I') => Some(ActiveTool::Eyedropper),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
