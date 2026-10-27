//! 各面板 / 条带：选项栏、工具栏、调色盘、画布、右栏（标签页 + 图层）。
//!
//! 每个 builder 都从 [`Wiring`] 里取自己那几个共享格、把自己的 `NodeRef` 收进
//! `wiring.*_refs`，然后把组件**交给调用方**拼进页面 —— 组件不在这里存，
//! 免得跟 `impl Component` 的匿名类型纠缠。

use super::*;

/// 菜单栏下面那一条：工具选项栏（笔刷参数 / 钢笔动作 / 渐变类型 + 提示）。
pub(super) fn options_strip(theme: &'static dyn Theme, wiring: &mut Wiring) -> impl Component {
    options_bar(
        theme,
        crate::tools::brush::preset::PRESETS[0].name.as_ref(),
        BarInputs {
            intents: wiring.intents.clone(),
            hard: wiring.pixel_mode.clone(),
            square: wiring.square_mode.clone(),
            symmetry: wiring.symmetry_mode.clone(),
            contiguous: wiring.bucket_contiguous.clone(),
            gradient: GradientOptions {
                gradient: wiring.gradient.clone(),
                reverse: wiring.gradient_reverse.clone(),
                intents: wiring.intents.clone(),
            },
        },
        &wiring.options,
        OptionSlots {
            brush: &mut wiring.brush_refs,
            brush_toggles: &mut wiring.toggle_refs,
            pen: &mut wiring.pen_refs,
            bucket: &mut wiring.bucket_refs,
            bucket_toggles: &mut wiring.bucket_toggle_refs,
            gradient: &mut wiring.gradient_refs,
        },
    )
}

/// 中间一行：工具栏 | 调色盘面板 | 画布 | 右栏（标签页 + 图层）。
///
/// 顺带把两个列表控件的 `ListState` 交回去（行池在 `layout` 之后同步）。
pub(super) fn main_row(
    theme: &'static dyn Theme,
    state: &Rc<RefCell<AppState>>,
    wiring: &mut Wiring,
) -> (impl Component, Lists) {
    let toolbar = toolbar::tool_bar(
        theme,
        toolbar::ToolbarWiring {
            state: state.clone(),
            message: wiring.message.clone(),
            icons: wiring.icons.clone(),
            faces: wiring.tool_faces.clone(),
            long_press: wiring.long_press.clone(),
            intents: wiring.intents.clone(),
        },
        &mut wiring.group_refs,
        &mut wiring.label_refs,
        &mut wiring.history_refs,
    );
    let palette_panel = palette::palette_panel(
        theme,
        state.clone(),
        &mut wiring.palette_refs,
        &wiring.refs.palette_picker,
    );
    let (sidebar, lists) = sidebar(theme, state, wiring);
    let row = Flex::row()
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
                        .basis(SizeBasis::Px(wiring.palette_width.get()))
                        .shrink(0.0)
                        .padding(Edges::ZERO)
                        .mouse_filter(MouseFilter::Ignore)
                        .child(palette_panel)
                        .ref_(&wiring.refs.palette_panel),
                )
                .child(
                    ResizeHandle::vertical(theme)
                        .target(wiring.refs.palette_panel.clone())
                        .width(wiring.palette_width.clone())
                        .min(PALETTE_MIN)
                        .max(PALETTE_MAX)
                        .ref_(&wiring.refs.palette_handle),
                ),
        )
        .child(canvas::canvas_area().ref_(&wiring.refs.canvas))
        .child(sidebar);
    (row, lists)
}

/// 右栏：上面是「文件 / 历史 / 属性」标签页，下面是图层列表，中间一条可拖的分隔条。
fn sidebar(
    theme: &'static dyn Theme,
    state: &Rc<RefCell<AppState>>,
    wiring: &mut Wiring,
) -> (impl Component, Lists) {
    let file_panel = file_panel::file_panel(
        theme,
        wiring.intents.clone(),
        &wiring.refs.path,
        &mut wiring.file_refs,
    );
    let layer_list = layer_panel::layer_list(
        theme,
        state.clone(),
        wiring.layer_rows.clone(),
        wiring.layer_count.clone(),
        wiring.layer_selected.clone(),
        wiring.intents.clone(),
    );
    let layer_state = layer_list.state();
    let history_list =
        history_panel::history_list(theme, state.clone(), wiring.history_count.clone());
    let history_state = history_list.state();
    let properties = properties_panel::properties_panel(
        theme,
        &wiring.refs.props_name,
        &wiring.refs.props_detail,
        &wiring.refs.props_geometry,
    );

    // 侧栏标签页：文件 / 历史 / 属性共用一个卡片。按钮与内容容器各留一个
    // `NodeRef`，建完后收进 `tab_buttons` / `tab_contents` 供交互与可见性同步。
    let mut tabs_view = TabsView::new(theme, wiring.active_tab.clone());
    let file_button = NodeRef::new();
    let file_content = NodeRef::new();
    tabs_view = tabs_view.tab(SidebarTab::File, &file_button, &file_content, file_panel);
    wiring
        .tab_refs
        .push((SidebarTab::File, file_button, file_content));
    let history_button = NodeRef::new();
    let history_content = NodeRef::new();
    tabs_view = tabs_view.tab(
        SidebarTab::History,
        &history_button,
        &history_content,
        history_list,
    );
    wiring
        .tab_refs
        .push((SidebarTab::History, history_button, history_content));
    let props_button = NodeRef::new();
    let props_content = NodeRef::new();
    tabs_view = tabs_view.tab(
        SidebarTab::Properties,
        &props_button,
        &props_content,
        properties,
    );
    wiring
        .tab_refs
        .push((SidebarTab::Properties, props_button, props_content));

    let panel = Flex::row()
        .background(theme.background())
        .padding(Edges::ZERO)
        .gap(0.0)
        .child(
            ResizeHandle::vertical(theme)
                .target(wiring.refs.sidebar.clone())
                .width(wiring.sidebar_width.clone())
                .min(SIDEBAR_MIN)
                .invert()
                .ref_(&wiring.refs.sidebar_handle),
        )
        .child(
            Flex::column()
                .basis(SizeBasis::Px(wiring.sidebar_width.get()))
                .shrink(0.0)
                .gap(space::XXXS)
                .padding(Edges::ZERO)
                .mouse_filter(MouseFilter::Ignore)
                .child(
                    tabs_view
                        .basis(SizeBasis::Px(wiring.tabs_height.get()))
                        .shrink(0.0)
                        .ref_(&wiring.refs.tabs_panel),
                )
                .child(
                    ResizeHandle::horizontal(theme)
                        .target(wiring.refs.tabs_panel.clone())
                        .width(wiring.tabs_height.clone())
                        .min(TABS_PANEL_MIN)
                        .max(TABS_PANEL_MAX)
                        .ref_(&wiring.refs.tabs_handle),
                )
                .child(layer_panel::layer_panel(
                    theme,
                    state.clone(),
                    layer_list,
                    wiring.intents.clone(),
                ))
                .ref_(&wiring.refs.sidebar),
        );
    let lists = Lists {
        layer_state,
        history_state,
    };
    (panel, lists)
}
