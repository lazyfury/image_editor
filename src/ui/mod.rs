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
mod input;
mod io;
mod layer_panel;
mod layout;
pub mod menu;
mod menus;
mod new_document;
mod options_bar;
mod paint;
mod palette;
mod properties_panel;
mod selection;
mod status_bar;
mod sync;
mod tabs;
mod toolbar;
mod tools;
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
use std::rc::Rc;
use std::time::Duration;

use igui_components::{
    set_text, update_control, Component, Divider, Flex, ListState, NodeRef, Overlays, ResizeHandle,
};
use igui_core::{Edges, InputEvent, Key, NodeId, PointerButton, Rect, Size, Vec2, ViewportSize};
use igui_scene::SceneTree;
use igui_theme::{space, Theme};
use igui_ui::{MouseFilter, SizeBasis, TextMeasurer};

use crate::app::state::{ActiveTool, AppState, HistoryAction};
use crate::app::tool_groups::{group_index, TOOL_GROUPS};
use crate::canvas::{document_to_pixel, paint_backdrop, screen_to_document, CanvasCamera};
use crate::document::{CropLayerCommand, Gradient, LayerId, LayerMetaCommand};
use crate::icons::IconSet;
use crate::renderer::{CpuRenderer, RenderTarget, Renderer};
use crate::tools::{
    BrushShape, BrushTool, GradientKind, GradientTool, MoveTool, PaintBucketTool, PointerEvent,
    RectangleFillTool,
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
