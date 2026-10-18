use super::*;
use crate::canvas::DOCUMENT_TEXTURE;
use crate::document::{Color, PixelBuffer, Point};
use crate::renderer::sample_pixel;
use igui_core::{InputEvent, Key, NodeId, PointerButton, Size, Vec2, ViewportSize};
use igui_render::{DrawCommand, PaintContext};

fn viewport() -> ViewportSize {
    ViewportSize::new(Size::new(1280.0, 800.0))
}

fn click(view: &mut EditorView, position: Vec2) {
    igui_ui::handle_input(
        &mut view.tree,
        &InputEvent::PointerDown {
            position,
            button: PointerButton::Left,
        },
    );
    igui_ui::handle_input(
        &mut view.tree,
        &InputEvent::PointerUp {
            position,
            button: PointerButton::Left,
        },
    );
}

fn label_text(view: &EditorView, id: NodeId) -> String {
    igui_ui::content(&view.tree, id)
        .and_then(|content| content.as_text())
        .map(str::to_string)
        .expect("expected a label")
}

/// 点图层面板的「重命名」按钮。
fn click_rename(view: &mut EditorView) {
    let center = view
        .rename_button_center()
        .expect("图层面板应有「重命名」按钮");
    click(view, center);
}

#[test]
fn keyboard_shortcut_selects_a_tool() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    assert!(view
        .event(&InputEvent::KeyDown {
            key: Key::Character('b'),
        })
        .is_handled());
    view.update();
    assert_eq!(view.active_tool(), ActiveTool::Brush);
    assert_eq!(label_text(&view, view.tool_label), "画笔工具");
}

#[test]
fn clicking_a_group_button_activates_its_face() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 默认脸是矩形选择：点选区组按钮激活它。
    let center = view
        .tool_center(ActiveTool::RectangleSelect)
        .expect("selection group");
    click(&mut view, center);
    view.update();
    assert_eq!(view.active_tool(), ActiveTool::RectangleSelect);

    // 按 L 选套索，脸跟着变；再点同一个组按钮就激活套索。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('l'),
    });
    view.update();
    assert_eq!(view.tool_face(3), Some(ActiveTool::LassoSelect));
    click(&mut view, center);
    view.update();
    assert_eq!(view.active_tool(), ActiveTool::LassoSelect);
    assert_eq!(label_text(&view, view.tool_label), "自由选区");
}

fn menu_index(name: &str) -> usize {
    crate::ui::menu::MENUS
        .iter()
        .position(|candidate| *candidate == name)
        .expect("menu exists")
}

/// 走 `EditorView::event` 的一次点击（覆盖层参与路由，与真实鼠标一致）。
fn click_event(view: &mut EditorView, position: Vec2) {
    view.event(&InputEvent::PointerDown {
        position,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position,
        button: PointerButton::Left,
    });
}

#[test]
fn clicking_a_menu_title_opens_its_dropdown() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let help = menu_index("帮助");
    let center = view.menu_center(help).expect("help menu button");
    click_event(&mut view, center);
    view.update();
    assert!(view.menu_open(), "点菜单标题应打开下拉");
}

#[test]
fn clicking_another_menu_title_switches_in_one_click() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let file = menu_index("文件");
    let edit = menu_index("编辑");
    let center = view.menu_center(file).expect("file menu button");
    click_event(&mut view, center);
    view.update();
    assert_eq!(view.open_menu_index(), Some(file));
    // 一次点击就切到「编辑」，而不是先关再点。
    let center = view.menu_center(edit).expect("edit menu button");
    click_event(&mut view, center);
    view.update();
    assert_eq!(view.open_menu_index(), Some(edit));
    assert!(view.menu_open());
}

#[test]
fn clicking_the_open_menu_title_closes_it() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let help = menu_index("帮助");
    let center = view.menu_center(help).expect("help menu button");
    click_event(&mut view, center);
    view.update();
    assert!(view.menu_open());
    click_event(&mut view, center);
    view.update();
    assert!(!view.menu_open(), "点同一标题应关闭菜单");
}

#[test]
fn escape_closes_an_open_menu() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let help = menu_index("帮助");
    let center = view.menu_center(help).expect("help menu button");
    click_event(&mut view, center);
    view.update();
    assert!(view.menu_open());
    view.event(&InputEvent::KeyDown { key: Key::Escape });
    assert!(!view.menu_open(), "Esc 应关闭菜单");
}

#[test]
fn the_status_bar_survives_a_relayout_without_extra_text_churn() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    // 首次布局会做适配，状态栏文字随之变化；先让它稳定下来。
    view.layout(viewport());
    view.update();
    view.layout(viewport());
    let before = igui_ui::layout_count(&view.tree);
    view.update();
    view.layout(viewport());
    assert_eq!(
        igui_ui::layout_count(&view.tree),
        before,
        "稳定后的一帧不该触发重排"
    );
}

fn paint_commands(view: &EditorView) -> Vec<DrawCommand> {
    let mut ctx = PaintContext::new();
    view.paint(&mut ctx);
    ctx.into_draw_list().into_commands()
}

#[test]
fn palette_swatches_wrap_within_the_panel() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let c0 = view.palette_swatch_center(0).expect("swatch 0");
    let c1 = view.palette_swatch_center(1).expect("swatch 1");
    let last = view.palette_swatch_center(15).expect("swatch 15");
    assert!((c0.y - c1.y).abs() < 0.5, "同一行的前两个 y 应相同");
    assert!(last.y > c0.y + 1.0, "最后一个色块应换到下面的行");
    // 每行不超过面板宽度。
    assert!(c1.x > c0.x, "同一行从左往右排");
}

#[test]
fn clicking_a_palette_swatch_sets_the_foreground() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    view.layout(viewport());
    assert_eq!(view.foreground(), Color::BLACK, "默认前景是黑");
    assert!(view.palette_swatch_count() >= 16);

    // SWATCHES[4] = (255, 0, 0)。
    let center = view.palette_swatch_center(4).expect("swatch mounted");
    view.event(&InputEvent::PointerDown {
        position: center,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: center,
        button: PointerButton::Left,
    });
    view.update();
    assert_eq!(view.foreground(), Color::RED);
}

#[test]
fn dragging_the_picker_sets_the_foreground() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    view.layout(viewport());
    assert_eq!(view.foreground(), Color::BLACK, "默认前景是黑");

    let center = view.palette_picker_center().expect("picker mounted");
    view.event(&InputEvent::PointerDown {
        position: center,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: center,
        button: PointerButton::Left,
    });
    view.update();
    let fg = view.foreground();
    assert_ne!(fg, Color::BLACK, "取色器应改前景色");
    assert!(
        fg.r > fg.g && fg.g == fg.b,
        "中心应是色相 0 / s=v=0.5 的暗红，得到 {fg:?}"
    );
}

#[test]
fn the_history_panel_tracks_the_command_count() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.update();
    assert_eq!(view.history_rows(), 1, "空历史只有「当前」一行");
    paint_a_stroke(&mut view);
    view.update();
    assert_eq!(view.history_rows(), 2, "一笔之后多一行");
}

#[test]
fn the_options_bar_adjusts_the_brush_size() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // Select the brush so its config is shown in the options bar.
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    view.layout(viewport());
    let before = view.brush_size();
    let center = view
        .brush_adjust_center(BrushAdjust::SizeUp)
        .expect("size + button");
    view.event(&InputEvent::PointerDown {
        position: center,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: center,
        button: PointerButton::Left,
    });
    view.update();
    assert_eq!(view.brush_size(), before + 1.0);
}

#[test]
fn the_pixel_mode_toggles_flip_the_brush() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    view.layout(viewport());

    // 默认：像素模式开、方形开。
    assert!(view.pixel_mode());
    assert!(view.square_mode());
    assert!(view.brush_hard());
    assert_eq!(view.brush_shape(), BrushShape::Square);

    for toggle in [BrushToggle::Hard, BrushToggle::Square] {
        click_toggle(&mut view, toggle);
    }
    assert!(!view.pixel_mode());
    assert!(!view.square_mode());
    // 开关同步进了画笔。
    assert!(!view.brush_hard());
    assert_eq!(view.brush_shape(), BrushShape::Round);

    for toggle in [BrushToggle::Hard, BrushToggle::Square] {
        click_toggle(&mut view, toggle);
    }
    assert!(view.pixel_mode() && view.square_mode());
}

fn click_toggle(view: &mut EditorView, toggle: BrushToggle) {
    let center = view.brush_toggle_center(toggle).expect("toggle button");
    view.event(&InputEvent::PointerDown {
        position: center,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: center,
        button: PointerButton::Left,
    });
    view.update();
}

#[test]
fn dragging_the_sidebar_handle_resizes_the_sidebar() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let before = view.sidebar_width();
    let start = view.sidebar_handle_center().expect("sidebar handle");
    // The sidebar is on the right, so dragging the handle left grows it.
    let end = start - Vec2::new(40.0, 0.0);
    view.event(&InputEvent::PointerDown {
        position: start,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove { position: end });
    view.event(&InputEvent::PointerUp {
        position: end,
        button: PointerButton::Left,
    });
    view.layout(viewport());
    assert!(
        view.sidebar_width() > before,
        "{} should have grown from {before}",
        view.sidebar_width()
    );
}

#[test]
fn dragging_the_palette_handle_resizes_it() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());

    let before = view.palette_width();
    let start = view.palette_handle_center().expect("palette handle");
    let end = start + Vec2::new(24.0, 0.0);
    view.event(&InputEvent::PointerDown {
        position: start,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove { position: end });
    view.event(&InputEvent::PointerUp {
        position: end,
        button: PointerButton::Left,
    });
    view.layout(viewport());
    assert!(view.palette_width() > before, "调色盘面板应变宽");
}

#[test]
fn a_layer_panel_edit_is_undoable() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 走和面板按钮一样的路径：AppState::execute + LayerMetaCommand。
    {
        let mut state = view.state.borrow_mut();
        let id = state.document.active_layer().unwrap().id;
        let before = crate::document::LayerMetaCommand::capture(&state.document);
        state.document.set_layer_visible(id, false);
        let after = crate::document::LayerMetaCommand::capture(&state.document);
        state.execute(Box::new(crate::document::LayerMetaCommand::new(
            before,
            after,
            "显示/隐藏",
        )));
    }
    view.update();
    assert!(view.can_undo());
    assert!(view.undo());
    let visible = view.state.borrow().document.layers[0].visible;
    assert!(visible, "撤销后图层恢复可见");
}

#[test]
fn the_active_tool_button_paints_the_selection_color() {
    let theme = crate::theme::editor_theme(false);
    let mut view = EditorView::new(theme, AppState::default());
    view.layout(viewport());
    let selection = theme.palette().selection;
    assert!(
        paint_commands(&view).iter().any(|command| matches!(
            command,
            DrawCommand::FillRoundedRect { paint, .. } if paint.color == selection
        )),
        "当前工具的按钮应画出 selection 高亮"
    );
}

/// 标签条只占顶部一条，不盖住编辑器。
#[test]
fn the_tab_strip_stays_at_the_top() {
    let mut tabs = WorkspaceTabs::new(crate::theme::editor_theme(false));
    tabs.set_tabs(
        &[WorkspaceTab {
            name: "A".into(),
            dirty: false,
        }],
        0,
    );
    tabs.layout(viewport());
    let center = tabs.tab_center(0).expect("标签");
    assert!(center.y < 40.0, "标签条应贴顶，实际 y = {}", center.y);
}

/// 标签条下方的点击应透传给编辑器，工具能切换。
#[test]
fn a_click_on_the_editor_toolbar_reaches_it_through_the_tab_strip() {
    let theme = crate::theme::editor_theme(false);
    let mut editor = EditorView::new(theme, AppState::default());
    editor.set_top_inset(TAB_STRIP_HEIGHT);
    let mut tabs = WorkspaceTabs::new(theme);
    tabs.set_tabs(
        &[WorkspaceTab {
            name: "A".into(),
            dirty: false,
        }],
        0,
    );
    editor.layout(viewport());
    tabs.layout(viewport());
    let center = editor
        .tool_center(ActiveTool::RectangleSelect)
        .expect("选区组按钮");
    for event in [
        InputEvent::PointerDown {
            position: center,
            button: PointerButton::Left,
        },
        InputEvent::PointerUp {
            position: center,
            button: PointerButton::Left,
        },
    ] {
        if !tabs.event(&event).is_handled() {
            editor.event(&event);
        }
    }
    editor.update();
    assert_eq!(
        editor.active_tool(),
        ActiveTool::RectangleSelect,
        "标签条下方的点击应到编辑器"
    );
}

/// 顶部预留 32px（工作区标签条）后，编辑器内容仍然绘制。
#[test]
fn the_editor_still_paints_after_a_top_inset() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.set_top_inset(32.0);
    view.layout(viewport());
    let mut menu_y = None;
    for command in paint_commands(&view) {
        if let DrawCommand::DrawText { text, position, .. } = command {
            if text == "文件" {
                menu_y = Some(position.y);
            }
        }
    }
    assert!(
        menu_y.is_some_and(|y| y > 32.0),
        "页面应下移 32px，菜单 y = {menu_y:?}"
    );
}

/// 文档图像只在画布区域的裁剪里画一次，且在所有 UI 之前（不会盖住面板）。
#[test]
fn the_document_is_clipped_to_the_canvas_area_and_painted_once() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let commands = paint_commands(&view);
    let area = view.canvas_area_rect().expect("画布区域");

    let draws: Vec<usize> = commands
            .iter()
            .enumerate()
            .filter(|(_, command)| {
                matches!(command, DrawCommand::DrawImage { texture, .. } if *texture == DOCUMENT_TEXTURE)
            })
            .map(|(index, _)| index)
            .collect();
    assert_eq!(draws.len(), 1, "文档图像只该画一次：{draws:?}");

    let index = draws[0];
    assert!(
        matches!(commands.get(index - 1), Some(DrawCommand::ClipRect(rect)) if *rect == area),
        "文档图像应紧跟画布区域的 ClipRect：{:?}",
        commands.get(index - 1)
    );
    assert!(
        matches!(commands.get(index - 2), Some(DrawCommand::Save)),
        "裁剪块应以 Save 开头：{:?}",
        commands.get(index - 2)
    );
}

#[test]
fn transparent_document_pixels_show_the_checkerboard() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 默认文档带白色「背景」图层：棋盘格被盖住。
    view.mark_texture_dirty();
    let covered = view.take_texture_upload().expect("应产出合成结果");
    assert_eq!(covered.get_pixel(0, 0), Color::WHITE);

    // 隐藏背景图层后，透明区域应露出棋盘格。
    let background = view.state.borrow().document.layers[0].id;
    view.state
        .borrow_mut()
        .document
        .set_layer_visible(background, false);
    view.mark_texture_dirty();
    let revealed = view.take_texture_upload().expect("应产出合成结果");
    assert_eq!(revealed.get_pixel(0, 0), crate::canvas::color_at(0, 0));
    assert_eq!(revealed.get_pixel(8, 0), crate::canvas::color_at(8, 0));
}

#[test]
fn the_checkerboard_can_be_toggled_off() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 隐藏背景图层，让透明区域露出来。
    let background = view.state.borrow().document.layers[0].id;
    view.state
        .borrow_mut()
        .document
        .set_layer_visible(background, false);
    view.mark_texture_dirty();
    let shown = view.take_texture_upload().expect("应产出合成结果");
    assert_eq!(shown.get_pixel(0, 0), crate::canvas::color_at(0, 0));

    view.apply_menu_action(super::menu::MenuAction::ToggleCheckerboard);
    let hidden = view.take_texture_upload().expect("应产出合成结果");
    assert_eq!(hidden.get_pixel(0, 0), Color::TRANSPARENT, "关掉后保持透明");
}

#[test]
fn the_pixel_grid_shows_only_when_zoomed_in_and_in_pixel_mode() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let line_count = |view: &EditorView| {
        paint_commands(view)
            .iter()
            .filter(|command| matches!(command, DrawCommand::Line { .. }))
            .count()
    };

    // 像素模式关：放大也不画网格。
    view.pixel_mode.set(false);
    view.state.borrow_mut().canvas.zoom = 12.0;
    view.state.borrow_mut().canvas.offset = Vec2::new(120.0, 120.0);
    let without = line_count(&view);

    // 打开像素模式：多出网格线。
    view.pixel_mode.set(true);
    let with = line_count(&view);
    assert!(
        with > without,
        "放大 + 像素模式应画网格（{with} vs {without}）"
    );

    // 缩到阈值以下：回到没有网格。
    view.state.borrow_mut().canvas.zoom = 1.0;
    assert_eq!(line_count(&view), without, "缩小后不应画网格");
}

#[test]
fn the_first_layout_fits_and_centers_the_document() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let area = view.canvas_area_rect().expect("canvas area");
    let camera = view.canvas_camera();
    assert!(camera.zoom > 0.0);
    let center = camera.document_to_screen(Vec2::new(64.0, 64.0));
    assert!(
        (center.x - area.center().x).abs() < 0.5,
        "center = {center:?}"
    );
    assert!(
        (center.y - area.center().y).abs() < 0.5,
        "center = {center:?}"
    );
}

#[test]
fn the_wheel_zooms_around_the_pointer() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let point = view.canvas_area_rect().expect("canvas area").center();
    let before = view.canvas_camera();
    let anchor = before.screen_to_document(point);

    assert!(view
        .event(&InputEvent::Wheel {
            position: point,
            delta: Vec2::new(0.0, -40.0),
        })
        .is_handled());

    let after = view.canvas_camera();
    assert!(after.zoom > before.zoom, "向上滚应放大");
    assert!(
        (after.screen_to_document(point) - anchor).length_squared() < 0.25,
        "缩放应锚定在指针下的文档点"
    );
}

#[test]
fn middle_drag_pans_the_canvas() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let start = view.canvas_area_rect().expect("canvas area").center();
    let before = view.canvas_camera().offset;

    assert!(view
        .event(&InputEvent::PointerDown {
            position: start,
            button: PointerButton::Middle,
        })
        .is_handled());
    view.event(&InputEvent::PointerMove {
        position: start + Vec2::new(20.0, -10.0),
    });
    view.event(&InputEvent::PointerUp {
        position: start + Vec2::new(20.0, -10.0),
        button: PointerButton::Middle,
    });

    assert_eq!(view.canvas_camera().offset, before + Vec2::new(20.0, -10.0));
}

#[test]
fn the_zoom_shortcuts_change_the_camera() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let before = view.canvas_camera().zoom;
    assert!(view
        .event(&InputEvent::KeyDown {
            key: Key::Character('+'),
        })
        .is_handled());
    assert!(view.canvas_camera().zoom > before);
    assert!(view
        .event(&InputEvent::KeyDown {
            key: Key::Character('0'),
        })
        .is_handled());
    assert_eq!(view.canvas_camera().zoom, 1.0);
}

/// 切到某个标签页（等同点标签：写共享状态后 `update` + `layout`）。
fn select_tab(view: &mut EditorView, tab: SidebarTab) {
    view.active_tab.set(tab);
    view.update();
    view.layout(viewport());
}

fn frame_texts(view: &EditorView) -> Vec<String> {
    paint_commands(view)
        .iter()
        .filter_map(|command| match command {
            DrawCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn adding_a_layer_refreshes_the_list_and_recomposites() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.state.borrow_mut().document.add_layer("新图层");
    view.update();
    view.layout(viewport());

    assert_eq!(view.layer_count.get(), 2);
    assert!(
        frame_texts(&view)
            .iter()
            .any(|text| text.contains("新图层")),
        "图层列表应出现新图层"
    );
    assert!(view.take_texture_upload().is_some(), "改动后要重传纹理");
}

#[test]
fn the_properties_panel_follows_the_active_layer() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 新图层成为当前图层，属性面板应显示它的名字。
    view.state.borrow_mut().document.add_layer("上层");
    select_tab(&mut view, SidebarTab::Properties);
    assert!(frame_texts(&view).iter().any(|text| text.contains("上层")));
}

#[test]
fn the_properties_panel_shows_layer_geometry() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let id = view.state.borrow().document.active_layer().unwrap().id;
    view.state
        .borrow_mut()
        .document
        .set_layer_position(id, Point::new(3, -2));
    select_tab(&mut view, SidebarTab::Properties);
    let texts = frame_texts(&view);
    assert!(
        texts.iter().any(|text| text.contains("偏移 (3, -2)")),
        "texts = {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains("缓冲 128×128")),
        "texts = {texts:?}"
    );
}

#[test]
fn the_crop_action_shrinks_a_moved_layer_back_to_the_document() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let id = view.state.borrow().document.active_layer().unwrap().id;
    {
        let mut state = view.state.borrow_mut();
        state.document.set_layer_position(id, Point::new(10, 0));
        state.document.ensure_layer_covers_document(id);
    }
    assert!(view.state.borrow().document.layer(id).unwrap().pixels.width > 128);

    view.apply_menu_action(super::menu::MenuAction::CropLayerToDocument);
    {
        let state = view.state.borrow();
        let layer = state.document.layer(id).unwrap();
        assert_eq!((layer.pixels.width, layer.pixels.height), (128, 128));
        assert_eq!(layer.position, Point::ZERO);
    }
    assert!(view.take_texture_upload().is_some(), "裁剪后要重合成");
}

#[test]
fn renaming_the_active_layer_uses_text_input() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 点真实按钮：它把自己的 `NodeId` 写进请求格，兼作浮层锚点。
    click_rename(&mut view);
    view.update();
    view.layout(viewport());

    let edit = view
        .rename
        .as_ref()
        .expect("应打开重命名浮层")
        .edit
        .borrow()
        .clone()
        .expect("浮层内容应建好输入框");

    // 浮层锚在按钮上方，整块落在视口里（否则输入框点不到）。
    let overlay = view.rename.as_ref().expect("浮层已开").overlay;
    let popover = view.overlays.rect(overlay).expect("浮层应已布局");
    let screen = viewport().logical_size();
    assert!(
        popover.origin.x >= 0.0
            && popover.origin.y >= 0.0
            && popover.origin.x + popover.size.width <= screen.width
            && popover.origin.y + popover.size.height <= screen.height,
        "浮层应完整落在视口里：{popover:?}"
    );

    edit.borrow_mut().select_all();
    view.event(&InputEvent::TextInput {
        text: "Hero".into(),
    });
    view.event(&InputEvent::KeyDown { key: Key::Enter });

    assert!(view.rename.is_none(), "Enter 结束重命名");
    let name = view
        .state
        .borrow()
        .document
        .active_layer()
        .map(|layer| layer.name.clone());
    assert_eq!(name.as_deref(), Some("Hero"));
}

/// 「确定」按钮只写共享格；`update` 取走后走同一条提交路径。
#[test]
fn the_rename_confirm_request_commits() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    click_rename(&mut view);
    view.update();
    view.layout(viewport());

    {
        let rename = view.rename.as_ref().expect("应打开重命名浮层");
        rename
            .edit
            .borrow()
            .as_ref()
            .unwrap()
            .borrow_mut()
            .set_text("Hero");
        rename.confirm.set(true);
    }
    view.update();

    assert!(view.rename.is_none(), "确定后浮层关闭");
    assert_eq!(
        view.state.borrow().document.active_layer().unwrap().name,
        "Hero"
    );
}

#[test]
fn escape_cancels_a_rename() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    click_rename(&mut view);
    view.update();
    view.layout(viewport());
    assert!(view.rename.is_some(), "应打开重命名浮层");

    // Esc 由浮层自己吞掉（`dismiss_on_escape`），下一帧 `update` 清掉记录。
    view.event(&InputEvent::KeyDown { key: Key::Escape });
    view.update();
    assert!(view.rename.is_none());
    assert_eq!(
        view.state.borrow().document.active_layer().unwrap().name,
        "背景",
        "取消后名字不变"
    );
}

#[test]
fn a_left_drag_with_the_brush_paints_the_active_layer() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();

    let point = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));
    view.event(&InputEvent::PointerDown {
        position: point,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove {
        position: point + Vec2::new(40.0, 0.0),
    });
    view.event(&InputEvent::PointerUp {
        position: point + Vec2::new(40.0, 0.0),
        button: PointerButton::Left,
    });
    view.update();

    let painted = view
        .state
        .borrow()
        .document
        .active_layer()
        .unwrap()
        .pixels
        .get_pixel(64, 64);
    assert_eq!(painted, Color::BLACK);
    assert!(view.take_texture_upload().is_some(), "画笔改动要重传纹理");
}

#[test]
fn the_eraser_clears_instead_of_painting() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 走真实路径：先按 E 选橡皮，再点工具栏的橡皮组按钮。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('e'),
    });
    view.update();
    let eraser = view.tool_center(ActiveTool::Eraser).expect("brush group");
    view.event(&InputEvent::PointerDown {
        position: eraser,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: eraser,
        button: PointerButton::Left,
    });
    view.update();
    assert_eq!(view.active_tool(), ActiveTool::Eraser);

    let point = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));
    view.event(&InputEvent::PointerDown {
        position: point,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: point,
        button: PointerButton::Left,
    });

    let alpha = view
        .state
        .borrow()
        .document
        .active_layer()
        .unwrap()
        .pixels
        .get_pixel(64, 64)
        .a;
    assert_eq!(alpha, 0, "橡皮把背景擦透明");
}

#[test]
fn erasing_removes_a_brush_stroke() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    paint_a_stroke(&mut view);
    let point = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));
    assert_eq!(
        view.state
            .borrow()
            .document
            .active_layer()
            .unwrap()
            .pixels
            .get_pixel(64, 64),
        Color::BLACK,
        "画笔应在 (64,64) 留下黑色"
    );

    // 先按 E 选橡皮，再点工具栏的橡皮组按钮。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('e'),
    });
    view.update();
    let eraser = view.tool_center(ActiveTool::Eraser).expect("brush group");
    view.event(&InputEvent::PointerDown {
        position: eraser,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: eraser,
        button: PointerButton::Left,
    });
    view.update();
    view.event(&InputEvent::PointerDown {
        position: point,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: point,
        button: PointerButton::Left,
    });

    let alpha = view
        .state
        .borrow()
        .document
        .active_layer()
        .unwrap()
        .pixels
        .get_pixel(64, 64)
        .a;
    assert_eq!(alpha, 0, "橡皮应把这一笔擦掉（alpha=0）");
}

/// 用画笔在画布中央画一笔（屏幕坐标 -> 文档坐标由视图换算）。
fn paint_a_stroke(view: &mut EditorView) {
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    let point = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));
    view.event(&InputEvent::PointerDown {
        position: point,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove {
        position: point + Vec2::new(40.0, 0.0),
    });
    view.event(&InputEvent::PointerUp {
        position: point + Vec2::new(40.0, 0.0),
        button: PointerButton::Left,
    });
    view.update();
}

#[test]
fn a_brush_stroke_is_exactly_one_undo_step() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    paint_a_stroke(&mut view);

    assert!(view.can_undo());
    assert_eq!(view.history_len(), (1, 0));
    assert!(view.undo());
    assert_eq!(view.history_len(), (0, 1));
    assert!(view.can_redo());
    assert!(view.redo());
    assert_eq!(view.history_len(), (1, 0));
}

#[test]
fn undo_restores_the_stroke_and_redo_reapplies_it_through_the_view() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    paint_a_stroke(&mut view);
    view.mark_texture_dirty();
    let painted = view.take_texture_upload().expect("脏文档应产出合成结果");
    assert_eq!(painted.get_pixel(64, 64), Color::BLACK);

    view.undo();
    let undone = view.take_texture_upload().expect("撤销后应重合成");
    assert_eq!(undone.get_pixel(64, 64), Color::WHITE);

    view.redo();
    let redone = view.take_texture_upload().expect("重做后应重合成");
    assert_eq!(redone.get_pixel(64, 64), Color::BLACK);
}

#[test]
fn undo_and_redo_with_empty_history_are_no_ops() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    assert!(!view.can_undo());
    assert!(!view.undo());
    view.update();
    assert!(label_text(&view, view.message_label).contains("没有可撤销"));
    assert!(!view.redo());
    view.update();
    assert!(label_text(&view, view.message_label).contains("没有可重做"));
}

#[test]
fn the_history_toolbar_buttons_undo_and_redo_a_stroke() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    paint_a_stroke(&mut view);
    assert_eq!(view.history_len(), (1, 0));

    let undo = view.history_center(HistoryAction::Undo).expect("撤销按钮");
    click(&mut view, undo);
    view.update();
    assert_eq!(view.history_len(), (0, 1));
    assert!(label_text(&view, view.message_label).contains("撤销"));

    let redo = view.history_center(HistoryAction::Redo).expect("重做按钮");
    click(&mut view, redo);
    view.update();
    assert_eq!(view.history_len(), (1, 0));
}

#[test]
fn the_icon_pack_covers_the_toolbar_icons() {
    let view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    let needed = ActiveTool::ALL.len() + HistoryAction::ALL.len();
    assert!(view.icon_count() >= needed, "图标包应覆盖工具栏图标");
}

/// 每个工具的图标名都能在 vendor 的图标包里解析（包括自制的 `gradient`）。
#[test]
fn every_tool_icon_name_resolves() {
    let icons = crate::icons::IconSet::load();
    for tool in ActiveTool::ALL {
        assert!(
            icons.document(tool.icon()).is_some(),
            "缺少图标 `{}`",
            tool.icon()
        );
    }
}

#[test]
fn icons_draw_into_the_painted_frame() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 图标用圆头 / 圆角，会发出 FillCircle；普通 UI 不画圆。
    assert!(
        paint_commands(&view)
            .iter()
            .any(|command| matches!(command, DrawCommand::FillCircle { .. })),
        "图标应发出 FillCircle"
    );
}

#[test]
fn the_toolbar_packs_its_buttons_tightly() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let first = view.tool_group_center(0).expect("first group");
    let last = view
        .tool_group_center(TOOL_GROUPS.len() - 1)
        .expect("last group");
    let pitch = (last.y - first.y) / (TOOL_GROUPS.len() - 1) as f32;
    // `Flex` 默认带 16px 内边距；忘记清零会让每一项高 32px、间距翻倍。
    assert!(pitch < 60.0, "工具栏每一项的间距应紧凑，实际 {pitch:.1}px");
}

/// 快捷键选中组内工具后，组的「脸」跟着更新（组按钮图标 / 标签同步）。
#[test]
fn a_shortcut_updates_the_group_face() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    assert_eq!(view.tool_face(3), Some(ActiveTool::RectangleSelect));
    assert_eq!(view.tool_face(5), Some(ActiveTool::RectangleFill));
    // L 把选区组的脸换成套索；R 把填充形状组的脸换成矩形填充。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('l'),
    });
    view.update();
    assert_eq!(view.tool_face(3), Some(ActiveTool::LassoSelect));
    view.event(&InputEvent::KeyDown {
        key: Key::Character('r'),
    });
    view.update();
    assert_eq!(view.tool_face(5), Some(ActiveTool::RectangleFill));
    assert_eq!(view.tool_face(1), Some(ActiveTool::Brush));
}

/// 长按组按钮展开工具浮层；短按不展开。
#[test]
fn a_long_press_opens_the_tool_flyout() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.set_long_press_ms(0);
    view.layout(viewport());
    let center = view.tool_center(ActiveTool::Brush).expect("brush group");

    view.event(&InputEvent::PointerDown {
        position: center,
        button: PointerButton::Left,
    });
    view.update();
    assert_eq!(view.tool_flyout_open(), Some(1), "长按应展开画笔组浮层");
    assert!(!view.needs_frame(), "展开后不再需要持续出帧");

    // 松手不算短按，不会切工具。
    view.event(&InputEvent::PointerUp {
        position: center,
        button: PointerButton::Left,
    });
    view.update();
    assert_eq!(view.active_tool(), ActiveTool::Brush);
}

/// 没到阈值的短按只激活脸，不展开浮层。
#[test]
fn a_short_click_does_not_open_the_tool_flyout() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.set_long_press_ms(10_000);
    view.layout(viewport());
    let center = view.tool_center(ActiveTool::Brush).expect("brush group");
    view.event(&InputEvent::PointerDown {
        position: center,
        button: PointerButton::Left,
    });
    view.update();
    assert_eq!(view.tool_flyout_open(), None);
    assert!(view.needs_frame(), "等阈值时应持续出帧");
    view.event(&InputEvent::PointerUp {
        position: center,
        button: PointerButton::Left,
    });
    view.update();
    assert_eq!(view.tool_flyout_open(), None);
    assert_eq!(view.active_tool(), ActiveTool::Brush);
}

// -- Phase 7：导入导出 --------------------------------------------------

/// 测试用唯一临时 PNG 路径。
fn temp_png(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "image_editor_ui_{}_{n}_{tag}.png",
        std::process::id()
    ))
}

#[test]
fn exporting_writes_the_composite_to_the_path() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    paint_a_stroke(&mut view);

    let path = temp_png("export");
    view.export_png_to(&path.to_string_lossy())
        .expect("导出 PNG");

    let exported = crate::io::read_png(&path).expect("导出的 PNG 应能解码");
    assert_eq!(exported.get_pixel(0, 0), Color::WHITE);
    assert_eq!(exported.get_pixel(64, 64), Color::BLACK);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn importing_a_png_adds_a_layer() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let path = temp_png("import");
    crate::io::write_png(&path, &PixelBuffer::filled(2, 2, Color::RED)).unwrap();

    let before = view.state.borrow().document.layers.len();
    view.import_png_from(&path.to_string_lossy())
        .expect("导入 PNG");
    view.update();

    assert_eq!(view.state.borrow().document.layers.len(), before + 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_failed_export_is_an_error() {
    let view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    let missing = std::env::temp_dir()
        .join("image_editor_definitely_missing_dir")
        .join("x.png");
    assert!(view.export_png_to(&missing.to_string_lossy()).is_err());
}

/// 每个文件按钮只留下对应的意图（路径由宿主原生对话框选）。
#[test]
fn file_buttons_emit_their_intent() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    for action in [
        FileRequest::Save,
        FileRequest::SaveAs,
        FileRequest::Open,
        FileRequest::ImportPng,
        FileRequest::ExportPng,
    ] {
        let center = view
            .file_center(action)
            .unwrap_or_else(|| panic!("{action:?} 按钮"));
        click(&mut view, center);
        view.update();
        assert_eq!(
            view.take_file_request(),
            Some(action),
            "{action:?} 应留下意图"
        );
    }
}

// -- Phase 8：移动 / 框选 / 吸管 ---------------------------------------

#[test]
fn the_move_tool_offsets_the_active_layer() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('v'),
    });
    view.update();

    let camera = view.canvas_camera();
    let start = camera.document_to_screen(Vec2::new(64.5, 64.5));
    let end = start + Vec2::new(12.0, -8.0);
    let a = camera.screen_to_document(start);
    let b = camera.screen_to_document(end);
    let expected = Point::new((b.x - a.x).round() as i32, (b.y - a.y).round() as i32);

    view.event(&InputEvent::PointerDown {
        position: start,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove { position: end });
    view.event(&InputEvent::PointerUp {
        position: end,
        button: PointerButton::Left,
    });

    let position = view
        .state
        .borrow()
        .document
        .active_layer()
        .unwrap()
        .position;
    assert_eq!(position, expected);
    assert!(view.take_texture_upload().is_some(), "移动后要重合成");
}

#[test]
fn painting_after_moving_the_layer_lands_under_the_cursor() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    // 移动工具把当前图层右移 10 个文档像素。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('v'),
    });
    view.update();
    let camera = view.canvas_camera();
    let start = camera.document_to_screen(Vec2::new(64.5, 64.5));
    let end = start + Vec2::new(10.0 * camera.zoom, 0.0);
    view.event(&InputEvent::PointerDown {
        position: start,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove { position: end });
    view.event(&InputEvent::PointerUp {
        position: end,
        button: PointerButton::Left,
    });
    view.update();
    assert_eq!(view.active_layer_position(), Some(Point::new(10, 0)));

    // 切画笔，在文档 (64, 64) 落笔：应落在光标下，而不是被位移顶到 (74, 64)。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    let point = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));
    view.event(&InputEvent::PointerDown {
        position: point,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: point,
        button: PointerButton::Left,
    });
    view.update();

    let state = view.state.borrow();
    let layer = state.document.active_layer().unwrap();
    assert_eq!(layer.position, Point::ZERO, "落笔前把位移烘进像素");
    assert_eq!(layer.pixels.get_pixel(64, 64), Color::BLACK);
    // 合成后确实在光标下的文档坐标 (64, 64)，而不是被位移顶到 (74, 64)。
    assert_eq!(sample_pixel(&state.document, 64, 64), Color::BLACK);
    assert_eq!(sample_pixel(&state.document, 74, 64), Color::WHITE);
}

#[test]
fn the_eyedropper_picks_the_composited_color_as_the_foreground() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    paint_a_stroke(&mut view);

    view.event(&InputEvent::KeyDown {
        key: Key::Character('i'),
    });
    view.update();
    let point = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));
    view.event(&InputEvent::PointerDown {
        position: point,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: point,
        button: PointerButton::Left,
    });

    assert_eq!(view.foreground(), Color::BLACK);
    view.update();
    assert!(label_text(&view, view.message_label).contains("吸管"));
}

#[test]
fn a_selection_confines_the_brush_and_escape_clears_it() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();

    // 框一块 54..74 × 54..74。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('m'),
    });
    view.update();
    let start = camera.document_to_screen(Vec2::new(54.0, 54.0));
    let end = camera.document_to_screen(Vec2::new(74.0, 74.0));
    view.event(&InputEvent::PointerDown {
        position: start,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove { position: end });
    view.event(&InputEvent::PointerUp {
        position: end,
        button: PointerButton::Left,
    });
    let selection = view.selection().expect("应产生选区");
    assert!(selection.contains(64, 64));
    assert!(!selection.contains(100, 100));

    // 画笔只在选区里落笔。
    view.event(&InputEvent::KeyDown {
        key: Key::Character('b'),
    });
    view.update();
    let inside = camera.document_to_screen(Vec2::new(64.5, 64.5));
    let outside = camera.document_to_screen(Vec2::new(100.0, 100.0));
    for point in [inside, outside] {
        view.event(&InputEvent::PointerDown {
            position: point,
            button: PointerButton::Left,
        });
        view.event(&InputEvent::PointerUp {
            position: point,
            button: PointerButton::Left,
        });
    }
    let (inside_pixel, outside_pixel) = {
        let state = view.state.borrow();
        let layer = state.document.active_layer().unwrap();
        (
            layer.pixels.get_pixel(64, 64),
            layer.pixels.get_pixel(100, 100),
        )
    };
    assert_eq!(inside_pixel, Color::BLACK, "选区内落笔");
    assert_eq!(outside_pixel, Color::WHITE, "选区外不落笔");

    // Esc 清空选区。
    assert!(view
        .event(&InputEvent::KeyDown { key: Key::Escape })
        .is_handled());
    assert!(view.selection().is_none());
}

/// 魔棒：点一下把容差连通区域变成选区（默认白底 -> 整张文档）。
#[test]
fn the_magic_wand_turns_a_click_into_a_selection() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();

    view.event(&InputEvent::KeyDown {
        key: Key::Character('w'),
    });
    view.update();
    assert_eq!(view.active_tool(), ActiveTool::MagicWand, "W 切到魔棒");

    let point = camera.document_to_screen(Vec2::new(64.5, 64.5));
    view.event(&InputEvent::PointerDown {
        position: point,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerUp {
        position: point,
        button: PointerButton::Left,
    });

    let selection = view.selection().expect("魔棒应产生选区");
    assert!(selection.contains(0, 0), "白底连通 -> 左上角也选中");
    assert!(selection.contains(127, 127), "右下角也选中");
    assert_eq!(selection.area(), 128 * 128);
}

/// 选区边框是黑白相间的线段（马赛克 / marching ants），不是半透明单色线。
#[test]
fn the_selection_border_is_marching_ants() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();

    view.event(&InputEvent::KeyDown {
        key: Key::Character('m'),
    });
    view.update();
    let start = camera.document_to_screen(Vec2::new(54.0, 54.0));
    let end = camera.document_to_screen(Vec2::new(74.0, 74.0));
    view.event(&InputEvent::PointerDown {
        position: start,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove { position: end });
    view.event(&InputEvent::PointerUp {
        position: end,
        button: PointerButton::Left,
    });
    assert!(view.selection().is_some());

    let commands = paint_commands(&view);
    let has_line = |color: igui_core::Color| {
        commands.iter().any(
            |command| matches!(command, DrawCommand::Line { paint, .. } if paint.color == color),
        )
    };
    assert!(has_line(igui_core::Color::BLACK), "选区边框应有黑段");
    assert!(has_line(igui_core::Color::WHITE), "选区边框应有白段");
}

/// 套索：沿自由折线围出的区域被选中，区域外不选。
#[test]
fn a_lasso_selects_only_the_enclosed_region() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();
    view.event(&InputEvent::KeyDown {
        key: Key::Character('l'),
    });
    view.update();

    let path = [
        Vec2::new(20.0, 20.0),
        Vec2::new(60.0, 20.0),
        Vec2::new(60.0, 60.0),
        Vec2::new(20.0, 60.0),
        Vec2::new(20.0, 20.0),
    ];
    let screen = |point: Vec2| camera.document_to_screen(point);
    view.event(&InputEvent::PointerDown {
        position: screen(path[0]),
        button: PointerButton::Left,
    });
    for point in &path[1..] {
        view.event(&InputEvent::PointerMove {
            position: screen(*point),
        });
    }
    view.event(&InputEvent::PointerUp {
        position: screen(path[4]),
        button: PointerButton::Left,
    });

    let selection = view.selection().expect("套索应产生选区");
    assert!(selection.contains(40, 40), "围出的区域应被选中");
    assert!(!selection.contains(80, 80), "区域外不该选中");
}

/// 套索拖动中会实时画折线预览（黑白马赛克线段）。
#[test]
fn the_lasso_preview_is_drawn_while_dragging() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();
    view.event(&InputEvent::KeyDown {
        key: Key::Character('l'),
    });
    view.update();

    let screen = |point: Vec2| camera.document_to_screen(point);
    view.event(&InputEvent::PointerDown {
        position: screen(Vec2::new(20.0, 20.0)),
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove {
        position: screen(Vec2::new(60.0, 20.0)),
    });
    view.event(&InputEvent::PointerMove {
        position: screen(Vec2::new(60.0, 60.0)),
    });

    let commands = paint_commands(&view);
    let has_line = |color: igui_core::Color| {
        commands.iter().any(
            |command| matches!(command, DrawCommand::Line { paint, .. } if paint.color == color),
        )
    };
    assert!(has_line(igui_core::Color::BLACK), "套索预览应有黑段");
    assert!(has_line(igui_core::Color::WHITE), "套索预览应有白段");
}

/// 矩形填充：拖出的矩形被前景色填充，且能撤销。
#[test]
fn a_rectangle_fill_drag_fills_the_rect() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();
    view.event(&InputEvent::KeyDown {
        key: Key::Character('r'),
    });
    view.update();

    let from = camera.document_to_screen(Vec2::new(20.0, 20.0));
    let to = camera.document_to_screen(Vec2::new(60.0, 60.0));
    view.event(&InputEvent::PointerDown {
        position: from,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove { position: to });
    view.event(&InputEvent::PointerUp {
        position: to,
        button: PointerButton::Left,
    });
    view.update();
    view.mark_texture_dirty();
    let frame = view.take_texture_upload().expect("填充后应重合成");
    assert_eq!(frame.get_pixel(40, 40), Color::BLACK, "矩形内被填充");
    assert_eq!(frame.get_pixel(10, 10), Color::WHITE, "矩形外不变");
    assert!(view.can_undo(), "填充应入历史");
}

/// 矩形填充拖动中会画矩形预览。
#[test]
fn the_rectangle_fill_preview_is_drawn_while_dragging() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();
    view.event(&InputEvent::KeyDown {
        key: Key::Character('r'),
    });
    view.update();
    view.event(&InputEvent::PointerDown {
        position: camera.document_to_screen(Vec2::new(20.0, 20.0)),
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove {
        position: camera.document_to_screen(Vec2::new(60.0, 60.0)),
    });

    let commands = paint_commands(&view);
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, DrawCommand::Line { .. })),
        "拖动中应有矩形预览"
    );
}

/// 自由填充：拖出的自由形状内部被前景色填充。
#[test]
fn a_free_fill_traces_and_fills_the_enclosed_area() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();
    view.event(&InputEvent::KeyDown {
        key: Key::Character('u'),
    });
    view.update();

    let path = [
        Vec2::new(20.0, 20.0),
        Vec2::new(60.0, 20.0),
        Vec2::new(60.0, 60.0),
        Vec2::new(20.0, 60.0),
        Vec2::new(20.0, 20.0),
    ];
    let screen = |point: Vec2| camera.document_to_screen(point);
    view.event(&InputEvent::PointerDown {
        position: screen(path[0]),
        button: PointerButton::Left,
    });
    for point in &path[1..] {
        view.event(&InputEvent::PointerMove {
            position: screen(*point),
        });
    }
    view.event(&InputEvent::PointerUp {
        position: screen(path[4]),
        button: PointerButton::Left,
    });
    view.update();
    view.mark_texture_dirty();
    let frame = view.take_texture_upload().expect("填充后应重合成");
    assert_eq!(frame.get_pixel(40, 40), Color::BLACK, "自由形状内部被填");
    assert_eq!(frame.get_pixel(80, 80), Color::WHITE, "形状外不变");
}

/// 渐变工具：拖一条轴把选区（整层）填成前景→背景的渐变，且能撤销。
#[test]
fn a_gradient_drag_fills_the_layer() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let camera = view.canvas_camera();
    view.event(&InputEvent::KeyDown {
        key: Key::Character('n'),
    });
    view.update();
    assert_eq!(view.active_tool(), ActiveTool::Gradient);

    let from = camera.document_to_screen(Vec2::new(0.5, 0.5));
    let to = camera.document_to_screen(Vec2::new(127.5, 0.5));
    view.event(&InputEvent::PointerDown {
        position: from,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove { position: to });
    view.event(&InputEvent::PointerUp {
        position: to,
        button: PointerButton::Left,
    });
    view.update();
    view.mark_texture_dirty();
    let frame = view.take_texture_upload().expect("渐变后应重合成");
    assert_eq!(frame.get_pixel(0, 0), Color::BLACK, "起点是前景色");
    assert_eq!(frame.get_pixel(127, 0), Color::WHITE, "终点是背景色");
    assert!(view.can_undo(), "渐变应入历史");
}

/// 渐变类型从选项栏 `+` 切换。
#[test]
fn the_gradient_kind_cycles_from_the_options_bar() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('n'),
    });
    view.update();
    view.layout(viewport());
    assert_eq!(view.gradient_kind(), GradientKind::Linear);
    let center = view
        .gradient_button_center(GradientAdjust::KindNext)
        .expect("渐变类型 + 按钮");
    click(&mut view, center);
    view.update();
    assert_eq!(view.gradient_kind(), GradientKind::Reflected);
}

/// 默认渐变端点跟随前景 / 背景；「编辑…」打开多节点编辑器。
#[test]
fn the_gradient_endpoints_follow_the_colors_and_the_editor_opens() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('n'),
    });
    view.update();
    view.layout(viewport());
    let stops = view.gradient_stops();
    assert_eq!(stops.first().map(|(_, c)| *c), Some(Color::BLACK));
    assert_eq!(stops.last().map(|(_, c)| *c), Some(Color::WHITE));

    let edit = view.gradient_edit_center().expect("编辑… 按钮");
    click(&mut view, edit);
    view.update();
    assert!(view.gradient_editor_open(), "点编辑应打开编辑器");
}
