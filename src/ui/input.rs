//! 输入分发：快捷键 / 画布缩放平移先由视图处理，其余交给 `igui_ui`。

use igui_core::{EventResult, InputEvent, Key, Vec2};

use super::*;

impl EditorView {
    /// 路由一个后端无关的输入事件。
    ///
    /// 工具快捷键、画布缩放 / 平移由视图先处理；其余（点击、指针）交给
    /// `igui_ui::handle_input`，由它去找带回调的控件。
    pub fn event(&mut self, event: &InputEvent) -> EventResult {
        // 覆盖层优先：菜单打开时，Esc / 点击外部由它先处理。
        // 例外：点的是菜单标题时，先关掉当前菜单、把点击留给主树 —— 这样点
        // 另一个标题一次就能切换，点同一个标题则是关闭。
        if let InputEvent::PointerDown { position, .. } = event {
            if self.menu_open() {
                if let Some(index) = self.menu_index_at(*position) {
                    let same = self.open_menu.get() == Some(index);
                    self.overlays.close_all();
                    self.open_menu.set(None);
                    if same {
                        return EventResult::Handled;
                    }
                } else if self.overlays.handle_input(event).is_handled() {
                    return EventResult::Handled;
                }
            } else if self.overlays.handle_input(event).is_handled() {
                return EventResult::Handled;
            }
        } else if self.overlays.handle_input(event).is_handled() {
            return EventResult::Handled;
        }
        // 重命名浮层里的输入框会自己吞掉编辑键；单行的 `TextInput` 不处理 Enter，
        // 所以这里补一刀提交。
        if self.rename.is_some() && matches!(event, InputEvent::KeyDown { key: Key::Enter }) {
            self.finish_rename(true);
            return EventResult::Handled;
        }
        match event {
            InputEvent::KeyDown { key } => {
                if let Some(tool) = shortcut_tool(*key) {
                    self.state.borrow_mut().active_tool = tool;
                    tracing::info!(target: "image_editor", tool = tool.label(), "tool_changed");
                    return EventResult::Handled;
                }
                if let Some(action) = canvas_shortcut(*key) {
                    self.apply_canvas_action(action);
                    return EventResult::Handled;
                }
                if let Some(delta) = brush_size_shortcut(*key) {
                    self.adjust_brush_size(delta);
                    return EventResult::Handled;
                }
                if let Some(delta) = brush_opacity_shortcut(*key) {
                    self.adjust_brush_opacity(delta);
                    return EventResult::Handled;
                }
                if matches!(key, Key::Escape) && self.clear_selection() {
                    return EventResult::Handled;
                }
            }
            InputEvent::Wheel { position, delta } => {
                if self.canvas_area_contains(*position) {
                    let factor = if delta.y < 0.0 {
                        ZOOM_STEP
                    } else {
                        1.0 / ZOOM_STEP
                    };
                    self.state.borrow_mut().canvas.zoom_at(*position, factor);
                    return EventResult::Handled;
                }
            }
            InputEvent::PointerDown {
                position,
                button: PointerButton::Left,
            } => {
                if self.canvas_area_contains(*position) {
                    self.begin_tool(*position);
                    return EventResult::Handled;
                }
            }
            InputEvent::PointerDown {
                position,
                button: PointerButton::Middle,
            } => {
                if self.canvas_area_contains(*position) {
                    self.pan_last = Some(*position);
                    return EventResult::Handled;
                }
            }
            InputEvent::PointerMove { position } => {
                self.pointer = self.canvas_area_contains(*position).then_some(*position);
                if let Some(last) = self.pan_last {
                    let delta = *position - last;
                    self.pan_last = Some(*position);
                    self.state.borrow_mut().canvas.pan_by(delta);
                    return EventResult::Handled;
                }
                if self.is_dragging() {
                    self.continue_tool(*position);
                    return EventResult::Handled;
                }
            }
            InputEvent::PointerLeave => {
                self.pointer = None;
            }
            InputEvent::PointerUp {
                position,
                button: PointerButton::Left,
            } => {
                if self.is_dragging() {
                    self.end_tool(*position);
                    return EventResult::Handled;
                }
            }
            InputEvent::PointerUp {
                button: PointerButton::Middle,
                ..
            } if self.pan_last.take().is_some() => {
                return EventResult::Handled;
            }
            _ => {}
        }
        igui_ui::handle_input(&mut self.tree, event)
    }

    // -- 画布 ------------------------------------------------------------

    pub(super) fn apply_canvas_action(&mut self, action: CanvasAction) {
        match action {
            CanvasAction::ZoomIn => self.zoom_by(ZOOM_STEP),
            CanvasAction::ZoomOut => self.zoom_by(1.0 / ZOOM_STEP),
            CanvasAction::Reset => self.reset_view(),
            CanvasAction::Fit => self.fit_to_canvas(),
        }
    }

    pub(super) fn zoom_by(&mut self, factor: f32) {
        let anchor = self
            .canvas_area_rect()
            .map(|rect| rect.center())
            .unwrap_or_else(|| {
                Rect::from_min_size(Vec2::ZERO, self.viewport.logical_size()).center()
            });
        self.state.borrow_mut().canvas.zoom_at(anchor, factor);
    }

    /// 让文档适配画布区域并居中。
    pub fn fit_to_canvas(&mut self) {
        let Some(area) = self.canvas_area_rect() else {
            return;
        };
        let document = self.document_size();
        let camera = CanvasCamera::fit(
            area,
            Size::new(document.0 as f32, document.1 as f32),
            FIT_PADDING,
        );
        self.state.borrow_mut().canvas = camera;
    }

    /// 100% 且居中。
    pub(super) fn reset_view(&mut self) {
        let Some(area) = self.canvas_area_rect() else {
            return;
        };
        let document = self.document_size();
        self.state.borrow_mut().canvas =
            CanvasCamera::centered(area, Size::new(document.0 as f32, document.1 as f32), 1.0);
    }
}
