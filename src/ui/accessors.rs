//! [`EditorView`] 的访问器：宿主 / 测试 / `--selfcheck` 读状态、点控件用。
//!
//! 全是 `&self` / `&mut self` 的薄查询，不含业务逻辑；放在这里让 `mod.rs`
//! 只留构造 / 更新 / 事件分发。

use std::rc::Rc;

use igui_core::Vec2;

use crate::app::state::{ActiveTool, HistoryAction};
use crate::canvas::{CanvasCamera, Selection};
use crate::document::{Color, PixelBuffer, Point};
use crate::tools::{BrushShape, GradientKind};

use super::mask_view::{paint_mask_overlay, paint_mask_overlay_region};
use super::*;

impl EditorView {
    // -- 访问器（宿主 / 测试 / 自检） -----------------------------------

    /// 当前激活的工具。
    pub fn active_tool(&self) -> ActiveTool {
        self.state.borrow().active_tool
    }

    /// 当前文档名字（宿主用它更新窗口标题）。
    pub fn document_name(&self) -> String {
        self.state.borrow().document.name.clone()
    }

    /// 当前文档像素尺寸。
    pub fn document_size(&self) -> (u32, u32) {
        let state = self.state.borrow();
        (state.document.width, state.document.height)
    }

    /// 当前画布相机。
    pub fn canvas_camera(&self) -> CanvasCamera {
        self.state.borrow().canvas
    }

    /// 指针当前是否落在画布区域里（宿主选光标用）。
    pub fn pointer_over_canvas(&self) -> bool {
        self.pointer
            .is_some_and(|pointer| self.canvas_area_contains(pointer))
    }

    /// 指针下 igui 控件想要的光标（按钮 / 文本 / 拖拽条…）。
    pub fn hovered_cursor(&self) -> igui_core::Cursor {
        igui_ui::hovered_cursor(&self.tree)
    }

    /// 取走待上传的文档合成结果；宿主把它注册成纹理。没有变化时返回 `None`。
    pub fn take_texture_upload(&mut self) -> Option<PixelBuffer> {
        let full = std::mem::take(&mut self.texture_dirty);
        let region = self.dirty_region.take();
        if !full && region.is_none() {
            return None;
        }
        let state = self.state.borrow();
        let document = &state.document;
        match (full, region) {
            // 整图：导出 / 尺寸变化 / 图层结构变了。
            (true, _) | (false, None) => {
                self.renderer.render(document, &mut self.render_target);
                if self.show_checkerboard {
                    paint_backdrop(&mut self.render_target.pixels);
                }
                paint_mask_overlay(document, &mut self.render_target.pixels);
            }
            // 局部：画笔一笔只脏一小块，4K 文档不用每帧重算整张。
            (false, Some(region)) => {
                let region = region.clamped(document.width, document.height);
                if region.area() == 0 {
                    return None;
                }
                self.renderer
                    .render_region(document, &mut self.render_target, region);
                if self.show_checkerboard {
                    paint_backdrop_in(&mut self.render_target.pixels, region);
                }
                paint_mask_overlay_region(document, &mut self.render_target.pixels, region);
            }
        }
        // 预览面板：直接从刚合成好的显示结果缩，不再多合成一遍。
        // 整图重合成（结构改动 / 切工作区）立即刷新；画笔的局部更新走 200ms 节流。
        if full
            || self.preview_at.elapsed()
                >= std::time::Duration::from_millis(crate::ui::preview_panel::PREVIEW_INTERVAL_MS)
        {
            self.preview_at = std::time::Instant::now();
            self.preview_upload = Some(
                self.render_target
                    .pixels
                    .scaled_to_fit(crate::ui::preview_panel::PREVIEW_MAX_EDGE),
            );
        }
        Some(self.render_target.pixels.clone())
    }

    /// 取走待上传的预览图（宿主把它灌成 `PREVIEW_TEXTURE`）。
    pub fn take_preview_upload(&mut self) -> Option<crate::document::PixelBuffer> {
        self.preview_upload.take()
    }

    /// 预览面板当前的采样方式。
    pub fn preview_filter(&self) -> crate::canvas::PreviewFilter {
        self.preview_filter.get()
    }

    /// 设置预览采样方式（宿主启动时用偏好初始化）。
    pub fn set_preview_filter(&mut self, filter: crate::canvas::PreviewFilter) {
        self.preview_filter.set(filter);
    }

    /// 「线性 / 最近邻」按钮的中心（测试模拟点击用）。
    #[allow(dead_code)] // 只有测试点；自检不点这个按钮。
    ///
    /// 只找**预览面板里**的同名文字：渐变类型也有「线性」，不限定范围会点到别处。
    pub fn preview_filter_center(&self, filter: crate::canvas::PreviewFilter) -> Option<Vec2> {
        let label = filter.label();
        let area = self.preview_rect()?;
        let node = self.tree.iter().find(|&node| {
            igui_ui::content(&self.tree, node).and_then(|content| content.as_text()) == Some(label)
                && igui_ui::control(&self.tree, node)
                    .is_some_and(|control| area.contains(control.rect.center()))
        })?;
        igui_ui::control(&self.tree, node).map(|control| control.rect.center())
    }

    /// 预览面板的矩形（测试 / 自检用）。
    #[allow(dead_code)] // 只有测试读；自检看的是命令流。
    pub fn preview_rect(&self) -> Option<igui_core::Rect> {
        igui_ui::control(&self.tree, self.preview).map(|control| control.rect)
    }

    /// 当前文档的缩略图（最长边 `max_edge`）：主页画廊预览用。
    ///
    /// 只读：合成到一块临时目标上，不动 `render_target` / 脏标记，也不加
    /// 棋盘格（那是画布的显示背景，缩略图用工作区自己的图层内容）。
    pub fn thumbnail(&self, max_edge: u32) -> PixelBuffer {
        let state = self.state.borrow();
        let mut target =
            crate::renderer::RenderTarget::new(state.document.width, state.document.height);
        self.renderer.render(&state.document, &mut target);
        target.pixels.scaled_to_fit(max_edge)
    }

    /// 文档像素变了，需要**整图**重新合成 / 上传（图层增删 / 移动 / 撤销 / 导入…）。
    pub fn mark_texture_dirty(&mut self) {
        self.texture_dirty = true;
        self.dirty_region = None;
    }

    /// 只有 `region`（文档坐标）变了：局部重合成，其余像素沿用上一帧。
    pub fn mark_region_dirty(&mut self, region: crate::document::PixelRegion) {
        if self.texture_dirty {
            return; // 反正要整图，不用再攒脏区
        }
        self.dirty_region = Some(match self.dirty_region {
            Some(old) => old.union(region),
            None => region,
        });
    }

    // -- 撤销 / 重做（Phase 6） -------------------------------------------

    /// 撤销一步，返回是否真的撤销了。
    pub fn undo(&mut self) -> bool {
        self.run_history(HistoryAction::Undo)
    }

    /// 重做一步，返回是否真的重做了。
    pub fn redo(&mut self) -> bool {
        self.run_history(HistoryAction::Redo)
    }

    fn run_history(&mut self, action: HistoryAction) -> bool {
        let result = {
            let mut state = self.state.borrow_mut();
            match action {
                HistoryAction::Undo => state.undo(),
                HistoryAction::Redo => state.redo(),
            }
        };
        let message = match result {
            Some(label) => {
                self.texture_dirty = true;
                format!("{}：{label}", action.label())
            }
            None => format!("没有可{}的操作", action.label()),
        };
        *self.message.borrow_mut() = Some(message);
        result.is_some()
    }

    pub fn can_undo(&self) -> bool {
        self.state.borrow().can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.state.borrow().can_redo()
    }

    /// 最近一条撤销命令改动的像素区域（自检核对「撤销区域贴着笔迹」用）。
    pub fn undo_region(&self) -> Option<crate::document::PixelRegion> {
        self.state.borrow().history.undo_region()
    }

    /// 撤销 / 重做栈的长度，测试与自检用。
    pub fn history_len(&self) -> (usize, usize) {
        let state = self.state.borrow();
        (state.history.undo_len(), state.history.redo_len())
    }

    /// 图标包里索引到的图标数量（vendor 子集为 20，完整包为 2000+）。
    pub fn icon_count(&self) -> usize {
        self.icon_count
    }

    /// 工具所在**组按钮**的中心点（逻辑坐标）。测试与自检用它模拟一次真实点击；
    /// 短按组按钮激活的是该组当前的「脸」。
    pub fn tool_center(&self, tool: ActiveTool) -> Option<Vec2> {
        let id = self.tool_node(tool)?;
        igui_ui::control(&self.tree, id).map(|control| control.rect.center())
    }

    /// 工具所在组按钮的节点 id。
    pub fn tool_node(&self, tool: ActiveTool) -> Option<NodeId> {
        let group = group_index(tool)?;
        self.tool_group_nodes.get(group).copied()
    }

    /// 某个组按钮的中心点（下标对应 `TOOL_GROUPS`）。
    #[cfg(test)]
    pub fn tool_group_center(&self, group: usize) -> Option<Vec2> {
        let id = *self.tool_group_nodes.get(group)?;
        igui_ui::control(&self.tree, id).map(|control| control.rect.center())
    }

    /// 某个组当前的「脸」工具（测试 / 自检）。
    #[cfg(test)]
    pub fn tool_face(&self, group: usize) -> Option<ActiveTool> {
        self.tool_faces.get(group).map(|face| face.get())
    }

    /// 当前展开的工具浮层属于哪个组。
    #[cfg(test)]
    pub fn tool_flyout_open(&self) -> Option<usize> {
        self.open_tool_group.get()
    }

    /// 编辑器是否在等长按阈值（宿主据此持续出帧）。
    pub fn needs_frame(&self) -> bool {
        matches!(
            &*self.long_press.borrow(),
            toolbar::LongPress::Pending { .. }
        )
    }

    /// 测试：把长按阈值改成 `ms`（0 = 下一帧就展开，大值 = 当作短按）。
    #[cfg(test)]
    pub fn set_long_press_ms(&mut self, ms: u64) {
        self.long_press_threshold = Duration::from_millis(ms);
    }

    /// 撤销 / 重做按钮的中心点（逻辑坐标）；测试与自检模拟点击用。
    pub fn history_center(&self, action: HistoryAction) -> Option<Vec2> {
        let id = self.history_node(action)?;
        igui_ui::control(&self.tree, id).map(|control| control.rect.center())
    }

    /// 撤销 / 重做按钮的节点 id。
    pub fn history_node(&self, action: HistoryAction) -> Option<NodeId> {
        self.history_nodes
            .iter()
            .find(|(candidate, _)| *candidate == action)
            .map(|(_, id)| *id)
    }

    /// 图层面板「重命名」按钮的节点（重命名浮层的锚点）。
    pub(super) fn rename_anchor(&self) -> Option<NodeId> {
        self.tree.iter().find(|&node| {
            igui_ui::content(&self.tree, node).and_then(|content| content.as_text())
                == Some("重命名")
        })
    }

    /// 图层面板「重命名」按钮的中心（点它弹重命名浮层）。
    pub fn rename_button_center(&self) -> Option<Vec2> {
        let node = self.rename_anchor()?;
        igui_ui::control(&self.tree, node).map(|control| control.rect.center())
    }

    /// 重命名浮层的输入框句柄（浮层开着时才 `Some`）。
    pub fn rename_field(&self) -> Option<Rc<RefCell<igui_ui::TextEdit>>> {
        self.rename
            .as_ref()
            .and_then(|rename| rename.edit.borrow().clone())
    }

    /// 图层行的可见数量（折叠的组不计子节点）。
    pub fn layer_row_count(&self) -> usize {
        self.layer_rows.borrow().len()
    }

    /// 图层行的名字列（含 `▾` / `◧` 前缀），从上到下。
    pub fn layer_row_names(&self) -> Vec<String> {
        self.layer_rows
            .borrow()
            .iter()
            .map(|row| {
                let mut name = String::new();
                if row.is_group {
                    name.push_str(if row.expanded { "▾ " } else { "▸ " });
                }
                if row.has_mask {
                    name.push_str(if row.mask_enabled { "◧ " } else { "◨ " });
                }
                name.push_str(&row.name);
                name
            })
            .collect()
    }

    /// 第 `row` 行第 `cell` 个格子的中心点（测试模拟点击格子用）。
    ///
    /// 格子顺序和 `List::on_cell` 一致：leads（缩进 / 三角 / 蒙版）在前，
    /// 文本列（眼睛 / 名字 / 不透明度）在后。
    pub fn layer_cell_center(&self, row: usize, cell: usize) -> Option<Vec2> {
        let range = self.layer_state.visible_range();
        if !range.contains(&row) {
            return None;
        }
        let node = *self.layer_state.rows().get(row - range.start)?;
        let child = *self.tree.children(node)?.get(cell)?;
        igui_ui::control(&self.tree, child).map(|control| control.rect.center())
    }

    /// 第 `index` 个可见图层行的中心点（测试模拟点击用）。
    #[allow(dead_code)] // 只有测试点整行（自检点的是格子）。
    pub fn layer_row_center(&self, index: usize) -> Option<Vec2> {
        let range = self.layer_state.visible_range();
        if !range.contains(&index) {
            return None;
        }
        let node = *self.layer_state.rows().get(index - range.start)?;
        igui_ui::control(&self.tree, node).map(|control| control.rect.center())
    }

    /// 调色盘色块数量。
    pub fn palette_swatch_count(&self) -> usize {
        self.palette_nodes.len()
    }

    /// 调色盘第 `index` 个色块的中心点；测试与自检模拟点击用。
    pub fn palette_swatch_center(&self, index: usize) -> Option<Vec2> {
        let id = *self.palette_nodes.get(index)?;
        igui_ui::control(&self.tree, id).map(|control| control.rect.center())
    }

    /// 取色器饱和 / 明度方块的中心点；测试与自检模拟拖动用。
    pub fn palette_picker_center(&self) -> Option<Vec2> {
        igui_ui::control(&self.tree, self.layout.palette_picker)
            .map(|control| control.rect.center())
    }

    /// 选项栏 `−` / `+` 按钮的节点。
    pub fn brush_adjust_node(&self, adjust: BrushAdjust) -> Option<NodeId> {
        self.options
            .brush_buttons
            .iter()
            .find(|(candidate, _)| *candidate == adjust)
            .map(|(_, id)| *id)
    }

    /// 选项栏 `−` / `+` 按钮的中心点（逻辑坐标）；测试与自检模拟点击用。
    pub fn brush_adjust_center(&self, adjust: BrushAdjust) -> Option<Vec2> {
        let id = self.brush_adjust_node(adjust)?;
        igui_ui::control(&self.tree, id).map(|control| control.rect.center())
    }

    /// 当前笔刷大小（逻辑像素）。
    pub fn brush_size(&self) -> f32 {
        self.brush.size
    }

    /// 钢笔：当前路径的锚点数 / 是否闭合 / 选项栏摘要。
    pub fn pen_anchor_count(&self) -> usize {
        self.pen.path().len()
    }

    pub fn pen_path_closed(&self) -> bool {
        self.pen.path().is_closed()
    }

    /// 钢笔动作按钮的中心点（测试与自检点击用）。
    pub fn pen_action_node(&self, action: PenAction) -> Option<NodeId> {
        self.options
            .pen_buttons
            .iter()
            .find(|(candidate, _)| *candidate == action)
            .map(|(_, id)| *id)
    }

    pub fn pen_action_center(&self, action: PenAction) -> Option<Vec2> {
        let id = self.pen_action_node(action)?;
        igui_ui::control(&self.tree, id).map(|control| control.rect.center())
    }

    /// 当前对称绘画模式 / 循环按钮的中心点。
    pub fn symmetry_mode(&self) -> crate::tools::Symmetry {
        self.options.symmetry_mode.get()
    }

    pub fn symmetry_button_center(&self) -> Option<Vec2> {
        igui_ui::control(&self.tree, self.options.symmetry_button)
            .map(|control| control.rect.center())
    }

    /// 当前笔刷硬度（0..=1）。
    pub fn brush_hardness(&self) -> f32 {
        self.brush.hardness
    }

    /// 当前落笔间距（相对直径的比例）。
    pub fn brush_spacing(&self) -> f32 {
        self.brush.spacing_ratio
    }

    /// 当前平滑强度（0 = 折线、1 = 二次中点平滑）。
    pub fn brush_smooth(&self) -> f32 {
        self.brush.smooth
    }

    /// 当前笔刷不透明度（0..=1）。
    pub fn brush_opacity(&self) -> f32 {
        self.brush.opacity
    }

    /// 选项栏整行的矩形（宿主与测试验证自上而下的顺序用）。
    pub fn options_bar_rect(&self) -> Option<Rect> {
        igui_ui::control(&self.tree, self.options.panel).map(|control| control.rect)
    }

    /// 选项栏末尾「全部属性…」按钮的中心点（测试 / 自检点击用）。
    pub fn settings_button_center(&self) -> Option<Vec2> {
        igui_ui::control(&self.tree, self.options.settings_button)
            .map(|control| control.rect.center())
    }

    /// 选项栏「预设」触发按钮的中心点（测试与自检点击用）。
    pub fn brush_preset_center(&self) -> Option<Vec2> {
        igui_ui::control(&self.tree, self.options.preset).map(|control| control.rect.center())
    }

    /// 选项栏显示的笔刷预设名（命中不到预设时是「自定义」）。
    pub fn brush_preset_label(&self) -> String {
        crate::tools::brush::preset::label(crate::tools::brush::preset::PRESETS, &self.brush)
    }

    /// 画笔当前的硬边状态（像素模式同步后的结果）。
    pub fn brush_hard(&self) -> bool {
        self.brush.hard
    }

    /// 画笔当前的形状。
    pub fn brush_shape(&self) -> BrushShape {
        self.brush.shape
    }

    /// 像素模式开关是否打开（硬边）。
    pub fn pixel_mode(&self) -> bool {
        self.options.pixel_mode.get()
    }

    /// 方形笔开关是否打开。
    pub fn square_mode(&self) -> bool {
        self.options.square_mode.get()
    }

    /// 选项栏开关按钮的节点。
    pub fn brush_toggle_node(&self, toggle: BrushToggle) -> Option<NodeId> {
        self.options
            .brush_toggles
            .iter()
            .find(|(candidate, _)| *candidate == toggle)
            .map(|(_, id)| *id)
    }

    /// 选项栏开关按钮的中心点（逻辑坐标）；测试与自检模拟点击用。
    pub fn brush_toggle_center(&self, toggle: BrushToggle) -> Option<Vec2> {
        let id = self.brush_toggle_node(toggle)?;
        igui_ui::control(&self.tree, id).map(|control| control.rect.center())
    }

    /// 油漆桶选项栏按钮 / 开关的节点与中心点（测试 / 自检点击用）。
    pub fn bucket_button_node(&self, adjust: BucketAdjust) -> Option<NodeId> {
        self.options
            .bucket_buttons
            .iter()
            .find(|(candidate, _)| *candidate == adjust)
            .map(|(_, id)| *id)
    }

    pub fn bucket_button_center(&self, adjust: BucketAdjust) -> Option<Vec2> {
        let id = self.bucket_button_node(adjust)?;
        igui_ui::control(&self.tree, id).map(|control| control.rect.center())
    }

    pub fn bucket_toggle_node(&self, toggle: BucketToggle) -> Option<NodeId> {
        self.options
            .bucket_toggles
            .iter()
            .find(|(candidate, _)| *candidate == toggle)
            .map(|(_, id)| *id)
    }

    pub fn bucket_toggle_center(&self, toggle: BucketToggle) -> Option<Vec2> {
        let id = self.bucket_toggle_node(toggle)?;
        igui_ui::control(&self.tree, id).map(|control| control.rect.center())
    }

    /// 渐变类型切换按钮 / 「编辑…」按钮的中心点（测试 / 自检点击用）。
    pub fn gradient_button_center(&self, adjust: GradientAdjust) -> Option<Vec2> {
        let id = self
            .gradient
            .buttons
            .iter()
            .find(|(candidate, _)| *candidate == adjust)
            .map(|(_, id)| *id)?;
        igui_ui::control(&self.tree, id).map(|control| control.rect.center())
    }

    pub fn gradient_edit_center(&self) -> Option<Vec2> {
        igui_ui::control(&self.tree, self.gradient.edit_button).map(|control| control.rect.center())
    }

    /// 当前渐变类型 / ramp 色标 / 是否自定义 / 编辑器是否开着（测试 / 自检）。
    pub fn gradient_kind(&self) -> GradientKind {
        self.gradient.kind
    }

    pub fn gradient_stops(&self) -> Vec<(f32, Color)> {
        self.gradient
            .ramp
            .borrow()
            .stops()
            .iter()
            .map(|stop| (stop.position, stop.color))
            .collect()
    }

    pub fn gradient_editor_open(&self) -> bool {
        self.gradient.editor_open.get()
    }

    /// 油漆桶当前容差 / 是否连续（测试 / 自检）。
    pub fn bucket_tolerance(&self) -> u8 {
        self.paint_bucket.tolerance
    }

    pub fn bucket_contiguous(&self) -> bool {
        self.paint_bucket.contiguous
    }

    /// 右栏当前宽度（逻辑像素）。
    pub fn sidebar_width(&self) -> f32 {
        self.layout.sidebar_width.get()
    }

    /// 右栏分隔条的中心点（逻辑坐标）；测试与自检模拟拖动用。
    pub fn sidebar_handle_center(&self) -> Option<Vec2> {
        igui_ui::control(&self.tree, self.layout.sidebar_handle)
            .map(|control| control.rect.center())
    }

    /// 左侧调色盘面板的宽度 / 分隔条中心。
    pub fn palette_width(&self) -> f32 {
        self.layout.palette_width.get()
    }

    pub fn palette_handle_center(&self) -> Option<Vec2> {
        igui_ui::control(&self.tree, self.layout.palette_handle)
            .map(|control| control.rect.center())
    }

    /// 右侧栏标签页面板的当前高度（逻辑像素）。
    pub fn tabs_height(&self) -> f32 {
        self.layout.tabs_height.get()
    }

    /// 标签页面板与「图层」面板之间分隔条的中心点（逻辑坐标）。
    pub fn tabs_handle_center(&self) -> Option<Vec2> {
        igui_ui::control(&self.tree, self.layout.tabs_handle).map(|control| control.rect.center())
    }

    /// 当前标签页。
    pub fn active_tab(&self) -> SidebarTab {
        self.layout.active_tab.get()
    }

    /// 某个标签按钮的中心点（逻辑坐标）；测试与自检模拟点击用。
    pub fn tab_center(&self, tab: SidebarTab) -> Option<Vec2> {
        let id = self
            .layout
            .tab_buttons
            .iter()
            .find(|(candidate, _)| *candidate == tab)
            .map(|(_, id)| *id)?;
        igui_ui::control(&self.tree, id).map(|control| control.rect.center())
    }

    /// 某个标签页内容当前是否可见。
    pub fn tab_content_visible(&self, tab: SidebarTab) -> bool {
        self.layout
            .tab_contents
            .iter()
            .find(|(candidate, _)| *candidate == tab)
            .and_then(|(_, id)| self.tree.is_visible(*id))
            .unwrap_or(false)
    }

    /// 历史面板的行数（撤销栈 + 「当前」+ 重做栈）。
    pub fn history_rows(&self) -> usize {
        self.history_count.get()
    }

    /// 菜单标题按钮的中心点（逻辑坐标）；测试与自检模拟点击用。
    pub fn menu_center(&self, index: usize) -> Option<Vec2> {
        let id = *self.menu_nodes.get(index)?;
        igui_ui::control(&self.tree, id).map(|control| control.rect.center())
    }

    /// 直接设置当前选区（测试 / 自检造场景；`None` = 取消选择）。
    pub fn set_selection(&mut self, selection: Option<Selection>) {
        self.state.borrow_mut().selection = selection.map(Rc::new);
    }

    /// 当前是否有下拉菜单打开。
    pub fn menu_open(&self) -> bool {
        !self.overlays.is_empty()
    }

    /// 当前打开菜单的下标（没有打开则为 `None`）。
    pub fn open_menu_index(&self) -> Option<usize> {
        self.open_menu.get()
    }

    /// 当前文档的图层数（自检核对导入结果用）。
    pub fn layer_count(&self) -> usize {
        self.state.borrow().document.layers.len()
    }

    /// 当前前景色（吸管取色后测试 / 自检用）。
    pub fn foreground(&self) -> Color {
        self.state.borrow().foreground
    }

    /// 当前选区（框选后测试 / 自检用）。
    pub fn selection(&self) -> Option<Rc<Selection>> {
        self.state.borrow().selection.clone()
    }

    /// 当前图层的像素偏移（移动工具的效果，测试 / 自检用）。
    pub fn active_layer_position(&self) -> Option<Point> {
        self.state
            .borrow()
            .document
            .active_layer()
            .map(|layer| layer.position)
    }

    /// 请求一个文件动作（快捷键用）。
    pub fn request_file(&self, request: FileRequest) {
        self.intents.push(Intent::File(request));
    }

    /// 菜单 / 快捷键留下的文件意图；取走一个。
    pub fn take_file_request(&self) -> Option<FileRequest> {
        self.intents.take_file()
    }

    /// 菜单 / 选项栏要求打开模拟窗口（属性 / 关于 / 快捷键）；取走一个。
    pub fn take_dialog_request(&self) -> Option<crate::ui::DialogKind> {
        self.intents.take_dialog()
    }

    /// 意图队列（属性窗口与选项栏共用同一份：窗口里的 `−` / `+` 也进这个队列）。
    pub fn intents(&self) -> Intents {
        self.intents.clone()
    }

    /// 写一条状态栏提示（宿主保存 / 打开工作区后回报结果）。
    pub fn set_message(&self, text: impl Into<String>) {
        *self.message.borrow_mut() = Some(text.into());
    }

    /// 控件数（自检的预算检查用）。
    pub fn control_count(&self) -> usize {
        igui_ui::control_count(&self.tree)
    }
}
