//! 解析 NodeRef -> NodeId，填出 [`EditorView`]（`build` 的最后一步）。

use super::*;

pub(super) fn finish(
    theme: &'static dyn Theme,
    state: Rc<RefCell<AppState>>,
    document_size: (u32, u32),
    tree: SceneTree,
    page: NodeId,
    wiring: Wiring,
    lists: Lists,
) -> EditorView {
    let Wiring {
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
        bucket_refs,
        bucket_toggle_refs,
        gradient_refs,
        tab_refs,
        icons,
        refs,
        options,
    } = wiring;
    let Lists {
        layer_state,
        history_state,
    } = lists;
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

    let mut view = EditorView {
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
        options_hardness: options.hardness.get().expect("options hardness mounted"),
        options_preset: options
            .preset_button
            .get()
            .expect("options preset button mounted"),
        options_preset_label: options.preset.get().expect("options preset label mounted"),
        options_opacity: options.opacity.get().expect("options opacity mounted"),
        options_bucket: options.bucket.get().expect("options bucket mounted"),
        options_gradient: options.gradient.get().expect("options gradient mounted"),
        options_tolerance: options.tolerance.get().expect("options tolerance mounted"),
        options_hint: options.hint.get().expect("options hint mounted"),
        brush_request,
        brush_preset_open,
        brush_preset_request: Rc::new(Cell::new(None)),
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
        rename: None,
        props_name: refs.props_name.get().expect("props name mounted"),
        props_detail: refs.props_detail.get().expect("props detail mounted"),
        props_geometry: refs.props_geometry.get().expect("props geometry mounted"),
        last_active: None,
    };
    view.update();
    view
}
