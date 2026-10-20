//! 建各面板并把整棵树拼起来（`build` 的第一步）。

use super::*;

pub(super) fn build_tree(
    theme: &'static dyn Theme,
    state: &Rc<RefCell<AppState>>,
) -> (SceneTree, NodeId, Wiring, Lists) {
    let refs = Refs::default();
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
    let rename_request: Rc<Cell<Option<NodeId>>> = Rc::new(Cell::new(None));
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
    // 「预设」触发按钮只发节点 id；下拉由 `update` 打开（回调拿不到 `&mut self`）。
    let brush_preset_open: Rc<Cell<Option<NodeId>>> = Rc::new(Cell::new(None));
    let pixel_mode = Rc::new(Cell::new(true));
    let square_mode = Rc::new(Cell::new(true));
    let symmetry_mode = Rc::new(Cell::new(crate::tools::Symmetry::Off));
    let mut toggle_refs: Vec<(BrushToggle, NodeRef)> = Vec::new();
    let pen_request: Rc<Cell<Option<crate::ui::PenAction>>> = Rc::new(Cell::new(None));
    let mut pen_refs: Vec<(crate::ui::PenAction, NodeRef)> = Vec::new();
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
        .child(menu::menu_bar(
            theme,
            menu_request.clone(),
            &mut menu_refs,
            &refs.menu_bar,
        ))
        // 菜单栏和选项栏之间给宿主留一条带子画工作区标签条（默认 0 高）。
        .child(
            Flex::row()
                .padding(Edges::ZERO)
                .gap(0.0)
                .mouse_filter(MouseFilter::Ignore)
                .ref_(&refs.tab_band),
        )
        .child(options_bar(
            theme,
            brush_request.clone(),
            brush_preset_open.clone(),
            crate::tools::brush::preset::label(&BrushTool::paint()),
            pixel_mode.clone(),
            square_mode.clone(),
            symmetry_mode.clone(),
            pen_request.clone(),
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
            &mut pen_refs,
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

    let wiring = Wiring {
        pen_request,
        message,
        file_request,
        workspace_path,
        menu_request,
        menu_action,
        tool_faces,
        long_press,
        tool_request,
        palette_width,
        sidebar_width,
        active_tab,
        tabs_height,
        layer_count,
        layer_selected,
        rename_request,
        history_count,
        brush_request,
        brush_preset_open,
        pixel_mode,
        square_mode,
        symmetry_mode,
        bucket_request,
        bucket_contiguous,
        gradient,
        gradient_reverse,
        gradient_request,
        gradient_edit_request,
        menu_refs,
        file_refs,
        group_refs,
        label_refs,
        history_refs,
        palette_refs,
        brush_refs,
        toggle_refs,
        pen_refs,
        bucket_refs,
        bucket_toggle_refs,
        gradient_refs,
        tab_refs,
        icons,
        refs,
        options,
    };
    let lists = Lists {
        layer_state,
        history_state,
    };
    (tree, page, wiring, lists)
}
