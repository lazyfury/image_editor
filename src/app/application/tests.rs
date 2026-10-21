//! 宿主级测试：模态 / 标签条 / 保存流程 / 关闭确认（走真实事件与真实视图）。

use super::*;

use std::cell::RefCell;

/// 脚本化的假对话框：`open` / `save` 各一个返回队列。
#[derive(Default)]
struct FakeFileDialog {
    open: RefCell<std::collections::VecDeque<PathBuf>>,
    save: RefCell<std::collections::VecDeque<PathBuf>>,
}

impl FakeFileDialog {
    fn saving(path: PathBuf) -> Self {
        let dialog = Self::default();
        dialog.save.borrow_mut().push_back(path);
        dialog
    }
}

impl FileDialog for FakeFileDialog {
    fn pick_open(&self, _options: &DialogOptions) -> Option<PathBuf> {
        self.open.borrow_mut().pop_front()
    }

    fn pick_save(&self, _options: &DialogOptions) -> Option<PathBuf> {
        self.save.borrow_mut().pop_front()
    }
}

fn logic_with_dialog(dialog: Rc<dyn FileDialog>) -> ImageEditorLogic {
    let theme = crate::theme::editor_theme(false);
    let mut logic = ImageEditorLogic::new(theme, dialog);
    logic
        .workspaces
        .push(Workspace::new(EditorView::new(theme, AppState::default())));
    logic
}

/// 标签条是 chrome，必须画在编辑器之前：菜单下拉属于编辑器浮层，要盖住它。
#[test]
fn the_tab_strip_is_painted_below_the_editor_overlays() {
    use igui_core::{InputEvent, PointerButton, Size, ViewportSize};
    use igui_render::{DrawCommand, PaintContext};

    let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
    // 从主页切到编辑器（正常流程走「新建文档」，这里直接切屏）。
    logic.screen = Screen::Editor;
    let viewport = ViewportSize::new(Size::new(1280.0, 800.0));
    let tabs_state = vec![crate::ui::WorkspaceTab {
        name: "A".into(),
        dirty: false,
    }];
    let workspace = logic.workspaces.active_mut().expect("一个工作区");
    workspace.editor.set_tab_band(crate::ui::TAB_STRIP_HEIGHT);
    workspace.editor.layout(viewport);
    let tabs_top = workspace.editor.menu_bar_rect().expect("菜单栏").bottom();
    logic.tabs.set_tabs(&tabs_state, 0);
    logic.tabs.set_top(tabs_top);
    logic.tabs.layout(viewport);

    // 打开「文件」菜单（编辑器的浮层），下拉会跨过标签条那条带子。
    let title = workspace.editor.menu_center(0).expect("文件菜单标题");
    for event in [
        InputEvent::PointerDown {
            position: title,
            button: PointerButton::Left,
        },
        InputEvent::PointerUp {
            position: title,
            button: PointerButton::Left,
        },
    ] {
        workspace.editor.event(&event);
    }
    workspace.editor.update();
    workspace.editor.layout(viewport);

    let mut paint = PaintContext::new();
    paint_screen(
        &logic.screen,
        &logic.workspaces,
        &logic.tabs,
        logic.modal.as_ref(),
        logic.confirm.as_ref(),
        &mut paint,
    );
    let texts: Vec<String> = paint
        .draw_list()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    let tab_index = texts
        .iter()
        .position(|text| text == "主页")
        .expect("标签条");
    let item_index = texts
        .iter()
        .position(|text| text.contains("导入 PNG"))
        .expect("文件菜单项");
    assert!(
        tab_index < item_index,
        "标签条要先画，菜单浮层才能盖住它：tabs #{tab_index} / item #{item_index}"
    );
}

fn temp_workspace(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "image_editor_ws_{}_{n}_{tag}.igui",
        std::process::id()
    ))
}

#[test]
fn save_as_uses_the_dialog_and_clears_the_dirty_flag() {
    let path = temp_workspace("saveas");
    let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::saving(path.clone())));
    let index = logic.workspaces.active_index();
    assert!(logic.save_workspace_at(index, true), "另存为应写盘");
    assert!(path.exists());
    let workspace = logic.workspaces.get(index).unwrap();
    assert!(!workspace.dirty(), "保存后不脏");
    assert_eq!(workspace.path.as_deref(), Some(path.as_path()));
    let _ = std::fs::remove_file(&path);
}

/// 开了「记录过程」之后，保存工作区会顺手写下同名 `.igtl`；
/// 没开记录就不会多出文件（这个口子默认对用户透明）。
#[test]
fn saving_also_writes_the_timeline_when_recording_is_on() {
    use igui_core::{InputEvent, PointerButton, Size, Vec2, ViewportSize};

    let path = temp_workspace("timeline");
    let viewport = ViewportSize::new(Size::new(1280.0, 800.0));

    /// 在工作区里真实画一笔（走事件 -> 画笔 -> 历史）。
    fn stroke(workspace: &mut Workspace, viewport: ViewportSize) {
        let editor = &mut workspace.editor;
        editor.layout(viewport);
        let point = editor
            .canvas_camera()
            .document_to_screen(Vec2::new(8.5, 8.5));
        for event in [
            InputEvent::PointerDown {
                position: point,
                button: PointerButton::Left,
            },
            InputEvent::PointerMove {
                position: point + Vec2::new(6.0, 0.0),
            },
            InputEvent::PointerUp {
                position: point + Vec2::new(6.0, 0.0),
                button: PointerButton::Left,
            },
        ] {
            editor.event(&event);
        }
        editor.update();
    }

    // 关着：保存只写工作区。
    let mut off = logic_with_dialog(Rc::new(FakeFileDialog::saving(path.clone())));
    {
        let index = off.workspaces.active_index();
        let workspace = off.workspaces.get_mut(index).expect("工作区");
        workspace.editor.set_record_timeline(false);
        stroke(workspace, viewport);
        assert!(off.save_workspace_at(index, true));
    }
    assert!(
        !crate::io::timeline_path_for(&path).exists(),
        "没开记录不该写时间线"
    );
    let _ = std::fs::remove_file(&path);

    // 开着：保存顺手把当前记录写到 `x.igtl`。
    let mut on = logic_with_dialog(Rc::new(FakeFileDialog::saving(path.clone())));
    {
        let index = on.workspaces.active_index();
        on.preferences.record_timeline = true;
        let workspace = on.workspaces.get_mut(index).expect("工作区");
        workspace.editor.set_record_timeline(true);
        stroke(workspace, viewport);
        assert_eq!(workspace.editor.timeline_len(), Some(1), "一笔一步");
        assert!(on.save_workspace_at(index, true));
    }
    let timeline = crate::io::timeline_path_for(&path);
    let steps = crate::io::read_timeline(&timeline).expect("时间线应能读回");
    assert_eq!(steps.len(), 1, "一步笔触应在时间线里");
    assert_eq!(steps[0].label, "画笔");
    assert!(steps[0].patch.is_some(), "笔触应有像素补丁");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&timeline);
}

#[test]
fn cancelling_save_as_leaves_the_workspace_unsaved() {
    let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
    let index = logic.workspaces.active_index();
    assert!(!logic.save_workspace_at(index, true), "取消应返回 false");
    assert!(
        logic.workspaces.get(index).unwrap().path.is_none(),
        "取消后仍没有路径"
    );
}

#[test]
fn save_without_a_path_falls_back_to_save_as() {
    let path = temp_workspace("save");
    let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::saving(path.clone())));
    let index = logic.workspaces.active_index();
    assert!(
        logic.save_workspace_at(index, false),
        "没路径时保存应回退到另存为"
    );
    assert!(path.exists());
    let _ = std::fs::remove_file(&path);
}

/// 确认框的结果：没点按钮前保持打开，关窗口算取消。
#[test]
fn resolve_confirm_keeps_waiting_until_the_user_chooses() {
    assert_eq!(resolve_confirm(None, false), None, "还没选：保持打开");
    assert_eq!(
        resolve_confirm(None, true),
        Some(ConfirmResult::Cancel),
        "关窗口 = 取消"
    );
    assert_eq!(
        resolve_confirm(Some(ConfirmResult::Discard), false),
        Some(ConfirmResult::Discard)
    );
}

/// 把第 0 个工作区弄脏（保存 revision 与当前对不上）。
fn make_dirty(logic: &mut ImageEditorLogic) {
    let workspace = logic.workspaces.get_mut(0).unwrap();
    workspace.saved_revision = workspace.saved_revision.wrapping_add(1);
}

#[test]
fn closing_a_dirty_workspace_opens_a_confirmation_and_waits() {
    let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
    make_dirty(&mut logic);
    logic.request_close(0);
    assert!(logic.confirm.is_some(), "脏工作区关闭应先弹确认");
    logic.finish_confirm();
    assert!(logic.confirm.is_some(), "没点按钮时确认框不该消失");
    assert_eq!(logic.workspaces.tabs().len(), 1, "还没关");
}

#[test]
fn discarding_closes_without_saving() {
    let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
    make_dirty(&mut logic);
    logic.request_close(0);
    logic
        .confirm
        .as_ref()
        .unwrap()
        .choose(ConfirmResult::Discard);
    logic.finish_confirm();
    assert!(logic.confirm.is_none(), "确认框应关闭");
    assert!(logic.workspaces.is_empty(), "应直接丢弃并关闭");
}

#[test]
fn cancelling_keeps_the_workspace() {
    let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
    make_dirty(&mut logic);
    logic.request_close(0);
    logic
        .confirm
        .as_ref()
        .unwrap()
        .choose(ConfirmResult::Cancel);
    logic.finish_confirm();
    assert!(logic.confirm.is_none());
    assert_eq!(logic.workspaces.tabs().len(), 1, "取消后工作区还在");
}

#[test]
fn a_clean_workspace_closes_without_a_confirmation() {
    let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
    logic.request_close(0);
    assert!(logic.confirm.is_none(), "干净工作区不用确认");
    assert!(logic.workspaces.is_empty(), "直接关");
}

#[test]
fn file_shortcuts_map_to_intents() {
    let s = InputEvent::KeyDown {
        key: Key::Character('s'),
    };
    assert_eq!(file_shortcut(true, false, &s), Some(FileRequest::Save));
    assert_eq!(file_shortcut(true, true, &s), Some(FileRequest::SaveAs));
    assert_eq!(
        file_shortcut(false, false, &s),
        None,
        "没有 Ctrl/Cmd 不算快捷键"
    );
    let o = InputEvent::KeyDown {
        key: Key::Character('o'),
    };
    assert_eq!(file_shortcut(true, false, &o), Some(FileRequest::Open));
}

/// 不用随包字体时应直接用字体原生 `ascent`（cap 公式会把系统字体往下压）。
#[test]
fn a_non_bundled_measurer_keeps_the_native_ascent() {
    use igui_font::{FontConfig, FontMetrics, FontServer};

    let metrics = FontMetrics::new(std::rc::Rc::new(FontServer::load_with(
        FontConfig::default(),
    )));
    let native = metrics.ascent(13.0);
    assert_eq!(
        EditorTextMeasurer::new(metrics, false).ascent(13.0),
        native,
        "系统字体应直接用原生 ascent"
    );
}

#[test]
fn clipboard_shortcuts_cut_copy_and_paste() {
    let mut editor = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    let key = |ch: char| InputEvent::KeyDown {
        key: Key::Character(ch),
    };
    // 没有选区 / 剪贴板时仍是 handled（别把字符漏给输入框），但不改像素。
    assert!(
        !clipboard_shortcut(&mut editor, false, false, &key('c')),
        "没 Ctrl/Cmd 不算"
    );
    assert!(clipboard_shortcut(&mut editor, true, false, &key('c')));
    assert!(!editor.has_clipboard());

    // 选区 + 复制 -> 剪贴板有内容，粘贴后能贴回去。
    editor.select_all();
    assert!(clipboard_shortcut(&mut editor, true, false, &key('c')));
    assert!(editor.has_clipboard());
    assert!(
        clipboard_shortcut(&mut editor, true, false, &key('x')),
        "剪切"
    );
    assert!(
        clipboard_shortcut(&mut editor, true, false, &key('v')),
        "粘贴"
    );
    assert!(
        clipboard_shortcut(&mut editor, true, true, &key('v')),
        "Shift+粘贴 = 粘贴为新图层"
    );
    assert_eq!(editor.layer_count(), 2, "粘贴为新图层多一层");
}

#[test]
fn selection_shortcuts_select_all_and_invert() {
    let mut editor = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    let a = InputEvent::KeyDown {
        key: Key::Character('a'),
    };
    assert!(
        !selection_shortcut(&mut editor, false, false, &a),
        "没有 Ctrl/Cmd 时 A 不是全选"
    );
    assert!(selection_shortcut(&mut editor, true, false, &a));
    assert_eq!(
        editor.selection().map(|selection| selection.area()),
        Some(128 * 128)
    );

    let i = InputEvent::KeyDown {
        key: Key::Character('i'),
    };
    assert!(
        !selection_shortcut(&mut editor, true, false, &i),
        "反选要 Shift"
    );
    assert!(selection_shortcut(&mut editor, true, true, &i));
    assert!(editor.selection().is_none(), "全选反选 = 取消选择");
}

#[test]
fn history_shortcut_requires_a_command_modifier() {
    let mut editor = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    let z = InputEvent::KeyDown {
        key: Key::Character('z'),
    };
    assert!(
        !history_shortcut(&mut editor, false, false, &z),
        "没有 Ctrl/Cmd 时不该当快捷键"
    );
    assert!(
        history_shortcut(&mut editor, true, false, &z),
        "Ctrl/Cmd+Z 应命中撤销"
    );
}

/// 打开中的工作区是画廊的第一条，且带一张排好队的缩略图（纹理 id 按条目下标）。
#[test]
fn an_open_workspace_entry_carries_a_queued_thumbnail() {
    let logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
    let mut uploads = Vec::new();
    let entries = home_entries(&logic.workspaces, &mut uploads);
    let thumbnail = entries
        .first()
        .and_then(|entry| entry.thumbnail)
        .expect("打开中的工作区应有缩略图");
    assert_eq!(thumbnail.texture, crate::ui::thumbnail_texture(0));
    assert!(
        thumbnail.width <= 96 && thumbnail.height <= 96,
        "缩略图最长边不超过 96：{thumbnail:?}"
    );
    assert!(
        uploads
            .iter()
            .any(|(texture, pixels)| *texture == thumbnail.texture
                && (pixels.width, pixels.height) == (thumbnail.width, thumbnail.height)),
        "缩略图像素要排进上传队列"
    );
}

/// 文档 → 缩略图：合成（背景层 + 图层）后等比缩小，颜色不被缩放弄丢。
#[test]
fn a_document_thumbnail_is_composited_and_scaled() {
    let document = Document::with_background("测试", 200, 100, crate::document::Color::RED);
    let pixels = home::thumbnail_of(&document);
    assert_eq!(
        (pixels.width, pixels.height),
        (96, 48),
        "最长边 96、保持比例"
    );
    assert_eq!(pixels.get_pixel(48, 24), crate::document::Color::RED);
}

/// 清掉每个工作区开屏时的首次纹理上传，之后的变化才是本次操作引起的。
fn drain_texture_uploads(logic: &mut ImageEditorLogic) {
    for index in 0..logic.workspaces.tabs().len() {
        if let Some(workspace) = logic.workspaces.get_mut(index) {
            workspace.editor.take_texture_upload();
        }
    }
}

/// 后端只有一张 DOCUMENT_TEXTURE：关掉激活的工作区后，接手的那个必须重新合成，
/// 否则画布还停在已关闭工作区的图像上（关标签看起来「没更新」）。
#[test]
fn closing_the_active_workspace_recomposites_the_remaining_document() {
    let theme = crate::theme::editor_theme(false);
    let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
    logic
        .workspaces
        .push(Workspace::new(EditorView::new(theme, AppState::default())));
    drain_texture_uploads(&mut logic);
    assert_eq!(logic.workspaces.active_index(), 1);

    logic.close_workspace(1);
    assert_eq!(logic.workspaces.active_index(), 0);
    assert!(
        logic
            .workspaces
            .active_mut()
            .expect("还剩一个工作区")
            .editor
            .take_texture_upload()
            .is_some(),
        "关闭激活的工作区后要重合成剩下的文档"
    );
}

/// 关掉后台标签不动当前文档：没必要为 4K 文档重做一次整图合成。
#[test]
fn closing_a_background_workspace_keeps_the_active_texture() {
    let theme = crate::theme::editor_theme(false);
    let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
    logic
        .workspaces
        .push(Workspace::new(EditorView::new(theme, AppState::default())));
    drain_texture_uploads(&mut logic);

    logic.close_workspace(0);
    assert_eq!(logic.workspaces.active_index(), 0, "激活的仍是原来的第二个");
    assert!(
        logic
            .workspaces
            .active_mut()
            .expect("还剩一个工作区")
            .editor
            .take_texture_upload()
            .is_none(),
        "关后台标签不该触发重合成"
    );
}

/// 系统菜单栏点下来的动作由宿主转给当前工作区执行（那条路没有 `Intent` 队列）。
#[test]
fn a_native_menu_action_runs_in_the_active_workspace() {
    use crate::ui::menu::MenuAction;

    let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
    logic.screen = Screen::Editor;
    assert!(
        logic
            .workspaces
            .active()
            .expect("一个工作区")
            .editor
            .menu_state()
            .checkerboard,
        "棋盘格默认是开的"
    );
    logic.apply_menu_action(MenuAction::ToggleCheckerboard);
    assert!(
        !logic
            .workspaces
            .active()
            .expect("一个工作区")
            .editor
            .menu_state()
            .checkerboard,
        "宿主应把菜单动作交给当前工作区"
    );
    assert!(logic.title_dirty, "动作之后标题要在下一帧写回窗口");
}

/// 没有工作区（主页）时菜单是灰的：动作丢掉，不碰任何编辑器，也不重写窗口标题。
#[test]
fn a_native_menu_action_without_a_workspace_is_dropped() {
    use crate::ui::menu::MenuAction;

    let theme = crate::theme::editor_theme(false);
    let mut logic = ImageEditorLogic::new(theme, Rc::new(FakeFileDialog::default()));
    logic.screen = Screen::Home(crate::ui::HomeView::new(theme, Vec::new()));
    logic.apply_menu_action(MenuAction::Undo);
    assert!(logic.workspaces.active().is_none());
    assert!(!logic.title_dirty, "没执行的动作不该改标题");
}

/// 模态窗口开着时系统菜单栏整张置灰（不然就绕过了模态直接改下面的文档）。
#[test]
fn the_native_menu_is_greyed_out_while_a_modal_is_open() {
    let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
    logic.screen = Screen::Editor;
    assert!(logic.native_menu_state().has_document);
    logic.open_new_document_modal();
    assert!(
        !logic.native_menu_state().has_document,
        "模态开着时菜单不该可点"
    );
}

/// 模拟窗口（属性 / 关于 / 快捷键）开着时系统菜单栏也整张置灰。
#[test]
fn the_native_menu_is_greyed_out_while_a_dialog_is_open() {
    let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
    logic.screen = Screen::Editor;
    assert!(logic.native_menu_state().has_document);
    logic
        .workspaces
        .active()
        .expect("一个工作区")
        .editor
        .intents()
        .push(crate::ui::Intent::Dialog(crate::ui::DialogKind::Settings));
    logic.open_popup_if_requested();
    assert!(logic.popup.is_some());
    assert!(
        !logic.native_menu_state().has_document,
        "窗口开着时菜单不该可点"
    );
}

/// 选项栏的「全部属性…」→ 宿主建出属性窗口；Esc 关掉它，并且窗口开着时
/// 编辑器收不到输入（工具不被改掉）。
#[test]
fn the_properties_window_opens_from_the_options_bar_and_swallows_input() {
    use igui_core::{InputEvent, Key};

    let mut logic = logic_with_dialog(Rc::new(FakeFileDialog::default()));
    logic.screen = Screen::Editor;
    let viewport = igui_core::ViewportSize::new(igui_core::Size::new(1280.0, 800.0));
    logic
        .workspaces
        .active_mut()
        .expect("一个工作区")
        .editor
        .layout(viewport);
    let center = logic
        .workspaces
        .active()
        .expect("一个工作区")
        .editor
        .settings_button_center()
        .expect("全部属性…");
    let workspace = logic.workspaces.active_mut().expect("一个工作区");
    for event in [
        InputEvent::PointerDown {
            position: center,
            button: igui_core::PointerButton::Left,
        },
        InputEvent::PointerUp {
            position: center,
            button: igui_core::PointerButton::Left,
        },
    ] {
        assert!(!logic.tabs.event(&event).is_handled());
        workspace.editor.event(&event);
    }
    // 真实流程是 `update` 末尾取走意图；这里直接跑那一步。
    logic.open_popup_if_requested();
    assert!(logic.popup.is_some(), "「全部属性…」应打开属性窗口");

    // 窗口开着时按 G 不该把工具切到油漆桶。
    logic.route_event(&InputEvent::KeyDown {
        key: Key::Character('g'),
    });
    assert_eq!(
        logic
            .workspaces
            .active()
            .expect("一个工作区")
            .editor
            .active_tool(),
        crate::app::state::ActiveTool::Brush,
        "窗口开着时输入应被它吞掉"
    );

    // Esc 关闭。
    logic.route_event(&InputEvent::KeyDown { key: Key::Escape });
    assert!(logic.popup.is_none(), "Esc 应关掉窗口");
}
