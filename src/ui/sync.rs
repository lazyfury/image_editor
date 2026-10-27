//! 把 [`AppState`] / 工具状态同步进控件树（标签 / 列表 / 属性 / 选项栏）。
//!
//! 全是「读状态 -> `set_text` / `update_control` / 可见性」的幂等同步；`update`
//! 每帧按需调用。

use igui_components::Placement;

use crate::app::state::ActiveTool;

use super::*;

impl EditorView {
    /// 标签页切换：只让当前内容参与布局与绘制（隐藏内容不可见，也不命中）。
    /// 激活页没变时是空操作，避免每帧重排。
    pub(super) fn sync_tabs(&mut self) {
        let active = self.layout.active_tab.get();
        if self.layout.shown_tab == Some(active) {
            return;
        }
        self.layout.shown_tab = Some(active);
        tabs::show_active(
            &mut self.tree,
            self.layout.tabs_panel,
            active,
            &self.layout.tab_contents,
        );
    }

    /// 工具 / 缩放 / 视角 / 指针像素 -> 状态栏。
    pub(super) fn sync_status(&mut self) {
        let (tool, zoom, angle, width, height, camera) = {
            let state = self.state.borrow();
            (
                state.active_tool,
                state.canvas.zoom,
                state.canvas.angle,
                state.document.width,
                state.document.height,
                state.canvas,
            )
        };
        // 指针在画布区域里时，状态栏顺便报一下文档像素坐标（§12 的坐标转换）。
        let pixel = self
            .pointer
            .and_then(|point| document_to_pixel(screen_to_document(camera, point), width, height));
        let shown = (tool, zoom, width, height, pixel, angle);
        if self.shown != Some(shown) {
            set_text(&mut self.tree, self.tool_label, tool.label());
            let coords = match pixel {
                Some((x, y)) => format!("  ·  ({x}, {y})"),
                None => String::new(),
            };
            // 视图转了角度才显示：没转的时候状态栏不多一个「0°」。
            let rotation = if angle.abs() > 1e-4 {
                format!("  ·  ↻ {:.0}°", angle.to_degrees())
            } else {
                String::new()
            };
            set_text(
                &mut self.tree,
                self.zoom_label,
                format!(
                    "{width} × {height}  ·  {:.0}%{rotation}{coords}",
                    zoom * 100.0
                ),
            );
            self.shown = Some(shown);
        }
    }

    /// 文档改动后：更新列表行数与选中行，并让已挂载的行重新读数据。
    pub(super) fn refresh_layers(&mut self) {
        let count = self.state.borrow().document.layers.len();
        self.layer_count.set(count);
        self.layer_selected.set(self.active_list_index());
        self.layer_state.invalidate();
    }

    /// 历史长度变化后刷新历史面板的行数。
    pub(super) fn refresh_history(&mut self) {
        let (undo, redo) = {
            let state = self.state.borrow();
            (state.history.undo_len(), state.history.redo_len())
        };
        self.history_count.set(undo + 1 + redo);
        self.history_state.invalidate();
    }

    /// 当前图层在“最上面为 0”的列表里的下标。
    pub(super) fn active_list_index(&self) -> Option<usize> {
        let state = self.state.borrow();
        let document = &state.document;
        document
            .active_layer
            .and_then(|id| document.layer_index(id))
            .map(|index| document.layers.len() - 1 - index)
    }

    /// 当前图层 -> 属性面板。
    pub(super) fn sync_properties(&mut self) {
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

    /// 打开重命名浮层，锚在「重命名」按钮上。
    ///
    /// 编辑 / 光标 / 选区 / 剪贴板 / IME 全交给浮层里的
    /// [`TextInput`](igui_components::TextInput)；这里只管提交。
    pub(super) fn open_rename(&mut self, anchor: NodeId) {
        let (id, name) = {
            let state = self.state.borrow();
            match state.document.active_layer() {
                Some(layer) => (layer.id, layer.name.clone()),
                None => return,
            }
        };
        let edit = Rc::new(RefCell::new(None));
        let confirm = Rc::new(Cell::new(false));
        let cancel = Rc::new(Cell::new(false));
        let theme = self.theme;
        let slot = edit.clone();
        let (ok, no) = (confirm.clone(), cancel.clone());
        let overlay = self
            .overlays
            .popover(anchor, Placement::Above, move |tree, root| {
                rename::content(tree, root, theme, &name, &slot, ok.clone(), no.clone());
            });
        self.overlays.title(overlay, "重命名图层");
        self.rename = Some(Rename {
            id,
            overlay,
            edit,
            confirm,
            cancel,
        });
    }

    /// 关掉重命名浮层；`commit` 为真时提交输入框里的文本。
    pub(super) fn finish_rename(&mut self, commit: bool) {
        let Some(rename) = self.rename.take() else {
            return;
        };
        self.overlays.close(rename.overlay);
        if commit {
            self.commit_rename(rename);
        }
    }

    /// 提交重命名：读输入框的文本，非空就改名（一条 undo）。
    fn commit_rename(&mut self, rename: Rename) {
        let Some(edit) = rename.edit.borrow().clone() else {
            return;
        };
        let name = edit.borrow().text().trim().to_string();
        if name.is_empty() {
            return;
        }
        let mut state = self.state.borrow_mut();
        let before = LayerMetaCommand::capture(&state.document);
        state.document.rename_layer(rename.id, name);
        let after = LayerMetaCommand::capture(&state.document);
        state.execute(Box::new(LayerMetaCommand::new(before, after, "重命名图层")));
    }

    // -- 文件 / 路径（Phase 7） ------------------------------------------

    /// 工作区文件路径每帧同步（只读展示）。
    pub(super) fn sync_path(&mut self) {
        let display = match &*self.workspace_path.borrow() {
            Some(path) => path.display().to_string(),
            None => "未保存（⌘S 保存）".to_string(),
        };
        if self.shown_path.as_deref() != Some(display.as_str()) {
            set_text(&mut self.tree, self.path_label, display.clone());
            self.shown_path = Some(display);
        }
    }

    /// 工具选项栏：工具名 / 提示 / 笔刷配置，按当前工具显示。
    pub(super) fn sync_options(&mut self) {
        let tool = self.state.borrow().active_tool;
        set_text(&mut self.tree, self.options.tool, tool.label());
        set_text(
            &mut self.tree,
            self.options.size,
            format!("{:.0}px", self.brush.size),
        );
        set_text(
            &mut self.tree,
            self.options.hardness,
            format!("{:.0}%", self.brush.hardness * 100.0),
        );
        set_text(
            &mut self.tree,
            self.options.smooth,
            format!("{:.0}%", self.brush.smooth * 100.0),
        );
        set_text(
            &mut self.tree,
            self.options.spacing,
            format!("{:.0}%", self.brush.spacing_ratio * 100.0),
        );
        // 参数不命中任何预设时显示「自定义」（见 `tools::brush::preset`）。
        set_text(
            &mut self.tree,
            self.options.preset_label,
            crate::tools::brush::preset::label(crate::tools::brush::preset::PRESETS, &self.brush),
        );
        set_text(
            &mut self.tree,
            self.options.opacity,
            format!("{:.0}%", self.brush.opacity * 100.0),
        );
        set_text(&mut self.tree, self.options.hint, options_hint(tool));
        set_text(
            &mut self.tree,
            self.options.tolerance,
            format!("{}", self.paint_bucket.tolerance),
        );
        set_text(
            &mut self.tree,
            self.gradient.kind_label,
            self.gradient.kind.label(),
        );

        set_text(
            &mut self.tree,
            self.options.symmetry,
            self.options.symmetry_mode.get().label(),
        );
        let show_brush = tool_has_brush(tool);
        if self.tree.is_visible(self.options.brush) != Some(show_brush) {
            self.tree.set_visible(self.options.brush, show_brush);
            igui_ui::mark_dirty(&mut self.tree, self.options.brush);
        }
        let pen_summary = self.pen_summary();
        set_text(&mut self.tree, self.options.pen_summary, pen_summary);
        let show_pen = tool == ActiveTool::Pen;
        if self.tree.is_visible(self.options.pen) != Some(show_pen) {
            self.tree.set_visible(self.options.pen, show_pen);
            igui_ui::mark_dirty(&mut self.tree, self.options.pen);
        }
        let show_bucket = matches!(tool, ActiveTool::PaintBucket | ActiveTool::MagicWand);
        if self.tree.is_visible(self.options.bucket) != Some(show_bucket) {
            self.tree.set_visible(self.options.bucket, show_bucket);
            igui_ui::mark_dirty(&mut self.tree, self.options.bucket);
        }
        let show_gradient = tool == ActiveTool::Gradient;
        if self.tree.is_visible(self.options.gradient) != Some(show_gradient) {
            self.tree.set_visible(self.options.gradient, show_gradient);
            igui_ui::mark_dirty(&mut self.tree, self.options.gradient);
        }
    }
}
