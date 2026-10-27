//! 选区 / 剪贴板 / 删除：`Ctrl/Cmd+X`、`C`、`V` 与 `Delete` 的像素操作。
//!
//! 每次操作都是「快照 → 改当前图层 → 提交一条 [`PaintCommand`]」，所以和画笔
//! 一样一步 undo；区域用选区包围盒（`PixelRegion::in_buffer`）算出来，不扫整层。
//!
//! 剪贴板本身在 [`crate::tools::PixelClipboard`]，视图只持有共享句柄（宿主跨
//! 工作区共享一个，和文本剪贴板同一个套路）。
//!
//! [`PaintCommand`]: crate::document::PaintCommand

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::state::AppState;
use crate::canvas::Selection;
use crate::document::{AddLayerCommand, Layer, PaintCommand, PixelRegion, Point};
use crate::tools::PixelClipboard;

use super::EditorView;

impl EditorView {
    /// 换一块共享的像素剪贴板（宿主跨工作区共享；测试可以直接塞一块）。
    pub fn set_image_clipboard(&mut self, clipboard: Rc<RefCell<Option<PixelClipboard>>>) {
        self.image_clipboard = clipboard;
    }

    /// 剪贴板里有没有东西（菜单「粘贴」能不能点）。
    pub fn has_clipboard(&self) -> bool {
        self.image_clipboard
            .borrow()
            .as_ref()
            .is_some_and(|clipboard| !clipboard.is_empty())
    }

    /// 复制选区内容（不动像素）。
    pub fn copy_selection(&mut self) -> bool {
        if self.state.borrow().document.active_mask {
            *self.message.borrow_mut() = Some("蒙版编辑时不能复制图层像素".to_string());
            return false;
        }
        let selection = self.state.borrow().selection.clone();
        let Some(selection) = selection else {
            *self.message.borrow_mut() = Some("没有选区可复制".to_string());
            return false;
        };
        let captured = {
            let state = self.state.borrow();
            state
                .document
                .active_layer()
                .and_then(|layer| PixelClipboard::capture(layer, &selection))
        };
        let Some(clipboard) = captured else {
            *self.message.borrow_mut() = Some("选区里没有可复制的内容".to_string());
            return false;
        };
        let (width, height) = clipboard.size();
        // 顺手写一份到系统剪贴板（跨 App 粘贴）；失败只记日志，不影响进程内复制。
        if let Err(error) = crate::io::write_clipboard_image(&clipboard.to_buffer()) {
            tracing::debug!(target: "image_editor", %error, "system_clipboard_write_failed");
        }
        *self.image_clipboard.borrow_mut() = Some(clipboard);
        *self.message.borrow_mut() = Some(format!("已复制 {width} × {height}"));
        true
    }

    /// 剪切：复制 + 清空选区里的像素（一步 undo）。
    pub fn cut_selection(&mut self) -> bool {
        if !self.copy_selection() {
            return false;
        }
        if !self.delete_selection() {
            // 选区里本来就什么都没有：复制成功也算成功。
            return true;
        }
        *self.message.borrow_mut() = Some("已剪切".to_string());
        true
    }

    /// 粘贴：把剪贴板贴回原位置，按当前选区的覆盖率裁剪（一步 undo）。
    pub fn paste_clipboard(&mut self) -> bool {
        let Some(clipboard) = self.clipboard_source() else {
            *self.message.borrow_mut() = Some("剪贴板是空的".to_string());
            return false;
        };
        let selection = self.state.borrow().selection.clone();
        let Some(origin) = Some(clipboard.origin) else {
            return false;
        };
        let changed = self.edit_active_layer("粘贴", origin, |layer| {
            clipboard.paste(layer, selection.as_deref())
        });
        if changed {
            *self.message.borrow_mut() = Some(format!(
                "已粘贴 {} × {}",
                clipboard.origin.width, clipboard.origin.height
            ));
        }
        changed
    }

    /// 粘贴为**新图层**：把剪贴板（或系统剪贴板里的图）放到它原来的位置。
    ///
    /// 和 [`Self::paste_clipboard`] 的区别是不动当前图层的像素、也不受选区裁剪 ——
    /// 换来的是一条 `AddLayerCommand`（一步 undo），可以随便移动 / 隐藏。
    pub fn paste_as_new_layer(&mut self) -> bool {
        let Some(clipboard) = self.clipboard_source() else {
            *self.message.borrow_mut() = Some("剪贴板是空的".to_string());
            return false;
        };
        let (width, height) = (clipboard.origin.width, clipboard.origin.height);
        let mut layer = Layer::new("粘贴", clipboard.to_buffer());
        layer.position = Point::new(clipboard.origin.x as i32, clipboard.origin.y as i32);
        let mut state = self.state.borrow_mut();
        let before_active = state.document.active_layer;
        let (parent, index) = state.document.insertion_slot();
        state.execute(Box::new(AddLayerCommand::new(
            layer,
            parent,
            index,
            before_active,
        )));
        drop(state);
        self.mark_texture_dirty();
        *self.message.borrow_mut() = Some(format!("已粘贴为新图层 {width} × {height}"));
        true
    }

    /// 取一份可粘贴的内容：进程内剪贴板优先，没有就去系统剪贴板要一张图
    /// （外来图片放在选区左上角，没有选区就放文档左上角）。
    fn clipboard_source(&mut self) -> Option<PixelClipboard> {
        if let Some(clipboard) = self.image_clipboard.borrow().clone() {
            return Some(clipboard);
        }
        let pixels = crate::io::read_clipboard_image()?;
        let selection = self.state.borrow().selection.clone();
        let origin = foreign_origin(selection.as_deref(), (pixels.width, pixels.height));
        let colors = (0..pixels.height)
            .flat_map(|y| (0..pixels.width).map(move |x| (x, y)))
            .map(|(x, y)| pixels.get_pixel(x, y))
            .collect();
        let clipboard = PixelClipboard::from_pixels(origin, colors);
        *self.image_clipboard.borrow_mut() = Some(clipboard.clone());
        Some(clipboard)
    }

    /// 删除选区内容（`Delete` / `Backspace`）：把选区里的 alpha 按覆盖率清掉。
    pub fn delete_selection(&mut self) -> bool {
        if self.state.borrow().document.active_mask {
            *self.message.borrow_mut() = Some("蒙版编辑时不能删图层像素".to_string());
            return false;
        }
        let selection = self.state.borrow().selection.clone();
        let Some(selection) = selection else {
            return false;
        };
        let Some(bounds) = selection.bounds() else {
            return false;
        };
        let changed = self.edit_active_layer("删除", bounds, |layer| {
            PixelClipboard::clear_selection(layer, &selection)
        });
        if changed {
            *self.message.borrow_mut() = Some(format!("已删除 {} 像素", selection.area()));
        }
        changed
    }

    /// 在当前图层上做一次「一步 undo」的像素编辑：区域用 `region`（文档坐标）。
    fn edit_active_layer(
        &mut self,
        label: &'static str,
        region: PixelRegion,
        edit: impl FnOnce(&mut Layer) -> bool,
    ) -> bool {
        let changed = {
            let mut state = self.state.borrow_mut();
            let AppState {
                document, history, ..
            } = &mut *state;
            let Some(id) = document.active_layer().map(|layer| layer.id) else {
                return false;
            };
            let (dx, dy) = document.ensure_layer_covers_document(id);
            history.translate_layer(id, dx, dy);
            let Some(before) = document.layer(id).map(|layer| layer.pixels.clone()) else {
                return false;
            };
            let changed = match document.layer_mut(id) {
                Some(layer) => edit(layer),
                None => false,
            };
            if !changed {
                return false;
            }
            let command = document.layer(id).and_then(|layer| {
                let buffer_region = region.in_buffer(
                    (layer.position.x, layer.position.y),
                    layer.pixels.width,
                    layer.pixels.height,
                );
                PaintCommand::capture_in(id, buffer_region, &before, &layer.pixels, label)
            });
            match command {
                Some(command) => {
                    history.execute(Box::new(command), document);
                    true
                }
                None => false,
            }
        };
        if changed {
            self.mark_texture_dirty();
        }
        changed
    }
}

/// 外来图片（系统剪贴板）贴到哪：有选区就对齐选区左上角，否则文档左上角。
fn foreign_origin(selection: Option<&Selection>, size: (u32, u32)) -> PixelRegion {
    let (x, y) = selection
        .and_then(|selection| selection.bounds())
        .map(|bounds| (bounds.x, bounds.y))
        .unwrap_or((0, 0));
    PixelRegion::new(x, y, size.0, size.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AppState;
    use crate::document::{Color, PixelRegion};

    fn view() -> EditorView {
        EditorView::new(crate::theme::editor_theme(false), AppState::default())
    }

    fn select(view: &mut EditorView, region: PixelRegion) {
        let document = (128, 128);
        view.state.borrow_mut().selection =
            Some(Rc::new(crate::canvas::Selection::rect(region, document)));
    }

    fn pixel(view: &EditorView, x: u32, y: u32) -> Color {
        let state = view.state.borrow();
        state
            .document
            .active_layer()
            .unwrap()
            .pixels
            .get_pixel(x, y)
    }

    fn paint_rect(view: &mut EditorView, region: PixelRegion, color: Color) {
        let mut state = view.state.borrow_mut();
        let id = state.document.active_layer().unwrap().id;
        let layer = state.document.layer_mut(id).unwrap();
        for y in region.y..region.bottom() {
            for x in region.x..region.right() {
                layer.pixels.set_pixel(x, y, color);
            }
        }
    }

    #[test]
    fn cut_then_paste_round_trips_the_pixels_and_one_undo_each() {
        let mut view = view();
        let region = PixelRegion::new(4, 4, 4, 4);
        paint_rect(&mut view, region, Color::RED);
        select(&mut view, region);
        let before_undo = view.history_len().0;

        assert!(view.copy_selection());
        assert!(view.has_clipboard());
        assert!(view.cut_selection());
        assert_eq!(view.history_len().0, before_undo + 1, "剪切 = 一步 undo");
        assert_eq!(pixel(&view, 5, 5).a, 0, "剪切后选区变透明");

        assert!(view.paste_clipboard());
        assert_eq!(view.history_len().0, before_undo + 2, "粘贴 = 一步 undo");
        assert_eq!(pixel(&view, 5, 5), Color::RED, "贴回原位");
        assert_eq!(pixel(&view, 9, 9), Color::WHITE, "不会糊到选区外");

        // 两次 undo 都回到最开始。
        view.undo();
        view.undo();
        assert_eq!(pixel(&view, 5, 5), Color::RED, "撤销回原始像素");
    }

    #[test]
    fn deleting_clears_the_selection_and_is_one_undo() {
        let mut view = view();
        let region = PixelRegion::new(2, 2, 3, 3);
        paint_rect(&mut view, region, Color::RED);
        select(&mut view, region);
        let before_undo = view.history_len().0;
        assert!(view.delete_selection());
        assert_eq!(view.history_len().0, before_undo + 1);
        assert_eq!(pixel(&view, 3, 3).a, 0);
        assert_eq!(pixel(&view, 0, 0), Color::WHITE, "选区外不动");
        // 再删一次：没有像素变化，不加 undo。
        assert!(!view.delete_selection());
        assert_eq!(view.history_len().0, before_undo + 1);
    }

    #[test]
    fn copy_needs_a_selection_and_paste_needs_a_clipboard() {
        let mut view = view();
        assert!(!view.copy_selection(), "没有选区时复制是 no-op");
        assert!(!view.paste_clipboard(), "剪贴板空时粘贴是 no-op");
        assert!(!view.has_clipboard());
    }

    #[test]
    fn pasting_as_a_layer_adds_one_layer_and_keeps_the_current_pixels() {
        let mut view = view();
        let region = PixelRegion::new(4, 4, 2, 2);
        paint_rect(&mut view, region, Color::RED);
        select(&mut view, region);
        assert!(view.copy_selection());
        let layers_before = view.state.borrow().document.layers.len();
        let before_undo = view.history_len().0;
        let original = view.state.borrow().document.active_layer().unwrap().id;

        assert!(view.paste_as_new_layer());
        assert_eq!(
            view.state.borrow().document.layers.len(),
            layers_before + 1,
            "粘贴为新图层 = 多一层"
        );
        assert_eq!(view.history_len().0, before_undo + 1, "一步 undo");
        // 新图层在原位置，像素是复制出来的那些。
        let state = view.state.borrow();
        let layer = state.document.active_layer().unwrap();
        assert_eq!((layer.position.x, layer.position.y), (4, 4));
        assert_eq!(layer.pixels.get_pixel(0, 0), Color::RED);
        // 原图层的像素没被动过（按 id 读，活动图层这时已经是新粘贴的那一层）。
        let untouched = state
            .document
            .layer(original)
            .unwrap()
            .pixels
            .get_pixel(4, 4);
        assert_eq!(untouched, Color::RED);
    }

    #[test]
    fn a_foreign_image_lands_at_the_selection_origin_or_the_document_origin() {
        let selection = crate::canvas::Selection::rect(
            crate::document::PixelRegion::new(20, 30, 4, 4),
            (128, 128),
        );
        assert_eq!(
            foreign_origin(Some(&selection), (8, 6)),
            PixelRegion::new(20, 30, 8, 6)
        );
        assert_eq!(foreign_origin(None, (8, 6)), PixelRegion::new(0, 0, 8, 6));
    }

    #[test]
    fn pasting_is_clipped_to_the_current_selection() {
        let mut view = view();
        let region = PixelRegion::new(4, 4, 2, 2);
        paint_rect(&mut view, region, Color::RED);
        select(&mut view, region);
        assert!(view.copy_selection());
        assert!(view.delete_selection());
        // 换一个不重叠的选区：粘贴就不该生效。
        select(&mut view, PixelRegion::new(40, 40, 4, 4));
        assert!(!view.paste_clipboard());
        assert_eq!(pixel(&view, 5, 5).a, 0, "原位置仍然是空的");
    }
}
