//! 布局与几何：面板尺寸、分隔条 / 面板高度夹取，以及画布区域与坐标换算。
//!
//! 可拖动的尺寸状态收在 [`Layout`] 里（[`EditorView`] 的 `layout` 字段）：三条
//! 分隔条各自拖动的宽度 / 高度 + 侧栏标签页的当前页与按钮 / 内容节点。

use igui_core::{NodeId, Rect, Vec2};

use crate::canvas::screen_to_document;

use super::*;

/// 可拖动的面板尺寸 + 侧栏标签页状态。
///
/// 宽度 / 高度用 `Rc<Cell<f32>>`：分隔条控件（`ResizeHandle`）直接写，`layout`
/// 每帧按窗口大小再夹取一次；节点是分隔条与内容容器的目标，标签页节点给测试和
/// 自检用来「真的点一下」。
pub(super) struct Layout {
    /// 右侧栏宽度（分隔条写、`clamp_sidebar_width` 也写）。
    pub(super) sidebar_width: Rc<Cell<f32>>,
    /// 右侧栏节点（分隔条的目标）。
    pub(super) sidebar: NodeId,
    /// 右侧栏分隔条的节点。
    pub(super) sidebar_handle: NodeId,
    /// 左侧调色盘面板的宽度 / 节点 / 分隔条。
    pub(super) palette_width: Rc<Cell<f32>>,
    pub(super) palette_panel: NodeId,
    pub(super) palette_picker: NodeId,
    pub(super) palette_handle: NodeId,
    /// 侧栏标签页：当前页（按钮写）、面板高度 / 节点 / 分隔条。
    pub(super) active_tab: Rc<Cell<SidebarTab>>,
    /// 上一次同步到树的标签页；变了才重设可见性 + 标脏重排。
    pub(super) shown_tab: Option<SidebarTab>,
    pub(super) tabs_height: Rc<Cell<f32>>,
    pub(super) tabs_panel: NodeId,
    pub(super) tabs_handle: NodeId,
    /// 标签按钮 / 内容容器节点，测试与自检靠它们真的点一下。
    pub(super) tab_buttons: Vec<(SidebarTab, NodeId)>,
    pub(super) tab_contents: Vec<(SidebarTab, NodeId)>,
}

impl EditorView {
    /// 排布整棵树，并把相机同步到文档节点。
    pub fn layout(&mut self, viewport: ViewportSize) {
        self.viewport = viewport;
        self.tree.set_viewport_size(viewport.logical_size());
        self.clamp_palette_width();
        self.clamp_sidebar_width();
        igui_ui::layout(&mut self.tree, viewport);
        // 面板高度按刚拿到的侧栏矩形钳制一次；变了就再排一遍。
        if self.clamp_panel_heights() {
            igui_ui::layout(&mut self.tree, viewport);
        }
        // 第一次布局拿到画布区域的真实矩形，才能做适配。
        if !self.fitted {
            self.fit_to_canvas();
            self.fitted = true;
        }
        // 图层列表的行池按容器高度算，所以要在 layout 之后 sync；池变了再排一次。
        if self.layer_state.sync(&mut self.tree) {
            igui_ui::layout(&mut self.tree, viewport);
        }
        // 缩略图重建要在行池 sync 之后：只缩视口里那几行（节流由时间窗口管）。
        if self.layer_thumb_dirty && self.thumbnails_due() {
            self.layer_thumb_dirty = false;
            self.mark_thumbnails_rebuilt();
            self.rebuild_layer_thumbnails();
        }
        if self.history_state.sync(&mut self.tree) {
            igui_ui::layout(&mut self.tree, viewport);
        }
        // 覆盖层用自己的树，必须在主树排完之后定位。
        self.overlays.layout(&self.tree, viewport);
        self.tree.update();
    }

    pub(super) fn canvas_area_rect(&self) -> Option<Rect> {
        igui_ui::control(&self.tree, self.canvas_area).map(|control| control.rect)
    }

    /// 分隔条只管 `[min, max]`，管不了"窗口变窄了"。那一半在这里：右栏最多
    /// 占到给画布留 [`CANVAS_MIN`] 为止（`examples/file_browser` 同款）。
    pub(super) fn clamp_sidebar_width(&mut self) {
        let full = self.viewport.logical_size().width;
        let max = (full
            - TOOLBAR_WIDTH
            - self.layout.palette_width.get()
            - 2.0 * RESIZE_GUTTER
            - CANVAS_MIN)
            .max(SIDEBAR_MIN);
        let current = self.layout.sidebar_width.get();
        let next = current.clamp(SIDEBAR_MIN, max);
        if (next - current).abs() > f32::EPSILON {
            self.layout.sidebar_width.set(next);
            update_control(&mut self.tree, self.layout.sidebar, |data| {
                data.layout.basis = SizeBasis::Px(next);
            });
        }
    }

    /// 左侧调色盘面板的宽度不能把画布 / 右栏挤没。
    pub(super) fn clamp_palette_width(&mut self) {
        let full = self.viewport.logical_size().width;
        let max = (full - TOOLBAR_WIDTH - 2.0 * RESIZE_GUTTER - SIDEBAR_MIN - CANVAS_MIN)
            .clamp(PALETTE_MIN, PALETTE_MAX);
        let current = self.layout.palette_width.get();
        let next = current.clamp(PALETTE_MIN, max);
        if (next - current).abs() > f32::EPSILON {
            self.layout.palette_width.set(next);
            update_control(&mut self.tree, self.layout.palette_panel, |data| {
                data.layout.basis = SizeBasis::Px(next);
            });
        }
    }

    /// 保证可拖的标签页面板不会把下面的「图层」面板挤没。
    /// 返回是否调整过（调用方需要再排一次）。
    pub(super) fn clamp_panel_heights(&mut self) -> bool {
        let Some(sidebar) = igui_ui::control(&self.tree, self.layout.sidebar) else {
            return false;
        };
        // 预留：图层最小高度 + 分隔条 + 两个子节点之间的列间距。
        let overhead = LAYER_PANEL_MIN + RESIZE_GUTTER + space::XXXS;
        let max = (sidebar.rect.size.height - overhead).clamp(TABS_PANEL_MIN, TABS_PANEL_MAX);
        let current = self.layout.tabs_height.get();
        let next = current.clamp(TABS_PANEL_MIN, max);
        if (next - current).abs() <= f32::EPSILON {
            return false;
        }
        self.layout.tabs_height.set(next);
        update_control(&mut self.tree, self.layout.tabs_panel, |data| {
            data.layout.basis = SizeBasis::Px(next);
        });
        true
    }

    pub(super) fn canvas_area_contains(&self, position: Vec2) -> bool {
        self.canvas_area_rect()
            .is_some_and(|rect| rect.contains(position))
    }

    // -- 工具 ------------------------------------------------------------

    /// 屏幕逻辑坐标 -> 文档坐标。
    pub(super) fn document_position(&self, screen: Vec2) -> Vec2 {
        let camera = self.state.borrow().canvas;
        screen_to_document(camera, screen)
    }
}
