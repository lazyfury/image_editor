//! 输入分发：快捷键 / 画布缩放平移先由视图处理，其余交给 `igui_ui`。

use igui_core::{EventResult, InputEvent, Key, ScrollDevice, Vec2};

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
                // 空格 = 临时抓手：只在没有 Cmd/Ctrl、也没有在重命名时接管。
                if matches!(key, Key::Space)
                    && !self.modifiers.ctrl
                    && !self.modifiers.meta
                    && self.rename.is_none()
                {
                    self.hold_space();
                    return EventResult::Handled;
                }
                // 工具键不带修饰键：`Cmd+Z` 撤销、`Cmd+H` 隐藏窗口这些不能被
                // `Z` / `H` 工具键抢走（宿主已经先处理了一部分，这里再兜一层）。
                if !self.modifiers.ctrl && !self.modifiers.meta {
                    if let Some(tool) = shortcut_tool(*key) {
                        self.state.borrow_mut().active_tool = tool;
                        tracing::info!(target: "image_editor", tool = tool.label(), "tool_changed");
                        return EventResult::Handled;
                    }
                }
                if let Some(action) = canvas_shortcut(*key) {
                    self.apply_canvas_action(action);
                    return EventResult::Handled;
                }
                if let Some(action) = rotation_shortcut(*key) {
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
                // 钢笔优先：Esc 取消未完成的路径、Enter 收尾、Backspace 删锚点，
                // 之后再轮到「Esc 清空选区」这些全局快捷键。
                if self.handle_pen_key(*key) {
                    return EventResult::Handled;
                }
                // 删选区内容：macOS 的 Delete 键报成 `Backspace`（钢笔已经先截走
                // 它自己的「删锚点」）。
                if matches!(key, Key::Delete | Key::Backspace) && self.delete_selection() {
                    return EventResult::Handled;
                }
                if matches!(key, Key::Escape) {
                    self.layer_drag_cancel();
                    if self.clear_selection() {
                        return EventResult::Handled;
                    }
                }
            }
            // 松开空格 = 退出临时抓手（只在我们接管过的时候消费）。
            InputEvent::KeyUp { key } if matches!(key, Key::Space) && self.space_held() => {
                self.release_space();
                return EventResult::Handled;
            }
            // 按住空格时系统还会补一个 `TextInput(" ")`，别让它落进输入框 /
            // 触发按钮；空格接管期间一律吞掉。
            InputEvent::TextInput { text } if self.space_held() && text == " " => {
                return EventResult::Handled;
            }
            // 滚轮 / 触控板滚动：设备不同、语义不同（见 `docs/input-gestures.md`）。
            // 指针不在画布上就交回 `igui_ui`，面板里的列表照常滚。
            InputEvent::Wheel {
                position,
                delta,
                device,
                ..
            } => {
                if self.canvas_area_contains(*position) {
                    match device {
                        // 带刻度的滚轮：保持原来的「滚轮缩放」。`Shift` 换轴成水平平移。
                        ScrollDevice::Wheel if self.modifiers.shift => {
                            self.pan_canvas(Vec2::new(-delta.y, 0.0));
                        }
                        ScrollDevice::Wheel => {
                            let factor = if delta.y < 0.0 {
                                ZOOM_STEP
                            } else {
                                1.0 / ZOOM_STEP
                            };
                            self.state.borrow_mut().canvas.zoom_at(*position, factor);
                        }
                        // 触控板：内容跟手平移（含惯性），`Shift` 只取主方向换轴。
                        ScrollDevice::Trackpad => {
                            let content = if self.modifiers.shift {
                                Vec2::new(-delta.y, 0.0)
                            } else {
                                -*delta
                            };
                            self.pan_canvas(content);
                        }
                    }
                    return EventResult::Handled;
                }
            }
            // 两指捏合：以指针为锚缩放；指针不在画布上时锚画布中心。
            InputEvent::Pinch {
                position, factor, ..
            } => {
                if factor.is_finite() && *factor > 0.0 {
                    let anchor = if self.canvas_area_contains(*position) {
                        *position
                    } else {
                        self.rotation_anchor()
                    };
                    // 单个事件夹一下，防极端 delta 一步把缩放顶到上下限。
                    let factor = factor.clamp(PINCH_FACTOR_MIN, PINCH_FACTOR_MAX);
                    self.state.borrow_mut().canvas.zoom_at(anchor, factor);
                    return EventResult::Handled;
                }
            }
            // 两指旋转：绕画布中心转，接近 15° 倍数时吸附（像素画要能停在直角）。
            InputEvent::Rotate { delta, .. } => {
                if delta.is_finite() && delta.abs() > ROTATE_DEAD_ZONE_DEGREES {
                    self.gesture_rotate(*delta);
                    return EventResult::Handled;
                }
            }
            // 图层列表的拖动：先记起点（不消费），真拖起来之后由 move / up 消费。
            InputEvent::PointerDown {
                position,
                button: PointerButton::Left,
            } => {
                if self.canvas_area_contains(*position) {
                    let tool = self.state.borrow().active_tool;
                    if self.press_navigation(tool, *position) {
                        return EventResult::Handled;
                    }
                }
                self.layer_drag_press(*position);
                if self.canvas_area_contains(*position) {
                    self.begin_tool(*position);
                    return EventResult::Handled;
                }
            }
            // 双击组行 = 展开 / 折叠（`List` 只有整行点击，逐格点击要上游改）。
            InputEvent::DoubleClick { position } => {
                if self.toggle_layer_group_at(*position) {
                    return EventResult::Handled;
                }
            }
            InputEvent::PointerDown {
                position,
                button: PointerButton::Middle,
            } => {
                if self.canvas_area_contains(*position) {
                    self.press_middle_pan(*position);
                    return EventResult::Handled;
                }
            }
            InputEvent::PointerMove { position } => {
                self.pointer = self.canvas_area_contains(*position).then_some(*position);
                if self.drag_navigation(*position) {
                    return EventResult::Handled;
                }
                if self.layer_drag_move(*position) {
                    return EventResult::Handled;
                }
                if self.is_dragging() {
                    self.continue_tool(*position);
                    return EventResult::Handled;
                }
                // 钢笔的橡皮筋要跟手：不按键的移动也要转发（这里不消费事件，
                // 免得抢掉 UI 的 hover）。
                if self.state.borrow().active_tool == crate::app::state::ActiveTool::Pen {
                    self.hover_pen(*position);
                }
            }
            InputEvent::PointerLeave => {
                self.pointer = None;
                self.pen.set_hover(None);
            }
            InputEvent::PointerUp {
                position,
                button: PointerButton::Left,
            } => {
                if self.release_navigation() {
                    return EventResult::Handled;
                }
                if self.layer_drag_release(*position) {
                    return EventResult::Handled;
                }
                if self.is_dragging() {
                    self.end_tool(*position);
                    return EventResult::Handled;
                }
            }
            InputEvent::PointerUp {
                button: PointerButton::Middle,
                ..
            } if self.release_middle_pan() => {
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
            CanvasAction::RotateCw => self.rotate_view(ROTATE_STEP_DEGREES),
            CanvasAction::RotateCcw => self.rotate_view(-ROTATE_STEP_DEGREES),
            CanvasAction::RotateReset => self.rotate_view_to(0.0),
        }
    }

    /// 视图旋转的锚点：画布区域中心。
    ///
    /// 不用指针：转角手势报不了「两指中心」，锚在指针上会让画布往外漂；
    /// 锚在区域中心时连续转也不会跑出视野。
    fn rotation_anchor(&self) -> Vec2 {
        self.canvas_area_rect()
            .map(|rect| rect.center())
            .unwrap_or_else(|| {
                Rect::from_min_size(Vec2::ZERO, self.viewport.logical_size()).center()
            })
    }

    /// 屏幕空间的「内容位移」平移画布（正数 = 内容向右 / 向下移动）。
    pub(super) fn pan_canvas(&mut self, content: Vec2) {
        self.state.borrow_mut().canvas.pan_by(content);
    }

    /// 把视图转 `degrees` 度（正数 = 顺时针），绕画布中心。键盘 / 菜单用：
    /// 步进是固定的，不做吸附。
    fn rotate_view(&mut self, degrees: f32) {
        let anchor = self.rotation_anchor();
        self.state
            .borrow_mut()
            .canvas
            .rotate_by(degrees.to_radians(), anchor);
    }

    /// 触控板旋转手势：高频率小步，转完做一次 15° 吸附。离直角 / 15° 倍数
    /// 不到 [`ROTATE_SNAP_DEGREES`] 就贴上去，像素画能稳稳停在 90°，也顺手
    /// 吸掉「两指滚动时带出来的一点点旋转」。
    fn gesture_rotate(&mut self, degrees: f32) {
        let anchor = self.rotation_anchor();
        let mut state = self.state.borrow_mut();
        state.canvas.rotate_by(degrees.to_radians(), anchor);
        let angle = state.canvas.angle.to_degrees();
        let snapped = (angle / ROTATE_STEP_DEGREES).round() * ROTATE_STEP_DEGREES;
        if (angle - snapped).abs() <= ROTATE_SNAP_DEGREES {
            state.canvas.rotate_to(snapped.to_radians(), anchor);
        }
    }

    /// 转到指定角度（度），绕画布中心。
    fn rotate_view_to(&mut self, degrees: f32) {
        let anchor = self.rotation_anchor();
        self.state
            .borrow_mut()
            .canvas
            .rotate_to(degrees.to_radians(), anchor);
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

    /// 让文档适配画布区域并居中（**保留当前视图角度**）。
    pub fn fit_to_canvas(&mut self) {
        let Some(area) = self.canvas_area_rect() else {
            return;
        };
        let document = self.document_size();
        let angle = self.state.borrow().canvas.angle;
        let camera = CanvasCamera::fit_rotated(
            area,
            Size::new(document.0 as f32, document.1 as f32),
            FIT_PADDING,
            angle,
        );
        self.state.borrow_mut().canvas = camera;
    }

    /// 100% 且居中，**角度也归零**（「重置视图」）。
    pub(super) fn reset_view(&mut self) {
        let Some(area) = self.canvas_area_rect() else {
            return;
        };
        let document = self.document_size();
        self.state.borrow_mut().canvas =
            CanvasCamera::centered(area, Size::new(document.0 as f32, document.1 as f32), 1.0);
    }
}
