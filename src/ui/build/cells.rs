//! 构造期一次性建好的共享格（[`Wiring`]）：意图队列、消息格、各面板的尺寸 /
//! 开关 / 选中行、渐变色带，以及一堆等着收集 `NodeRef` 的空向量。
//!
//! 这里只管**默认值**；谁在什么时候写这些格由各个回调决定。`build_tree` 拿到的
//! 就是已经接好线的一份。

use super::*;
use crate::document::Color;

impl Wiring {
    /// 建好所有共享格与空的收集器（`refs` / `options` 也在这里初始化）。
    pub(super) fn new() -> Self {
        Self {
            message: Rc::new(RefCell::new(None)),
            // 所有回调（菜单 / 工具栏 / 选项栏 / 面板）共用一个意图队列；宿主从
            // 同一个队列里取文件动作，路径由它的原生对话框选。
            intents: Intents::new(),
            tool_faces: TOOL_GROUPS
                .iter()
                .map(|group| Rc::new(Cell::new(group.face())))
                .collect(),
            long_press: Rc::new(RefCell::new(toolbar::LongPress::Idle)),
            palette_width: Rc::new(Cell::new(PALETTE_WIDTH)),
            sidebar_width: Rc::new(Cell::new(SIDEBAR_WIDTH)),
            active_tab: Rc::new(Cell::new(SidebarTab::default())),
            tabs_height: Rc::new(Cell::new(TABS_PANEL_HEIGHT)),
            layer_count: Rc::new(Cell::new(0)),
            layer_selected: Rc::new(Cell::new(None)),
            layer_rows: Rc::new(RefCell::new(Vec::new())),
            layer_textures: Rc::new(RefCell::new(crate::ui::layer_thumbs::LayerTextures::new())),
            preview_viewport: Rc::new(Cell::new(None)),
            preview_filter: Rc::new(Cell::new(crate::canvas::PreviewFilter::default())),
            history_count: Rc::new(Cell::new(0)),
            // 选项栏开关是显示源，`EditorView::update` 每帧把它们写进工具。
            pixel_mode: Rc::new(Cell::new(true)),
            square_mode: Rc::new(Cell::new(true)),
            symmetry_mode: Rc::new(Cell::new(crate::tools::Symmetry::Off)),
            bucket_contiguous: Rc::new(Cell::new(true)),
            gradient: Rc::new(RefCell::new(Gradient::from_colors(
                Color::BLACK,
                Color::WHITE,
            ))),
            gradient_reverse: Rc::new(Cell::new(false)),
            menu_refs: Vec::new(),
            group_refs: Vec::new(),
            label_refs: Vec::new(),
            history_refs: Vec::new(),
            palette_refs: Vec::new(),
            brush_refs: Vec::new(),
            toggle_refs: Vec::new(),
            pen_refs: Vec::new(),
            bucket_refs: Vec::new(),
            bucket_toggle_refs: Vec::new(),
            gradient_refs: Vec::new(),
            tab_refs: Vec::new(),
            // 图标包只加载一次；工具栏的按钮在构建时就把 `Icon` 子组件搭进去。
            icons: Rc::new(IconSet::load()),
            refs: Refs::default(),
            options: OptionsRefs::default(),
        }
    }
}
