//! 布局与几何：排树、分隔条 / 面板高度夹取，以及画布区域与坐标换算。

use igui_core::{Rect, Vec2};

use crate::canvas::screen_to_document;

use super::*;

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
        let max =
            (full - TOOLBAR_WIDTH - self.palette_width.get() - 2.0 * RESIZE_GUTTER - CANVAS_MIN)
                .max(SIDEBAR_MIN);
        let current = self.sidebar_width.get();
        let next = current.clamp(SIDEBAR_MIN, max);
        if (next - current).abs() > f32::EPSILON {
            self.sidebar_width.set(next);
            update_control(&mut self.tree, self.sidebar_node, |data| {
                data.layout.basis = SizeBasis::Px(next);
            });
        }
    }

    /// 左侧调色盘面板的宽度不能把画布 / 右栏挤没。
    pub(super) fn clamp_palette_width(&mut self) {
        let full = self.viewport.logical_size().width;
        let max = (full - TOOLBAR_WIDTH - 2.0 * RESIZE_GUTTER - SIDEBAR_MIN - CANVAS_MIN)
            .clamp(PALETTE_MIN, PALETTE_MAX);
        let current = self.palette_width.get();
        let next = current.clamp(PALETTE_MIN, max);
        if (next - current).abs() > f32::EPSILON {
            self.palette_width.set(next);
            update_control(&mut self.tree, self.palette_panel_node, |data| {
                data.layout.basis = SizeBasis::Px(next);
            });
        }
    }

    /// 保证可拖的标签页面板不会把下面的「图层」面板挤没。
    /// 返回是否调整过（调用方需要再排一次）。
    pub(super) fn clamp_panel_heights(&mut self) -> bool {
        let Some(sidebar) = igui_ui::control(&self.tree, self.sidebar_node) else {
            return false;
        };
        // 预留：图层最小高度 + 分隔条 + 两个子节点之间的列间距。
        let overhead = LAYER_PANEL_MIN + RESIZE_GUTTER + space::XXXS;
        let max = (sidebar.rect.size.height - overhead).clamp(TABS_PANEL_MIN, TABS_PANEL_MAX);
        let current = self.tabs_height.get();
        let next = current.clamp(TABS_PANEL_MIN, max);
        if (next - current).abs() <= f32::EPSILON {
            return false;
        }
        self.tabs_height.set(next);
        update_control(&mut self.tree, self.tabs_panel_node, |data| {
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
