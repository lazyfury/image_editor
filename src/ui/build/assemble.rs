//! 解析 NodeRef -> NodeId，填出 [`EditorView`]（`build` 的最后一步）。

use super::*;
use crate::tools::PenTool;
use crate::ui::gradient_editor::GradientState;
use crate::ui::layout::Layout;
use crate::ui::options_bar::OptionsState;

pub(super) fn finish(
    theme: &'static dyn Theme,
    state: Rc<RefCell<AppState>>,
    document_size: (u32, u32),
    tree: SceneTree,
    wiring: Wiring,
    lists: Lists,
) -> EditorView {
    let Wiring {
        message,
        intents,
        workspace_path,
        tool_faces,
        long_press,
        palette_width,
        sidebar_width,
        active_tab,
        tabs_height,
        layer_count,
        layer_selected,
        history_count,
        pixel_mode,
        square_mode,
        symmetry_mode,
        bucket_contiguous,
        gradient,
        gradient_reverse,
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
    } = wiring;
    let Lists {
        layer_state,
        history_state,
    } = lists;
    // `NodeRef` -> `NodeId`：构造期的「控件一定挂上了」不变式就在这里兑现。
    let tool_group_nodes = slots(&group_refs);
    let tool_face_labels = slots(&label_refs);
    let history_nodes = ids(&history_refs);
    let palette_nodes = slots(&palette_refs);
    let file_nodes = ids(&file_refs);
    let menu_nodes = slots(&menu_refs);
    let brush_buttons = ids(&brush_refs);
    let brush_toggles = ids(&toggle_refs);
    let bucket_buttons = ids(&bucket_refs);
    let bucket_toggles = ids(&bucket_toggle_refs);
    let gradient_buttons = ids(&gradient_refs);
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
        menu_bar: refs.menu_bar.get().expect("menu bar mounted"),
        tab_band: refs.tab_band.get().expect("tab band mounted"),
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
        intents: intents.clone(),
        open_tool_group: Cell::new(None),
        long_press_threshold: Duration::from_millis(LONG_PRESS_MS),
        icons,
        history_nodes,
        palette_nodes,
        file_nodes,
        workspace_path,
        menu_nodes,
        open_menu: Cell::new(None),
        overlays: Overlays::new(theme),
        options: OptionsState {
            panel: options.panel.get().expect("options panel mounted"),
            tool: options.tool.get().expect("options tool mounted"),
            brush: options.brush.get().expect("options brush mounted"),
            size: options.size.get().expect("options size mounted"),
            hardness: options.hardness.get().expect("options hardness mounted"),
            smooth: options.smooth.get().expect("options smooth mounted"),
            spacing: options.spacing.get().expect("options spacing mounted"),
            opacity: options.opacity.get().expect("options opacity mounted"),
            symmetry: options.symmetry.get().expect("options symmetry mounted"),
            symmetry_button: options
                .symmetry_button
                .get()
                .expect("symmetry button mounted"),
            preset: options
                .preset_button
                .get()
                .expect("options preset button mounted"),
            preset_label: options.preset.get().expect("options preset label mounted"),
            pen: options.pen_panel.get().expect("options pen mounted"),
            pen_summary: options.pen.get().expect("options pen summary mounted"),
            pen_buttons: pen_refs
                .iter()
                .map(|(action, slot)| (*action, slot.get().expect("pen button mounted")))
                .collect(),
            bucket: options.bucket.get().expect("options bucket mounted"),
            gradient: options.gradient.get().expect("options gradient mounted"),
            tolerance: options.tolerance.get().expect("options tolerance mounted"),
            hint: options.hint.get().expect("options hint mounted"),
            brush_buttons,
            brush_toggles,
            bucket_buttons,
            bucket_toggles,
            pixel_mode,
            square_mode,
            symmetry_mode,
            bucket_contiguous,
        },
        // 默认参数就是 `BrushPreferences::default()`，所以启动时不会立刻写配置。
        saved_preferences: crate::io::BrushPreferences::default(),
        paint_bucket: PaintBucketTool::new(),
        rect_fill: RectangleFillTool::new(),
        gradient: GradientState {
            tool: GradientTool::new(gradient.clone()),
            ramp: gradient,
            kind: GradientKind::default(),
            reverse: gradient_reverse,
            customized: Rc::new(Cell::new(false)),
            selected: Rc::new(Cell::new(0)),
            editor_open: Cell::new(false),
            kind_label: options
                .gradient_kind
                .get()
                .expect("gradient kind label mounted"),
            preview: options
                .gradient_preview
                .get()
                .expect("gradient preview mounted"),
            edit_button: options.gradient_edit.get().expect("gradient edit mounted"),
            buttons: gradient_buttons,
        },
        layout: Layout {
            sidebar_width,
            sidebar: refs.sidebar.get().expect("sidebar mounted"),
            sidebar_handle: refs.sidebar_handle.get().expect("sidebar handle mounted"),
            palette_width,
            palette_panel: refs.palette_panel.get().expect("palette panel mounted"),
            palette_picker: refs.palette_picker.get().expect("palette picker mounted"),
            palette_handle: refs.palette_handle.get().expect("palette handle mounted"),
            active_tab,
            shown_tab: None,
            tabs_height,
            tabs_panel: refs.tabs_panel.get().expect("tabs panel mounted"),
            tabs_handle: refs.tabs_handle.get().expect("tabs handle mounted"),
            tab_buttons,
            tab_contents,
        },
        path_label: refs.path.get().expect("path label mounted"),
        shown_path: None,
        icon_count,
        shown: None,
        pointer: None,
        canvas_area: refs.canvas.get().expect("canvas area mounted"),
        renderer: CpuRenderer,
        render_target: RenderTarget::new(document_size.0, document_size.1),
        texture_dirty: true,
        dirty_region: None,
        show_checkerboard: true,
        fitted: false,
        viewport: ViewportSize::new(Size::new(1280.0, 800.0)),
        pan_last: None,
        brush: BrushTool::paint(),
        move_tool: MoveTool::new(),
        pen: PenTool::new(),
        select_anchor: None,
        modifiers: igui_core::Modifiers::NONE,
        image_clipboard: Rc::new(RefCell::new(None)),
        scale_factor: 1.0,
        merge: crate::canvas::MergeMode::Replace,
        merge_base: None,
        lasso_points: Vec::new(),
        layer_count,
        layer_selected,
        layer_state,
        history_count,
        history_state,
        last_history: None,
        last_revision: u64::MAX,
        rename: None,
        props_name: refs.props_name.get().expect("props name mounted"),
        props_detail: refs.props_detail.get().expect("props detail mounted"),
        props_geometry: refs.props_geometry.get().expect("props geometry mounted"),
        last_active: None,
    };
    view.update();
    view
}

/// `NodeRef` 列表 -> `NodeId` 列表。构造期不变式：这些控件在 `build_tree` 里
/// 一定已经挂到树上了，取不到就是构建代码漏挂了。
fn slots(refs: &[NodeRef]) -> Vec<NodeId> {
    refs.iter()
        .map(|slot| slot.get().expect("控件已挂载"))
        .collect()
}

/// 带键的 `NodeRef` 列表 -> 带键的 `NodeId` 列表（按钮表 / 色块表 / 图标表）。
fn ids<T: Copy>(refs: &[(T, NodeRef)]) -> Vec<(T, NodeId)> {
    refs.iter()
        .map(|(key, slot)| (*key, slot.get().expect("控件已挂载")))
        .collect()
}
