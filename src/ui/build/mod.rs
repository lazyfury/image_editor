//! [`EditorView`] 的构造：建共享状态槽 / 各面板，拼整棵树，再填出视图。
//!
//! `new` 原本 412 行；这里拆成三步：`build_tree`（建面板 + 组装树）、
//! [`finish`]（解析 NodeRef -> NodeId 并填 `EditorView`）。

mod assemble;
mod cells;
mod page;
mod panels;
mod tree;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use igui_components::{Component, Divider, Flex, ListState, NodeRef, Overlays, ResizeHandle};
use igui_core::{Edges, NodeId, Size, ViewportSize};
use igui_scene::SceneTree;
use igui_theme::{space, Theme};
use igui_ui::{MouseFilter, SizeBasis};

use crate::app::state::{ActiveTool, AppState, HistoryAction};
use crate::app::tool_groups::TOOL_GROUPS;
use crate::document::Gradient;
use crate::icons::IconSet;
use crate::renderer::{CpuRenderer, RenderTarget};
use crate::tools::{
    BrushTool, GradientKind, GradientTool, MoveTool, PaintBucketTool, RectangleFillTool,
};

use super::options_bar::{options_bar, BarInputs, GradientOptions, OptionSlots, OptionsRefs};
use super::tabs::{SidebarTab, TabsView};
use super::{
    canvas, history_panel, layer_panel, menu, palette, properties_panel, status_bar, toolbar,
    BrushAdjust, BrushToggle, BucketAdjust, BucketToggle, EditorView, GradientAdjust, Intents,
    Refs, LONG_PRESS_MS, PALETTE_MAX, PALETTE_MIN, PALETTE_WIDTH, SIDEBAR_MIN, SIDEBAR_WIDTH,
    TABS_PANEL_HEIGHT, TABS_PANEL_MAX, TABS_PANEL_MIN,
};

/// 构造 `EditorView`（`EditorView::new` 转交到这里）。
pub(super) fn editor(theme: &'static dyn Theme, state: AppState) -> EditorView {
    let document_size = (state.document.width, state.document.height);
    let state = Rc::new(RefCell::new(state));
    let (tree, _page, wiring, lists) = tree::build_tree(theme, &state);
    assemble::finish(theme, state, document_size, tree, wiring, lists)
}

/// 构造期收集的共享状态槽与 NodeRef；构造完搬进 [`EditorView`]。
struct Wiring {
    message: Rc<RefCell<Option<String>>>,
    /// 回调留下的意图（工具 / 菜单 / 选项栏 / 文件……）。
    intents: Intents,
    tool_faces: Vec<Rc<Cell<ActiveTool>>>,
    long_press: Rc<RefCell<toolbar::LongPress>>,
    palette_width: Rc<Cell<f32>>,
    sidebar_width: Rc<Cell<f32>>,
    active_tab: Rc<Cell<SidebarTab>>,
    tabs_height: Rc<Cell<f32>>,
    layer_count: Rc<Cell<usize>>,
    layer_selected: Rc<Cell<Option<usize>>>,
    /// 图层行的缓存（`Document::rows()` 的结果，面板 / 拖拽 / 自检共用）。
    layer_rows: Rc<RefCell<Vec<crate::document::LayerRow>>>,
    /// 图层缩略图纹理表（面板行绘制用）。
    layer_textures: Rc<RefCell<crate::ui::layer_thumbs::LayerTextures>>,
    history_count: Rc<Cell<usize>>,
    pixel_mode: Rc<Cell<bool>>,
    square_mode: Rc<Cell<bool>>,
    symmetry_mode: Rc<Cell<crate::tools::Symmetry>>,
    bucket_contiguous: Rc<Cell<bool>>,
    gradient: Rc<RefCell<Gradient>>,
    gradient_reverse: Rc<Cell<bool>>,
    menu_refs: Vec<NodeRef>,
    group_refs: Vec<NodeRef>,
    label_refs: Vec<NodeRef>,
    history_refs: Vec<(HistoryAction, NodeRef)>,
    palette_refs: Vec<NodeRef>,
    brush_refs: Vec<(BrushAdjust, NodeRef)>,
    toggle_refs: Vec<(BrushToggle, NodeRef)>,
    pen_refs: Vec<(crate::ui::PenAction, NodeRef)>,
    bucket_refs: Vec<(BucketAdjust, NodeRef)>,
    bucket_toggle_refs: Vec<(BucketToggle, NodeRef)>,
    gradient_refs: Vec<(GradientAdjust, NodeRef)>,
    tab_refs: Vec<(SidebarTab, NodeRef, NodeRef)>,
    icons: Rc<IconSet>,
    refs: Refs,
    options: OptionsRefs,
}

/// 两个列表控件的滚动 / 选中状态。
struct Lists {
    layer_state: ListState,
    history_state: ListState,
}
