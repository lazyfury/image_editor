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
mod canvas;
mod card;
mod confirm;
mod file_panel;
mod gradient_editor;
mod history_panel;
mod home;
mod layer_panel;
pub mod menu;
mod new_document;
mod options_bar;
mod paint;
mod palette;
mod properties_panel;
mod selection;
mod status_bar;
mod tabs;
mod toolbar;
mod window;
mod workspace_tabs;

pub use confirm::{ConfirmDialog, ConfirmResult};
pub use home::{HomeEntry, HomeOpenRequest, HomeView};
pub use new_document::{NewDocumentModal, NewDocumentResult, NewDocumentSpec};
pub use options_bar::BrushAdjust;
pub use options_bar::BrushToggle;
pub use options_bar::GradientAdjust;
pub use options_bar::{BucketAdjust, BucketToggle};
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
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use igui_components::{
    set_text, update_control, Component, Divider, Flex, ListState, NodeRef, Overlays, Placement,
    ResizeHandle,
};
use igui_core::{
    Edges, EventResult, InputEvent, Key, NodeId, PointerButton, Rect, Size, Vec2, ViewportSize,
};
use igui_scene::SceneTree;
use igui_theme::{space, Theme};
use igui_ui::{MouseFilter, SizeBasis, TextMeasurer};

use crate::app::state::{ActiveTool, AppState, HistoryAction};
use crate::app::tool_groups::{group_index, TOOL_GROUPS};
use crate::canvas::{
    document_to_pixel, paint_backdrop, pixel_selection, screen_to_document, CanvasCamera, Selection,
};
use crate::document::{
    AddLayerCommand, CropLayerCommand, Gradient, Layer, LayerId, LayerMetaCommand,
};
use crate::icons::IconSet;
use crate::renderer::{sample_pixel, CpuRenderer, RenderTarget, Renderer};
use crate::tools::{
    fill_selection, BrushMode, BrushShape, BrushTool, GradientKind, GradientTool, MoveTool,
    PaintBucketTool, PointerEvent, RectangleFillTool, Tool, ToolContext,
};
use crate::ui::options_bar::{
    options_bar, options_hint, tool_has_brush, GradientOptions, OptionsRefs,
};
use crate::ui::tabs::TabsView;

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

/// 正在进行的图层重命名。
struct Rename {
    id: LayerId,
    buffer: String,
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
    /// 工具激活请求（短按 / 浮层选中）。
    tool_request: Rc<Cell<Option<ActiveTool>>>,
    /// 当前展开的工具浮层属于哪个组；没展开为 `None`。
    open_tool_group: Cell<Option<usize>>,
    /// 长按阈值（测试可调，默认 [`LONG_PRESS_MS`]）。
    long_press_threshold: Duration,
    /// 页面（根 Flex 的子节点，顶部预留靠它的 `offsets` 实现）。
    page: NodeId,
    /// 顶部预留高度（工作区标签条占的那一条）。
    top_inset: f32,
    /// 图标包（长按浮层打开时还要用它画行图标）。
    icons: Rc<IconSet>,
    /// 撤销 / 重做按钮节点（Phase 6）。
    history_nodes: Vec<(HistoryAction, NodeId)>,
    palette_nodes: Vec<NodeId>,
    /// 文件面板的按钮节点（Phase 7）。
    file_nodes: Vec<(FileRequest, NodeId)>,
    /// 文件面板 / 快捷键留下的文件意图；宿主取走执行。
    file_request: Rc<Cell<Option<FileRequest>>>,
    /// 当前工作区文件路径（只读展示，宿主每帧喂）。
    workspace_path: Rc<RefCell<Option<std::path::PathBuf>>>,
    /// 菜单标题留下的“打开第几个菜单”请求；`update` 取走打开下拉。
    menu_request: Rc<Cell<Option<usize>>>,
    /// 菜单项留下的动作请求；`update` 取走执行（并关闭菜单）。
    menu_action: Rc<Cell<Option<menu::MenuAction>>>,
    /// 菜单标题的节点（下拉的锚点），顺序与 [`menu::MENUS`] 一致。
    menu_nodes: Vec<NodeId>,
    /// 当前打开菜单的下标（用于点同一标题时切换成关闭）。
    open_menu: Cell<Option<usize>>,
    /// 覆盖层：承载菜单下拉，画在 UI 之上并优先接收输入。
    overlays: Overlays,
    /// 工具选项栏的文本 / 容器节点。
    options_tool: NodeId,
    options_brush: NodeId,
    options_size: NodeId,
    options_opacity: NodeId,
    options_bucket: NodeId,
    options_gradient: NodeId,
    options_tolerance: NodeId,
    options_hint: NodeId,
    /// 选项栏 `−` / `+` 留下的请求；`update` 取走执行。
    brush_request: Rc<Cell<Option<BrushAdjust>>>,
    /// `−` / `+` 按钮节点（测试 / 自检点击用）。
    brush_buttons: Vec<(BrushAdjust, NodeId)>,
    /// 像素模式 / 方形笔的共享开关状态（选项栏按钮直接翻转，`update` 同步进
    /// `self.brush`）。
    pixel_mode: Rc<Cell<bool>>,
    square_mode: Rc<Cell<bool>>,
    /// 开关按钮节点（测试 / 自检点击用）。
    brush_toggles: Vec<(BrushToggle, NodeId)>,
    /// 油漆桶选项栏的请求 / 开关 / 按钮节点。
    bucket_request: Rc<Cell<Option<BucketAdjust>>>,
    bucket_buttons: Vec<(BucketAdjust, NodeId)>,
    bucket_toggles: Vec<(BucketToggle, NodeId)>,
    bucket_contiguous: Rc<Cell<bool>>,
    /// 油漆桶工具（颜色 / 容差 / 连续 / 选区在落笔前同步）。
    paint_bucket: PaintBucketTool,
    /// 矩形填充工具（拖动中暴露预览角点）。
    rect_fill: RectangleFillTool,
    /// 渐变工具 + 它的多节点 ramp（共享给编辑器）/ 类型 / 反向。
    gradient: Rc<RefCell<Gradient>>,
    gradient_tool: GradientTool,
    gradient_kind: GradientKind,
    gradient_reverse: Rc<Cell<bool>>,
    /// 用户改过渐变节点后为 true；否则首尾跟随前景 / 背景。
    gradient_customized: Rc<Cell<bool>>,
    /// 编辑器里选中的节点下标。
    gradient_selected: Rc<Cell<usize>>,
    /// 选项栏渐变类型标签 / 预览条 / 编辑按钮节点。
    gradient_kind_label: NodeId,
    gradient_preview_node: NodeId,
    gradient_edit_node: NodeId,
    /// 「编辑…」请求 / 类型切换请求 / 编辑器是否开着。
    gradient_edit_request: Rc<Cell<bool>>,
    gradient_adjust: Rc<Cell<Option<GradientAdjust>>>,
    gradient_editor_open: Cell<bool>,
    /// 渐变类型切换按钮节点（测试 / 自检点击用）。
    gradient_buttons: Vec<(GradientAdjust, NodeId)>,
    /// 右侧栏共享宽度（分隔条写、`clamp_sidebar_width` 也写）。
    sidebar_width: Rc<Cell<f32>>,
    /// 右侧栏节点（分隔条的目标）。
    sidebar_node: NodeId,
    /// 右侧栏分隔条的节点。
    sidebar_handle_node: NodeId,
    /// 左侧调色盘面板的宽度 / 节点 / 分隔条。
    palette_width: Rc<Cell<f32>>,
    palette_panel_node: NodeId,
    palette_picker_node: NodeId,
    palette_handle_node: NodeId,
    /// 侧栏标签页：当前页（按钮写）、面板高度 / 节点 / 分隔条。
    active_tab: Rc<Cell<SidebarTab>>,
    /// 上一次同步到树的标签页；变了才重设可见性 + 标脏重排。
    shown_tab: Option<SidebarTab>,
    tabs_height: Rc<Cell<f32>>,
    tabs_panel_node: NodeId,
    tabs_handle_node: NodeId,
    /// 标签按钮 / 内容容器节点，测试与自检靠它们真的点一下。
    tab_buttons: Vec<(SidebarTab, NodeId)>,
    tab_contents: Vec<(SidebarTab, NodeId)>,
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
    /// 文档变了、还没上传的合成结果。
    texture_dirty: bool,
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
    /// 框选拖拽起点（文档坐标）；`Some` = 正在框选。
    select_anchor: Option<Vec2>,
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
    /// “重命名”按钮点过之后置位，由 `update` 取走。
    rename_request: Rc<Cell<bool>>,
    renaming: Option<Rename>,
    props_name: NodeId,
    props_detail: NodeId,
    props_geometry: NodeId,
    last_active: Option<LayerId>,
}

impl EditorView {
    /// 构建整棵视图树，并做一次初始同步。
    pub fn new(theme: &'static dyn Theme, state: AppState) -> Self {
        let refs = Refs::default();
        let document_size = (state.document.width, state.document.height);
        let state = Rc::new(RefCell::new(state));
        let message: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        // 文件面板 / 快捷键只发意图；路径由宿主的原生对话框选。
        let file_request: Rc<Cell<Option<FileRequest>>> = Rc::new(Cell::new(None));
        let workspace_path: Rc<RefCell<Option<std::path::PathBuf>>> = Rc::new(RefCell::new(None));
        let menu_request: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
        let menu_action: Rc<Cell<Option<menu::MenuAction>>> = Rc::new(Cell::new(None));
        let mut menu_refs: Vec<NodeRef> = Vec::new();
        let mut file_refs: Vec<(FileRequest, NodeRef)> = Vec::new();
        let file_panel =
            file_panel::file_panel(theme, file_request.clone(), &refs.path, &mut file_refs);

        let mut group_refs: Vec<NodeRef> = Vec::new();
        let mut label_refs: Vec<NodeRef> = Vec::new();
        let mut history_refs: Vec<(HistoryAction, NodeRef)> = Vec::new();
        let mut palette_refs: Vec<NodeRef> = Vec::new();
        // 图标包只加载一次；工具栏的按钮在构建时就把 `Icon` 子组件搭进去。
        let icons = Rc::new(IconSet::load());
        let tool_faces: Vec<Rc<Cell<ActiveTool>>> = TOOL_GROUPS
            .iter()
            .map(|group| Rc::new(Cell::new(group.face())))
            .collect();
        let long_press: Rc<RefCell<toolbar::LongPress>> =
            Rc::new(RefCell::new(toolbar::LongPress::Idle));
        let tool_request: Rc<Cell<Option<ActiveTool>>> = Rc::new(Cell::new(None));
        let toolbar = toolbar::tool_bar(
            theme,
            toolbar::ToolbarWiring {
                state: state.clone(),
                message: message.clone(),
                icons: icons.clone(),
                faces: tool_faces.clone(),
                long_press: long_press.clone(),
                request: tool_request.clone(),
            },
            &mut group_refs,
            &mut label_refs,
            &mut history_refs,
        );
        let palette_panel = palette::palette_panel(
            theme,
            state.clone(),
            &mut palette_refs,
            &refs.palette_picker,
        );

        let layer_count = Rc::new(Cell::new(0usize));
        let layer_selected: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
        let rename_request = Rc::new(Cell::new(false));
        let layer_list = layer_panel::layer_list(
            theme,
            state.clone(),
            layer_count.clone(),
            layer_selected.clone(),
        );
        let layer_state = layer_list.state();

        let history_count = Rc::new(Cell::new(0usize));
        let history_list = history_panel::history_list(theme, state.clone(), history_count.clone());
        let history_state = history_list.state();

        // 工具选项栏（菜单栏下面一行）+ 可拖动的右栏。
        let options = OptionsRefs::default();
        let brush_request: Rc<Cell<Option<BrushAdjust>>> = Rc::new(Cell::new(None));
        let mut brush_refs: Vec<(BrushAdjust, NodeRef)> = Vec::new();
        let pixel_mode = Rc::new(Cell::new(true));
        let square_mode = Rc::new(Cell::new(true));
        let mut toggle_refs: Vec<(BrushToggle, NodeRef)> = Vec::new();
        let bucket_request: Rc<Cell<Option<BucketAdjust>>> = Rc::new(Cell::new(None));
        let mut bucket_refs: Vec<(BucketAdjust, NodeRef)> = Vec::new();
        let bucket_contiguous = Rc::new(Cell::new(true));
        let mut bucket_toggle_refs: Vec<(BucketToggle, NodeRef)> = Vec::new();
        let gradient = Rc::new(RefCell::new(Gradient::from_colors(
            crate::document::Color::BLACK,
            crate::document::Color::WHITE,
        )));
        let gradient_reverse = Rc::new(Cell::new(false));
        let gradient_request: Rc<Cell<Option<GradientAdjust>>> = Rc::new(Cell::new(None));
        let gradient_edit_request = Rc::new(Cell::new(false));
        let mut gradient_refs: Vec<(GradientAdjust, NodeRef)> = Vec::new();
        let sidebar_width = Rc::new(Cell::new(SIDEBAR_WIDTH));
        let palette_width = Rc::new(Cell::new(PALETTE_WIDTH));
        let active_tab: Rc<Cell<SidebarTab>> = Rc::new(Cell::new(SidebarTab::default()));
        let tabs_height = Rc::new(Cell::new(TABS_PANEL_HEIGHT));

        // 侧栏标签页：文件 / 历史 / 属性共用一个卡片。按钮与内容容器各留一个
        // `NodeRef`，建完后收进 `tab_buttons` / `tab_contents` 供交互与可见性同步。
        let properties = properties_panel::properties_panel(
            theme,
            &refs.props_name,
            &refs.props_detail,
            &refs.props_geometry,
        );
        let mut tab_refs: Vec<(SidebarTab, NodeRef, NodeRef)> = Vec::new();
        let mut tabs_view = TabsView::new(theme, active_tab.clone());
        let file_button = NodeRef::new();
        let file_content = NodeRef::new();
        tabs_view = tabs_view.tab(SidebarTab::File, &file_button, &file_content, file_panel);
        tab_refs.push((SidebarTab::File, file_button, file_content));
        let history_button = NodeRef::new();
        let history_content = NodeRef::new();
        tabs_view = tabs_view.tab(
            SidebarTab::History,
            &history_button,
            &history_content,
            history_list,
        );
        tab_refs.push((SidebarTab::History, history_button, history_content));
        let props_button = NodeRef::new();
        let props_content = NodeRef::new();
        tabs_view = tabs_view.tab(
            SidebarTab::Properties,
            &props_button,
            &props_content,
            properties,
        );
        tab_refs.push((SidebarTab::Properties, props_button, props_content));

        // 布局根（SceneTree 根）的子节点是按 anchors 摆的，flex 从下一层才开始 ——
        // 所以页面 column 必须是根的唯一子节点（demo_app / file_browser 同款形状）。
        let page = Flex::column()
            .gap(0.0)
            .padding(Edges::ZERO)
            .mouse_filter(MouseFilter::Ignore)
            .child(menu::menu_bar(theme, menu_request.clone(), &mut menu_refs))
            .child(options_bar(
                theme,
                brush_request.clone(),
                pixel_mode.clone(),
                square_mode.clone(),
                bucket_request.clone(),
                bucket_contiguous.clone(),
                GradientOptions {
                    gradient: gradient.clone(),
                    reverse: gradient_reverse.clone(),
                    request: gradient_request.clone(),
                    edit: gradient_edit_request.clone(),
                },
                &options,
                &mut brush_refs,
                &mut toggle_refs,
                &mut bucket_refs,
                &mut bucket_toggle_refs,
                &mut gradient_refs,
            ))
            .child(
                Flex::row()
                    .gap(0.0)
                    .padding(Edges::ZERO)
                    .grow(1.0)
                    .mouse_filter(MouseFilter::Ignore)
                    .child(toolbar)
                    .child(Divider::vertical(theme))
                    .child(
                        Flex::row()
                            .background(theme.background())
                            .padding(Edges::ZERO)
                            .gap(0.0)
                            .child(
                                // 调色盘外面留一圈边距，让它像右栏那些卡片一样“浮”在底色上，
                                // 而不是一条贴着工具栏的通栏面板。
                                Flex::column()
                                    .basis(SizeBasis::Px(palette_width.get()))
                                    .shrink(0.0)
                                    .padding(Edges::ZERO)
                                    .mouse_filter(MouseFilter::Ignore)
                                    .child(palette_panel)
                                    .ref_(&refs.palette_panel),
                            )
                            .child(
                                ResizeHandle::vertical(theme)
                                    .target(refs.palette_panel.clone())
                                    .width(palette_width.clone())
                                    .min(PALETTE_MIN)
                                    .max(PALETTE_MAX)
                                    .ref_(&refs.palette_handle),
                            ),
                    )
                    .child(canvas::canvas_area().ref_(&refs.canvas))
                    .child(
                        Flex::row()
                            .background(theme.background())
                            .padding(Edges::ZERO)
                            .gap(0.0)
                            .child(
                                ResizeHandle::vertical(theme)
                                    .target(refs.sidebar.clone())
                                    .width(sidebar_width.clone())
                                    .min(SIDEBAR_MIN)
                                    .invert()
                                    .ref_(&refs.sidebar_handle),
                            )
                            .child(
                                Flex::column()
                                    .basis(SizeBasis::Px(sidebar_width.get()))
                                    .shrink(0.0)
                                    .gap(space::XXXS)
                                    .padding(Edges::ZERO)
                                    .mouse_filter(MouseFilter::Ignore)
                                    .child(
                                        tabs_view
                                            .basis(SizeBasis::Px(tabs_height.get()))
                                            .shrink(0.0)
                                            .ref_(&refs.tabs_panel),
                                    )
                                    .child(
                                        ResizeHandle::horizontal(theme)
                                            .target(refs.tabs_panel.clone())
                                            .width(tabs_height.clone())
                                            .min(TABS_PANEL_MIN)
                                            .max(TABS_PANEL_MAX)
                                            .ref_(&refs.tabs_handle),
                                    )
                                    .child(layer_panel::layer_panel(
                                        theme,
                                        state.clone(),
                                        layer_list,
                                        rename_request.clone(),
                                    ))
                                    .ref_(&refs.sidebar),
                            ),
                    ),
            )
            .child(status_bar::status_bar(
                theme,
                &refs.tool,
                &refs.message,
                &refs.zoom,
            ));

        let page_ref = NodeRef::new();
        let root_flex = Flex::new()
            .gap(0.0)
            .padding(Edges::ZERO)
            .mouse_filter(MouseFilter::Ignore)
            .child(page.ref_(&page_ref));
        let mut tree = SceneTree::new();
        tree.add_child(tree.root(), root_flex);
        let page = page_ref.get().expect("页面已挂载");

        let tool_group_nodes: Vec<NodeId> = group_refs
            .iter()
            .map(|slot| slot.get().expect("tool group button mounted"))
            .collect();
        let tool_face_labels: Vec<NodeId> = label_refs
            .iter()
            .map(|slot| slot.get().expect("tool face label mounted"))
            .collect();
        let history_nodes: Vec<(HistoryAction, NodeId)> = history_refs
            .into_iter()
            .map(|(action, slot)| (action, slot.get().expect("history button mounted")))
            .collect();
        let palette_nodes: Vec<NodeId> = palette_refs
            .iter()
            .map(|slot| slot.get().expect("palette swatch mounted"))
            .collect();
        let file_nodes: Vec<(FileRequest, NodeId)> = file_refs
            .into_iter()
            .map(|(action, slot)| (action, slot.get().expect("file button mounted")))
            .collect();
        let menu_nodes: Vec<NodeId> = menu_refs
            .iter()
            .map(|slot| slot.get().expect("menu button mounted"))
            .collect();
        let brush_buttons: Vec<(BrushAdjust, NodeId)> = brush_refs
            .into_iter()
            .map(|(adjust, slot)| (adjust, slot.get().expect("brush button mounted")))
            .collect();
        let brush_toggles: Vec<(BrushToggle, NodeId)> = toggle_refs
            .into_iter()
            .map(|(toggle, slot)| (toggle, slot.get().expect("brush toggle mounted")))
            .collect();
        let bucket_buttons: Vec<(BucketAdjust, NodeId)> = bucket_refs
            .into_iter()
            .map(|(adjust, slot)| (adjust, slot.get().expect("bucket button mounted")))
            .collect();
        let bucket_toggles: Vec<(BucketToggle, NodeId)> = bucket_toggle_refs
            .into_iter()
            .map(|(toggle, slot)| (toggle, slot.get().expect("bucket toggle mounted")))
            .collect();
        let gradient_buttons: Vec<(GradientAdjust, NodeId)> = gradient_refs
            .into_iter()
            .map(|(adjust, slot)| (adjust, slot.get().expect("gradient button mounted")))
            .collect();
        let tab_buttons: Vec<(SidebarTab, NodeId)> = tab_refs
            .iter()
            .map(|(tab, button, _)| (*tab, button.get().expect("tab button mounted")))
            .collect();
        let tab_contents: Vec<(SidebarTab, NodeId)> = tab_refs
            .iter()
            .map(|(tab, _, content)| (*tab, content.get().expect("tab content mounted")))
            .collect();

        // 图标包在构建按钮时已被各 `Icon` 组件解析并持有（decorator 里），这里
        // 只留个数给自检 / 报告用。
        let icon_count = icons.len();

        let mut view = Self {
            tree,
            page,
            top_inset: 0.0,
            theme,
            state,
            message,
            tool_label: refs.tool.get().expect("tool label mounted"),
            zoom_label: refs.zoom.get().expect("zoom label mounted"),
            message_label: refs.message.get().expect("message label mounted"),
            tool_group_nodes,
            tool_face_labels,
            tool_faces,
            long_press,
            tool_request,
            open_tool_group: Cell::new(None),
            long_press_threshold: Duration::from_millis(LONG_PRESS_MS),
            icons,
            history_nodes,
            palette_nodes,
            file_nodes,
            file_request,
            workspace_path,
            menu_request,
            menu_action,
            menu_nodes,
            open_menu: Cell::new(None),
            overlays: Overlays::new(theme),
            options_tool: options.tool.get().expect("options tool mounted"),
            options_brush: options.brush.get().expect("options brush mounted"),
            options_size: options.size.get().expect("options size mounted"),
            options_opacity: options.opacity.get().expect("options opacity mounted"),
            options_bucket: options.bucket.get().expect("options bucket mounted"),
            options_gradient: options.gradient.get().expect("options gradient mounted"),
            options_tolerance: options.tolerance.get().expect("options tolerance mounted"),
            options_hint: options.hint.get().expect("options hint mounted"),
            brush_request,
            brush_buttons,
            pixel_mode,
            square_mode,
            brush_toggles,
            bucket_request,
            bucket_buttons,
            bucket_toggles,
            bucket_contiguous,
            paint_bucket: PaintBucketTool::new(),
            rect_fill: RectangleFillTool::new(),
            gradient_tool: GradientTool::new(gradient.clone()),
            gradient,
            gradient_kind: GradientKind::default(),
            gradient_reverse,
            gradient_customized: Rc::new(Cell::new(false)),
            gradient_selected: Rc::new(Cell::new(0)),
            gradient_kind_label: options
                .gradient_kind
                .get()
                .expect("gradient kind label mounted"),
            gradient_preview_node: options
                .gradient_preview
                .get()
                .expect("gradient preview mounted"),
            gradient_edit_node: options.gradient_edit.get().expect("gradient edit mounted"),
            gradient_edit_request,
            gradient_adjust: gradient_request,
            gradient_editor_open: Cell::new(false),
            gradient_buttons,
            sidebar_width,
            sidebar_node: refs.sidebar.get().expect("sidebar mounted"),
            sidebar_handle_node: refs.sidebar_handle.get().expect("sidebar handle mounted"),
            palette_width,
            palette_panel_node: refs.palette_panel.get().expect("palette panel mounted"),
            palette_picker_node: refs.palette_picker.get().expect("palette picker mounted"),
            palette_handle_node: refs.palette_handle.get().expect("palette handle mounted"),
            active_tab,
            shown_tab: None,
            tabs_height,
            tabs_panel_node: refs.tabs_panel.get().expect("tabs panel mounted"),
            tabs_handle_node: refs.tabs_handle.get().expect("tabs handle mounted"),
            tab_buttons,
            tab_contents,
            path_label: refs.path.get().expect("path label mounted"),
            shown_path: None,
            icon_count,
            shown: None,
            pointer: None,
            canvas_area: refs.canvas.get().expect("canvas area mounted"),
            renderer: CpuRenderer,
            render_target: RenderTarget::new(document_size.0, document_size.1),
            texture_dirty: true,
            show_checkerboard: true,
            fitted: false,
            viewport: ViewportSize::new(Size::new(1280.0, 800.0)),
            pan_last: None,
            brush: BrushTool::paint(),
            move_tool: MoveTool::new(),
            select_anchor: None,
            lasso_points: Vec::new(),
            layer_count,
            layer_selected,
            layer_state,
            history_count,
            history_state,
            last_history: None,
            last_revision: u64::MAX,
            rename_request,
            renaming: None,
            props_name: refs.props_name.get().expect("props name mounted"),
            props_detail: refs.props_detail.get().expect("props detail mounted"),
            props_geometry: refs.props_geometry.get().expect("props geometry mounted"),
            last_active: None,
        };
        view.update();
        view
    }

    // -- 生命周期 --------------------------------------------------------

    /// 用后端真实字体的度量，让排版量到的宽度跟画出来的宽度一致。
    pub fn set_text_measurer(&mut self, measurer: Rc<dyn TextMeasurer>) {
        igui_ui::set_text_measurer(&mut self.tree, measurer.clone());
        self.overlays.set_text_measurer(measurer);
    }

    /// 顶部预留 `inset` 逻辑像素（工作区标签条占的那一条）。
    ///
    /// 根 Flex 的子节点按 anchors 摆，所以把预留加在**页面**的 offsets 上。
    pub fn set_top_inset(&mut self, inset: f32) {
        self.top_inset = inset;
        let page = self.page;
        update_control(&mut self.tree, page, |data| {
            data.anchors = Edges::new(0.0, 0.0, 1.0, 1.0);
            data.offsets = Edges::new(0.0, inset, 0.0, 0.0);
        });
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
        if let Some(adjust) = self.brush_request.take() {
            self.apply_brush_adjust(adjust);
        }
        if let Some(adjust) = self.bucket_request.take() {
            self.apply_bucket_adjust(adjust);
        }
        if let Some(adjust) = self.gradient_adjust.take() {
            self.gradient_kind = match adjust {
                GradientAdjust::KindPrev => self.gradient_kind.prev(),
                GradientAdjust::KindNext => self.gradient_kind.next(),
            };
        }
        if self.gradient_edit_request.replace(false) {
            self.open_gradient_editor();
        }
        // 选项栏开关是显示源，`brush` / `paint_bucket` 是绘制源，这里每帧对齐。
        self.brush.hard = self.pixel_mode.get();
        self.brush.shape = if self.square_mode.get() {
            BrushShape::Square
        } else {
            BrushShape::Round
        };
        self.paint_bucket.contiguous = self.bucket_contiguous.get();
        // 渐变：未自定义时首尾跟随前景 / 背景；工具读类型 / 反向。
        if !self.gradient_customized.get() {
            let (foreground, background) = {
                let state = self.state.borrow();
                (state.foreground, state.background)
            };
            self.gradient
                .borrow_mut()
                .set_endpoints(foreground, background);
        }
        self.gradient_tool.kind = self.gradient_kind;
        self.gradient_tool.reverse = self.gradient_reverse.get();
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

        if self.rename_request.replace(false) {
            self.begin_rename();
        }
        if let Some(action) = self.menu_action.take() {
            self.overlays.close_all();
            self.apply_menu_action(action);
        }
        if let Some(index) = self.menu_request.take() {
            self.open_menu(index);
        }
        // 工具栏：长按展开浮层、处理工具激活请求、同步组的「脸」。
        self.poll_long_press();
        if let Some(tool) = self.tool_request.take() {
            self.state.borrow_mut().active_tool = tool;
            self.overlays.close_all();
            self.open_tool_group.set(None);
        }
        self.sync_tool_faces();
        // Esc / 点外部已经关掉了菜单 / 浮层时，清掉记录。
        if self.overlays.is_empty() {
            self.open_menu.set(None);
            self.open_tool_group.set(None);
            self.gradient_editor_open.set(false);
        }
        if self.renaming.is_some() {
            self.sync_rename_status();
        } else if let Some(message) = self.message.borrow_mut().take() {
            set_text(&mut self.tree, self.message_label, message);
        }
        self.sync_tabs();
    }

    /// 标签页切换：只让当前内容参与布局与绘制（隐藏内容不可见，也不命中）。
    /// 激活页没变时是空操作，避免每帧重排。
    fn sync_tabs(&mut self) {
        let active = self.active_tab.get();
        if self.shown_tab == Some(active) {
            return;
        }
        self.shown_tab = Some(active);
        tabs::show_active(
            &mut self.tree,
            self.tabs_panel_node,
            active,
            &self.tab_contents,
        );
    }

    /// 工具 / 缩放 / 指针像素 -> 状态栏。
    fn sync_status(&mut self) {
        let (tool, zoom, width, height, camera) = {
            let state = self.state.borrow();
            (
                state.active_tool,
                state.canvas.zoom,
                state.document.width,
                state.document.height,
                state.canvas,
            )
        };
        // 指针在画布区域里时，状态栏顺便报一下文档像素坐标（§12 的坐标转换）。
        let pixel = self
            .pointer
            .and_then(|point| document_to_pixel(screen_to_document(camera, point), width, height));
        let shown = (tool, zoom, width, height, pixel);
        if self.shown != Some(shown) {
            set_text(&mut self.tree, self.tool_label, tool.label());
            let coords = match pixel {
                Some((x, y)) => format!("  ·  ({x}, {y})"),
                None => String::new(),
            };
            set_text(
                &mut self.tree,
                self.zoom_label,
                format!("{width} × {height}  ·  {:.0}%{coords}", zoom * 100.0),
            );
            self.shown = Some(shown);
        }
    }

    /// 文档改动后：更新列表行数与选中行，并让已挂载的行重新读数据。
    fn refresh_layers(&mut self) {
        let count = self.state.borrow().document.layers.len();
        self.layer_count.set(count);
        self.layer_selected.set(self.active_list_index());
        self.layer_state.invalidate();
    }

    /// 历史长度变化后刷新历史面板的行数。
    fn refresh_history(&mut self) {
        let (undo, redo) = {
            let state = self.state.borrow();
            (state.history.undo_len(), state.history.redo_len())
        };
        self.history_count.set(undo + 1 + redo);
        self.history_state.invalidate();
    }

    /// 当前图层在“最上面为 0”的列表里的下标。
    fn active_list_index(&self) -> Option<usize> {
        let state = self.state.borrow();
        let document = &state.document;
        document
            .active_layer
            .and_then(|id| document.layer_index(id))
            .map(|index| document.layers.len() - 1 - index)
    }

    /// 当前图层 -> 属性面板。
    fn sync_properties(&mut self) {
        let (name, detail, geometry) = {
            let state = self.state.borrow();
            match state.document.active_layer() {
                Some(layer) => (
                    layer.name.clone(),
                    format!("不透明度 {:.0}%  ·  Normal", layer.opacity * 100.0),
                    format!(
                        "偏移 ({}, {})  ·  缓冲 {}×{}",
                        layer.position.x, layer.position.y, layer.pixels.width, layer.pixels.height
                    ),
                ),
                None => (
                    "无图层".to_string(),
                    "点「+ 图层」新建一层".to_string(),
                    String::new(),
                ),
            }
        };
        set_text(&mut self.tree, self.props_name, name);
        set_text(&mut self.tree, self.props_detail, detail);
        set_text(&mut self.tree, self.props_geometry, geometry);
    }

    fn begin_rename(&mut self) {
        let (id, name) = {
            let state = self.state.borrow();
            match state.document.active_layer() {
                Some(layer) => (layer.id, layer.name.clone()),
                None => return,
            }
        };
        self.renaming = Some(Rename { id, buffer: name });
    }

    fn cancel_rename(&mut self) {
        self.renaming = None;
        set_text(&mut self.tree, self.message_label, "");
    }

    fn commit_rename(&mut self) {
        let Some(rename) = self.renaming.take() else {
            return;
        };
        let name = rename.buffer.trim().to_string();
        if !name.is_empty() {
            let mut state = self.state.borrow_mut();
            let before = LayerMetaCommand::capture(&state.document);
            state.document.rename_layer(rename.id, name);
            let after = LayerMetaCommand::capture(&state.document);
            state.execute(Box::new(LayerMetaCommand::new(before, after, "重命名图层")));
        }
        set_text(&mut self.tree, self.message_label, "");
    }

    fn sync_rename_status(&mut self) {
        if let Some(rename) = &self.renaming {
            let text = format!("重命名图层：{}▌  （Enter 确认 · Esc 取消）", rename.buffer);
            set_text(&mut self.tree, self.message_label, text);
        }
    }

    /// 重命名编辑中的按键 / 文本输入；返回是否消费了事件。
    fn handle_rename_input(&mut self, event: &InputEvent) -> bool {
        match event {
            InputEvent::KeyDown { key: Key::Escape } => {
                self.cancel_rename();
                true
            }
            InputEvent::KeyDown { key: Key::Enter } => {
                self.commit_rename();
                true
            }
            InputEvent::KeyDown {
                key: Key::Backspace,
            } => {
                if let Some(rename) = &mut self.renaming {
                    rename.buffer.pop();
                }
                true
            }
            InputEvent::TextInput { text } => {
                if let Some(rename) = &mut self.renaming {
                    rename
                        .buffer
                        .extend(text.chars().filter(|ch| !ch.is_control()));
                }
                true
            }
            // 其余按键也吞掉：重命名时按字母不该触发工具 / 画布快捷键。
            InputEvent::KeyDown { .. } => true,
            _ => false,
        }
    }

    // -- 文件 / 路径（Phase 7） ------------------------------------------

    /// 工作区文件路径每帧同步（只读展示）。
    fn sync_path(&mut self) {
        let display = match &*self.workspace_path.borrow() {
            Some(path) => path.display().to_string(),
            None => "未保存（⌘S 保存）".to_string(),
        };
        if self.shown_path.as_deref() != Some(display.as_str()) {
            set_text(&mut self.tree, self.path_label, display.clone());
            self.shown_path = Some(display);
        }
    }

    /// 命中位置落在哪个菜单标题上（含其子节点）；用于一次点击切换菜单。
    fn menu_index_at(&self, position: Vec2) -> Option<usize> {
        let hit = igui_ui::hit_test(&self.tree, position)?;
        let mut current = Some(hit);
        while let Some(node) = current {
            if let Some(index) = self.menu_nodes.iter().position(|id| *id == node) {
                return Some(index);
            }
            current = self.tree.parent(node);
        }
        None
    }

    /// 打开第 `index` 个菜单的下拉，锚在对应标题按钮下。
    fn open_menu(&mut self, index: usize) {
        let Some(&anchor) = self.menu_nodes.get(index) else {
            return;
        };
        self.overlays.close_all();
        self.open_tool_group.set(None);
        let theme = self.theme;
        let action = self.menu_action.clone();
        let (can_undo, can_redo, has_selection) = {
            let state = self.state.borrow();
            (
                state.can_undo(),
                state.can_redo(),
                state.selection.is_some(),
            )
        };
        self.overlays.menu(anchor, move |tree, root| {
            menu::menu_content(
                tree,
                root,
                theme,
                index,
                action.clone(),
                can_undo,
                can_redo,
                has_selection,
            );
        });
        self.open_menu.set(Some(index));
    }

    /// 长按到阈值就展开对应组的工具浮层。
    fn poll_long_press(&mut self) {
        let group = {
            let state = self.long_press.borrow();
            match &*state {
                toolbar::LongPress::Pending {
                    group,
                    start,
                    moved,
                    ..
                } if !*moved && start.elapsed() >= self.long_press_threshold => Some(*group),
                _ => None,
            }
        };
        if let Some(group) = group {
            *self.long_press.borrow_mut() = toolbar::LongPress::Fired;
            self.open_tool_flyout(group);
        }
    }

    /// 展开某个组的工具浮层（向右锚在组按钮上）。
    fn open_tool_flyout(&mut self, group: usize) {
        let Some(&anchor) = self.tool_group_nodes.get(group) else {
            return;
        };
        self.overlays.close_all();
        self.open_menu.set(None);
        let theme = self.theme;
        let state = self.state.clone();
        let icons = self.icons.clone();
        let request = self.tool_request.clone();
        self.overlays
            .popover(anchor, Placement::Right, move |tree, root| {
                toolbar::tool_flyout_content(
                    tree,
                    root,
                    theme,
                    group,
                    state.clone(),
                    icons.clone(),
                    request.clone(),
                );
            });
        self.open_tool_group.set(Some(group));
    }

    /// 打开多节点渐变编辑器（popover 锚在选项栏的预览条下）。
    fn open_gradient_editor(&mut self) {
        self.overlays.close_all();
        self.open_menu.set(None);
        self.open_tool_group.set(None);
        let theme = self.theme;
        let state = self.state.clone();
        let gradient = self.gradient.clone();
        let customized = self.gradient_customized.clone();
        let selected = self.gradient_selected.clone();
        let refs = gradient_editor::GradientEditorRefs::default();
        let anchor = self.gradient_preview_node;
        self.overlays
            .popover(anchor, Placement::Below, move |tree, root| {
                gradient_editor::gradient_editor_content(
                    tree,
                    root,
                    theme,
                    state.clone(),
                    gradient.clone(),
                    customized.clone(),
                    selected.clone(),
                    &refs,
                );
            });
        self.gradient_editor_open.set(true);
    }

    /// 当前工具属于哪个组，就把那个组的「脸」更新成它；标签跟着改。
    fn sync_tool_faces(&mut self) {
        let active = self.state.borrow().active_tool;
        for (index, group) in TOOL_GROUPS.iter().enumerate() {
            if group.contains(active) {
                if let Some(face) = self.tool_faces.get(index) {
                    face.set(active);
                }
            }
            if let (Some(face), Some(&label)) =
                (self.tool_faces.get(index), self.tool_face_labels.get(index))
            {
                set_text(&mut self.tree, label, face.get().short_label());
            }
        }
    }

    /// 执行一个菜单动作（[`EditorView::update`] 在关闭菜单后调用）。
    fn apply_menu_action(&mut self, action: menu::MenuAction) {
        match action {
            menu::MenuAction::Undo => {
                self.undo();
            }
            menu::MenuAction::Redo => {
                self.redo();
            }
            menu::MenuAction::Import => self.file_request.set(Some(FileRequest::ImportPng)),
            menu::MenuAction::Export => self.file_request.set(Some(FileRequest::ExportPng)),
            menu::MenuAction::ZoomIn => self.apply_canvas_action(CanvasAction::ZoomIn),
            menu::MenuAction::ZoomOut => self.apply_canvas_action(CanvasAction::ZoomOut),
            menu::MenuAction::ZoomReset => self.apply_canvas_action(CanvasAction::Reset),
            menu::MenuAction::ZoomFit => self.apply_canvas_action(CanvasAction::Fit),
            menu::MenuAction::ClearSelection => {
                self.clear_selection();
            }
            menu::MenuAction::CropLayerToDocument => self.crop_layer_to_document(),
            menu::MenuAction::ToggleCheckerboard => {
                self.show_checkerboard = !self.show_checkerboard;
                self.texture_dirty = true;
                *self.message.borrow_mut() = Some(if self.show_checkerboard {
                    "已显示棋盘格".to_string()
                } else {
                    "已隐藏棋盘格".to_string()
                });
            }
            menu::MenuAction::About => {
                *self.message.borrow_mut() = Some(menu::about_text());
            }
            menu::MenuAction::Placeholder(note) => {
                *self.message.borrow_mut() = Some(note.to_string());
            }
        }
    }

    /// 把当前图层裁到文档大小（丢弃画布外像素，回收缓冲区）。
    fn crop_layer_to_document(&mut self) {
        let mut state = self.state.borrow_mut();
        let Some(layer) = state.document.active_layer() else {
            *self.message.borrow_mut() = Some("没有可裁剪的图层".to_string());
            return;
        };
        if layer.position == crate::document::Point::ZERO
            && layer.pixels.width == state.document.width
            && layer.pixels.height == state.document.height
        {
            *self.message.borrow_mut() = Some("图层已经在文档范围内".to_string());
            return;
        }
        let id = layer.id;
        let before = layer.pixels.clone();
        let position = layer.position;
        state.execute(Box::new(CropLayerCommand::new(id, before, position)));
        self.texture_dirty = true;
        *self.message.borrow_mut() = Some("已裁到文档大小".to_string());
    }

    /// 把当前文档合成后写成 PNG。
    pub fn export_png_to(&self, path: &str) -> Result<(), crate::io::IoError> {
        let state = self.state.borrow();
        let mut target = RenderTarget::new(0, 0);
        self.renderer.render(&state.document, &mut target);
        crate::io::write_png(Path::new(path), &target.pixels)
    }

    /// 从 PNG 文件导入一个新图层（放在最上面并选中）。
    pub fn import_png_from(&mut self, path: &str) -> Result<(u32, u32), crate::io::IoError> {
        let pixels = crate::io::read_png(Path::new(path))?;
        let size = (pixels.width, pixels.height);
        let name = crate::io::file_label(Path::new(path));
        let mut state = self.state.borrow_mut();
        let index = state.document.layers.len();
        let before_active = state.document.active_layer;
        let layer = Layer::new(name, pixels);
        state.execute(Box::new(AddLayerCommand::new(layer, index, before_active)));
        drop(state);
        self.texture_dirty = true;
        Ok(size)
    }

    /// 排布整棵树，并把相机同步到文档节点。
    pub fn layout(&mut self, viewport: ViewportSize) {
        self.viewport = viewport;
        self.tree.set_viewport_size(viewport.logical_size());
        self.clamp_palette_width();
        self.clamp_sidebar_width();
        igui_ui::layout(&mut self.tree, viewport);
        // 面板高度按刚拿到的侧栏矩形钳制一次；变了就再排一遍。
        if self.clamp_panel_heights() {
            igui_ui::layout(&mut self.tree, viewport);
        }
        // 第一次布局拿到画布区域的真实矩形，才能做适配。
        if !self.fitted {
            self.fit_to_canvas();
            self.fitted = true;
        }
        // 图层列表的行池按容器高度算，所以要在 layout 之后 sync；池变了再排一次。
        if self.layer_state.sync(&mut self.tree) {
            igui_ui::layout(&mut self.tree, viewport);
        }
        if self.history_state.sync(&mut self.tree) {
            igui_ui::layout(&mut self.tree, viewport);
        }
        // 覆盖层用自己的树，必须在主树排完之后定位。
        self.overlays.layout(&self.tree, viewport);
        self.tree.update();
    }

    /// 路由一个后端无关的输入事件。
    ///
    /// 工具快捷键、画布缩放 / 平移由视图先处理；其余（点击、指针）交给
    /// `igui_ui::handle_input`，由它去找带回调的控件。
    pub fn event(&mut self, event: &InputEvent) -> EventResult {
        // 覆盖层优先：菜单打开时，Esc / 点击外部由它先处理。
        // 例外：点的是菜单标题时，先关掉当前菜单、把点击留给主树 —— 这样点
        // 另一个标题一次就能切换，点同一个标题则是关闭。
        if let InputEvent::PointerDown { position, .. } = event {
            if self.menu_open() {
                if let Some(index) = self.menu_index_at(*position) {
                    let same = self.open_menu.get() == Some(index);
                    self.overlays.close_all();
                    self.open_menu.set(None);
                    if same {
                        return EventResult::Handled;
                    }
                } else if self.overlays.handle_input(event).is_handled() {
                    return EventResult::Handled;
                }
            } else if self.overlays.handle_input(event).is_handled() {
                return EventResult::Handled;
            }
        } else if self.overlays.handle_input(event).is_handled() {
            return EventResult::Handled;
        }
        // 重命名 / 路径编辑进行中：键盘只编辑文本，不触发工具 / 画布快捷键。
        if self.renaming.is_some() && self.handle_rename_input(event) {
            return EventResult::Handled;
        }
        match event {
            InputEvent::KeyDown { key } => {
                if let Some(tool) = shortcut_tool(*key) {
                    self.state.borrow_mut().active_tool = tool;
                    tracing::info!(target: "image_editor", tool = tool.label(), "tool_changed");
                    return EventResult::Handled;
                }
                if let Some(action) = canvas_shortcut(*key) {
                    self.apply_canvas_action(action);
                    return EventResult::Handled;
                }
                if let Some(delta) = brush_size_shortcut(*key) {
                    self.adjust_brush_size(delta);
                    return EventResult::Handled;
                }
                if let Some(delta) = brush_opacity_shortcut(*key) {
                    self.adjust_brush_opacity(delta);
                    return EventResult::Handled;
                }
                if matches!(key, Key::Escape) && self.clear_selection() {
                    return EventResult::Handled;
                }
            }
            InputEvent::Wheel { position, delta } => {
                if self.canvas_area_contains(*position) {
                    let factor = if delta.y < 0.0 {
                        ZOOM_STEP
                    } else {
                        1.0 / ZOOM_STEP
                    };
                    self.state.borrow_mut().canvas.zoom_at(*position, factor);
                    return EventResult::Handled;
                }
            }
            InputEvent::PointerDown {
                position,
                button: PointerButton::Left,
            } => {
                if self.canvas_area_contains(*position) {
                    self.begin_tool(*position);
                    return EventResult::Handled;
                }
            }
            InputEvent::PointerDown {
                position,
                button: PointerButton::Middle,
            } => {
                if self.canvas_area_contains(*position) {
                    self.pan_last = Some(*position);
                    return EventResult::Handled;
                }
            }
            InputEvent::PointerMove { position } => {
                self.pointer = self.canvas_area_contains(*position).then_some(*position);
                if let Some(last) = self.pan_last {
                    let delta = *position - last;
                    self.pan_last = Some(*position);
                    self.state.borrow_mut().canvas.pan_by(delta);
                    return EventResult::Handled;
                }
                if self.is_dragging() {
                    self.continue_tool(*position);
                    return EventResult::Handled;
                }
            }
            InputEvent::PointerLeave => {
                self.pointer = None;
            }
            InputEvent::PointerUp {
                position,
                button: PointerButton::Left,
            } => {
                if self.is_dragging() {
                    self.end_tool(*position);
                    return EventResult::Handled;
                }
            }
            InputEvent::PointerUp {
                button: PointerButton::Middle,
                ..
            } if self.pan_last.take().is_some() => {
                return EventResult::Handled;
            }
            _ => {}
        }
        igui_ui::handle_input(&mut self.tree, event)
    }

    // -- 画布 ------------------------------------------------------------

    fn apply_canvas_action(&mut self, action: CanvasAction) {
        match action {
            CanvasAction::ZoomIn => self.zoom_by(ZOOM_STEP),
            CanvasAction::ZoomOut => self.zoom_by(1.0 / ZOOM_STEP),
            CanvasAction::Reset => self.reset_view(),
            CanvasAction::Fit => self.fit_to_canvas(),
        }
    }

    fn zoom_by(&mut self, factor: f32) {
        let anchor = self
            .canvas_area_rect()
            .map(|rect| rect.center())
            .unwrap_or_else(|| {
                Rect::from_min_size(Vec2::ZERO, self.viewport.logical_size()).center()
            });
        self.state.borrow_mut().canvas.zoom_at(anchor, factor);
    }

    /// 让文档适配画布区域并居中。
    pub fn fit_to_canvas(&mut self) {
        let Some(area) = self.canvas_area_rect() else {
            return;
        };
        let document = self.document_size();
        let camera = CanvasCamera::fit(
            area,
            Size::new(document.0 as f32, document.1 as f32),
            FIT_PADDING,
        );
        self.state.borrow_mut().canvas = camera;
    }

    /// 100% 且居中。
    fn reset_view(&mut self) {
        let Some(area) = self.canvas_area_rect() else {
            return;
        };
        let document = self.document_size();
        self.state.borrow_mut().canvas =
            CanvasCamera::centered(area, Size::new(document.0 as f32, document.1 as f32), 1.0);
    }

    pub(super) fn canvas_area_rect(&self) -> Option<Rect> {
        igui_ui::control(&self.tree, self.canvas_area).map(|control| control.rect)
    }

    /// 分隔条只管 `[min, max]`，管不了"窗口变窄了"。那一半在这里：右栏最多
    /// 占到给画布留 [`CANVAS_MIN`] 为止（`examples/file_browser` 同款）。
    fn clamp_sidebar_width(&mut self) {
        let full = self.viewport.logical_size().width;
        let max =
            (full - TOOLBAR_WIDTH - self.palette_width.get() - 2.0 * RESIZE_GUTTER - CANVAS_MIN)
                .max(SIDEBAR_MIN);
        let current = self.sidebar_width.get();
        let next = current.clamp(SIDEBAR_MIN, max);
        if (next - current).abs() > f32::EPSILON {
            self.sidebar_width.set(next);
            update_control(&mut self.tree, self.sidebar_node, |data| {
                data.layout.basis = SizeBasis::Px(next);
            });
        }
    }

    /// 左侧调色盘面板的宽度不能把画布 / 右栏挤没。
    fn clamp_palette_width(&mut self) {
        let full = self.viewport.logical_size().width;
        let max = (full - TOOLBAR_WIDTH - 2.0 * RESIZE_GUTTER - SIDEBAR_MIN - CANVAS_MIN)
            .clamp(PALETTE_MIN, PALETTE_MAX);
        let current = self.palette_width.get();
        let next = current.clamp(PALETTE_MIN, max);
        if (next - current).abs() > f32::EPSILON {
            self.palette_width.set(next);
            update_control(&mut self.tree, self.palette_panel_node, |data| {
                data.layout.basis = SizeBasis::Px(next);
            });
        }
    }

    /// 保证可拖的标签页面板不会把下面的「图层」面板挤没。
    /// 返回是否调整过（调用方需要再排一次）。
    fn clamp_panel_heights(&mut self) -> bool {
        let Some(sidebar) = igui_ui::control(&self.tree, self.sidebar_node) else {
            return false;
        };
        // 预留：图层最小高度 + 分隔条 + 两个子节点之间的列间距。
        let overhead = LAYER_PANEL_MIN + RESIZE_GUTTER + space::XXXS;
        let max = (sidebar.rect.size.height - overhead).clamp(TABS_PANEL_MIN, TABS_PANEL_MAX);
        let current = self.tabs_height.get();
        let next = current.clamp(TABS_PANEL_MIN, max);
        if (next - current).abs() <= f32::EPSILON {
            return false;
        }
        self.tabs_height.set(next);
        update_control(&mut self.tree, self.tabs_panel_node, |data| {
            data.layout.basis = SizeBasis::Px(next);
        });
        true
    }

    fn canvas_area_contains(&self, position: Vec2) -> bool {
        self.canvas_area_rect()
            .is_some_and(|rect| rect.contains(position))
    }

    // -- 工具 ------------------------------------------------------------

    /// 屏幕逻辑坐标 -> 文档坐标。
    fn document_position(&self, screen: Vec2) -> Vec2 {
        let camera = self.state.borrow().canvas;
        screen_to_document(camera, screen)
    }

    /// 是否有工具正处在一次拖拽 / 笔画中。
    fn is_dragging(&self) -> bool {
        self.brush.is_drawing()
            || self.move_tool.is_moving()
            || self.select_anchor.is_some()
            || !self.lasso_points.is_empty()
            || self.rect_fill.is_filling()
            || self.gradient_tool.is_drawing()
    }

    /// 左键落下：把屏幕点换算成文档坐标，交给当前工具。
    fn begin_tool(&mut self, screen: Vec2) {
        let tool = self.state.borrow().active_tool;
        let position = self.document_position(screen);
        match tool {
            ActiveTool::Brush | ActiveTool::Eraser => {
                self.brush.mode = if tool == ActiveTool::Eraser {
                    BrushMode::Erase
                } else {
                    BrushMode::Paint
                };
                let (foreground, selection) = {
                    let state = self.state.borrow();
                    (state.foreground, state.selection.clone())
                };
                self.brush.color = foreground;
                self.brush.clip = selection;
                tracing::debug!(target: "image_editor", tool = self.brush.name(), "stroke_started");
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.brush.on_pointer_down(&mut ctx, left_pointer(position));
                self.texture_dirty = true;
            }
            ActiveTool::Move => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.move_tool
                    .on_pointer_down(&mut ctx, left_pointer(position));
                self.texture_dirty = true;
            }
            ActiveTool::RectangleSelect => {
                self.select_anchor = Some(position);
                self.update_selection(position);
            }
            ActiveTool::LassoSelect | ActiveTool::FreeFill => {
                self.lasso_points.clear();
                self.lasso_points.push(position);
            }
            ActiveTool::PaintBucket => {
                let (foreground, selection) = {
                    let state = self.state.borrow();
                    (state.foreground, state.selection.clone())
                };
                self.paint_bucket.color = foreground;
                self.paint_bucket.clip = selection;
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.paint_bucket
                    .on_pointer_down(&mut ctx, left_pointer(position));
                self.texture_dirty = true;
            }
            ActiveTool::RectangleFill => {
                let (foreground, selection) = {
                    let state = self.state.borrow();
                    (state.foreground, state.selection.clone())
                };
                self.rect_fill.color = foreground;
                self.rect_fill.clip = selection;
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.rect_fill
                    .on_pointer_down(&mut ctx, left_pointer(position));
            }
            ActiveTool::Gradient => {
                let (foreground, background, selection) = {
                    let state = self.state.borrow();
                    (state.foreground, state.background, state.selection.clone())
                };
                if !self.gradient_customized.get() {
                    self.gradient
                        .borrow_mut()
                        .set_endpoints(foreground, background);
                }
                self.gradient_tool.clip = selection;
                self.gradient_tool.kind = self.gradient_kind;
                self.gradient_tool.reverse = self.gradient_reverse.get();
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.gradient_tool
                    .on_pointer_down(&mut ctx, left_pointer(position));
            }
            ActiveTool::Eyedropper => self.pick_color(position),
        }
    }

    fn continue_tool(&mut self, screen: Vec2) {
        let tool = self.state.borrow().active_tool;
        let position = self.document_position(screen);
        match tool {
            ActiveTool::Brush | ActiveTool::Eraser if self.brush.is_drawing() => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.brush.on_pointer_move(&mut ctx, left_pointer(position));
                self.texture_dirty = true;
            }
            ActiveTool::Move if self.move_tool.is_moving() => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.move_tool
                    .on_pointer_move(&mut ctx, left_pointer(position));
                self.texture_dirty = true;
            }
            ActiveTool::RectangleSelect => self.update_selection(position),
            ActiveTool::LassoSelect | ActiveTool::FreeFill if !self.lasso_points.is_empty() => {
                if self
                    .lasso_points
                    .last()
                    .map_or(true, |last| (position - *last).length() >= 1.0)
                {
                    self.lasso_points.push(position);
                }
            }
            ActiveTool::RectangleFill if self.rect_fill.is_filling() => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.rect_fill
                    .on_pointer_move(&mut ctx, left_pointer(position));
            }
            ActiveTool::Gradient if self.gradient_tool.is_drawing() => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.gradient_tool
                    .on_pointer_move(&mut ctx, left_pointer(position));
                self.texture_dirty = true;
            }
            _ => {}
        }
    }

    fn end_tool(&mut self, screen: Vec2) {
        let tool = self.state.borrow().active_tool;
        let position = self.document_position(screen);
        match tool {
            ActiveTool::Brush | ActiveTool::Eraser if self.brush.is_drawing() => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.brush.on_pointer_up(&mut ctx, left_pointer(position));
                self.texture_dirty = true;
            }
            ActiveTool::Move if self.move_tool.is_moving() => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.move_tool
                    .on_pointer_up(&mut ctx, left_pointer(position));
            }
            ActiveTool::RectangleSelect if self.select_anchor.take().is_some() => {
                self.update_selection(position);
                let message = match self.state.borrow().selection.as_deref() {
                    Some(selection) => match selection.bounds() {
                        Some(region) => format!(
                            "选区 {} × {} @ ({}, {})",
                            region.width, region.height, region.x, region.y
                        ),
                        None => "选区已清空".to_string(),
                    },
                    None => "选区已清空".to_string(),
                };
                *self.message.borrow_mut() = Some(message);
            }
            ActiveTool::LassoSelect if !self.lasso_points.is_empty() => {
                self.lasso_points.push(position);
                let points = std::mem::take(&mut self.lasso_points);
                let (width, height) = self.document_size();
                let selection = Selection::polygon(&points, (width, height));
                let message = if selection.is_empty() {
                    "套索：没有围出区域".to_string()
                } else {
                    format!("自由选区 {} 像素", selection.area())
                };
                self.state.borrow_mut().selection =
                    (!selection.is_empty()).then(|| Rc::new(selection));
                *self.message.borrow_mut() = Some(message);
            }
            ActiveTool::FreeFill if !self.lasso_points.is_empty() => {
                self.lasso_points.push(position);
                let points = std::mem::take(&mut self.lasso_points);
                let (width, height) = self.document_size();
                let region = Selection::polygon(&points, (width, height));
                if region.is_empty() {
                    *self.message.borrow_mut() = Some("自由填充：没有围出区域".to_string());
                } else {
                    let (foreground, clip) = {
                        let state = self.state.borrow();
                        (state.foreground, state.selection.clone())
                    };
                    let mut state = self.state.borrow_mut();
                    let AppState {
                        document, history, ..
                    } = &mut *state;
                    let mut ctx = ToolContext { document, history };
                    fill_selection(&mut ctx, &region, clip.as_deref(), foreground);
                    self.texture_dirty = true;
                    *self.message.borrow_mut() = Some("自由填充：已填充".to_string());
                }
            }
            ActiveTool::RectangleFill if self.rect_fill.is_filling() => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.rect_fill
                    .on_pointer_up(&mut ctx, left_pointer(position));
                self.texture_dirty = true;
                *self.message.borrow_mut() = Some("矩形填充".to_string());
            }
            ActiveTool::Gradient if self.gradient_tool.is_drawing() => {
                let mut state = self.state.borrow_mut();
                let AppState {
                    document, history, ..
                } = &mut *state;
                let mut ctx = ToolContext { document, history };
                self.gradient_tool
                    .on_pointer_up(&mut ctx, left_pointer(position));
                self.texture_dirty = true;
                *self.message.borrow_mut() = Some("渐变".to_string());
            }
            _ => {}
        }
    }

    /// 拖拽框选：起点 + 当前点 -> 裁剪到画布的整数选区。
    fn update_selection(&mut self, position: Vec2) {
        let Some(anchor) = self.select_anchor else {
            return;
        };
        let (width, height) = self.document_size();
        let selection = pixel_selection(anchor, position, width, height)
            .map(|region| Rc::new(Selection::rect(region, (width, height))));
        self.state.borrow_mut().selection = selection;
    }

    /// 吸管：取合成后 `position` 处的颜色作为前景色。
    fn pick_color(&mut self, position: Vec2) {
        let (width, height) = self.document_size();
        let Some((x, y)) = document_to_pixel(position, width, height) else {
            *self.message.borrow_mut() = Some("吸管：点在画布外".to_string());
            return;
        };
        let color = {
            let state = self.state.borrow();
            sample_pixel(&state.document, x, y)
        };
        self.state.borrow_mut().foreground = color;
        tracing::info!(
            target: "image_editor",
            r = color.r,
            g = color.g,
            b = color.b,
            "color_picked"
        );
        *self.message.borrow_mut() = Some(format!(
            "吸管：({x}, {y}) → #{:02X}{:02X}{:02X}",
            color.r, color.g, color.b
        ));
    }

    /// Escape 清空选区；返回是否真的清了。
    fn clear_selection(&mut self) -> bool {
        if self.state.borrow().selection.is_none() {
            return false;
        }
        self.state.borrow_mut().selection = None;
        *self.message.borrow_mut() = Some("已清空选区".to_string());
        true
    }

    fn adjust_brush_size(&mut self, delta: f32) {
        self.brush.size = (self.brush.size + delta).clamp(BrushTool::MIN_SIZE, BrushTool::MAX_SIZE);
        *self.message.borrow_mut() = Some(format!("笔刷大小 {:.0}px", self.brush.size));
    }

    fn adjust_brush_opacity(&mut self, delta: f32) {
        self.brush.opacity = (self.brush.opacity + delta).clamp(0.05, 1.0);
        *self.message.borrow_mut() =
            Some(format!("笔刷不透明度 {:.0}%", self.brush.opacity * 100.0));
    }

    /// 选项栏的 `−` / `+` 请求 -> 真正的笔刷调整。
    fn apply_brush_adjust(&mut self, adjust: BrushAdjust) {
        match adjust {
            BrushAdjust::SizeDown => self.adjust_brush_size(-1.0),
            BrushAdjust::SizeUp => self.adjust_brush_size(1.0),
            BrushAdjust::OpacityDown => self.adjust_brush_opacity(-0.05),
            BrushAdjust::OpacityUp => self.adjust_brush_opacity(0.05),
        }
    }

    /// 油漆桶选项栏的「容差 −/+」。
    fn apply_bucket_adjust(&mut self, adjust: BucketAdjust) {
        let tolerance = &mut self.paint_bucket.tolerance;
        match adjust {
            BucketAdjust::ToleranceDown => {
                *tolerance = tolerance.saturating_sub(PaintBucketTool::TOLERANCE_STEP);
            }
            BucketAdjust::ToleranceUp => {
                // `u8` 的容差封顶就是 255，`saturating_add` 已经处理。
                *tolerance = tolerance.saturating_add(PaintBucketTool::TOLERANCE_STEP);
            }
        }
    }

    /// 工具选项栏：工具名 / 提示 / 笔刷配置，按当前工具显示。
    fn sync_options(&mut self) {
        let tool = self.state.borrow().active_tool;
        set_text(&mut self.tree, self.options_tool, tool.label());
        set_text(
            &mut self.tree,
            self.options_size,
            format!("{:.0}px", self.brush.size),
        );
        set_text(
            &mut self.tree,
            self.options_opacity,
            format!("{:.0}%", self.brush.opacity * 100.0),
        );
        set_text(&mut self.tree, self.options_hint, options_hint(tool));
        set_text(
            &mut self.tree,
            self.options_tolerance,
            format!("{}", self.paint_bucket.tolerance),
        );
        set_text(
            &mut self.tree,
            self.gradient_kind_label,
            self.gradient_kind.label(),
        );

        let show_brush = tool_has_brush(tool);
        if self.tree.is_visible(self.options_brush) != Some(show_brush) {
            self.tree.set_visible(self.options_brush, show_brush);
            igui_ui::mark_dirty(&mut self.tree, self.options_brush);
        }
        let show_bucket = tool == ActiveTool::PaintBucket;
        if self.tree.is_visible(self.options_bucket) != Some(show_bucket) {
            self.tree.set_visible(self.options_bucket, show_bucket);
            igui_ui::mark_dirty(&mut self.tree, self.options_bucket);
        }
        let show_gradient = tool == ActiveTool::Gradient;
        if self.tree.is_visible(self.options_gradient) != Some(show_gradient) {
            self.tree.set_visible(self.options_gradient, show_gradient);
            igui_ui::mark_dirty(&mut self.tree, self.options_gradient);
        }
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
        Key::Character('g') | Key::Character('G') => Some(ActiveTool::PaintBucket),
        Key::Character('n') | Key::Character('N') => Some(ActiveTool::Gradient),
        Key::Character('r') | Key::Character('R') => Some(ActiveTool::RectangleFill),
        Key::Character('u') | Key::Character('U') => Some(ActiveTool::FreeFill),
        Key::Character('i') | Key::Character('I') => Some(ActiveTool::Eyedropper),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
