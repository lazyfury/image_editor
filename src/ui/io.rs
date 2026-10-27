//! PNG 导入 / 导出：视图只负责合成 / 解码，写文件是宿主的活（路径由原生
//! 对话框给）。

use std::path::Path;

use crate::document::{AddLayerCommand, Layer};
use crate::renderer::{RenderTarget, Renderer};

use super::EditorView;

impl EditorView {
    /// 把当前文档合成后写成 PNG。
    pub fn export_png_to(&self, path: &str) -> Result<(), crate::io::IoError> {
        let state = self.state.borrow();
        let mut target = RenderTarget::new(0, 0);
        self.renderer.render(&state.document, &mut target);
        crate::io::write_png(Path::new(path), &target.pixels)
    }

    /// 从 PNG 文件导入一个新图层（放在最上面并选中）。
    pub fn import_png_from(&mut self, path: &str) -> Result<(u32, u32), crate::io::IoError> {
        let pixels = crate::io::read_png(Path::new(path))?;
        let size = (pixels.width, pixels.height);
        let name = crate::io::file_label(Path::new(path));
        let mut state = self.state.borrow_mut();
        let before_active = state.document.active_layer;
        let (parent, index) = state.document.insertion_slot();
        let layer = Layer::new(name, pixels);
        state.execute(Box::new(AddLayerCommand::new(
            layer,
            parent,
            index,
            before_active,
        )));
        drop(state);
        self.texture_dirty = true;
        Ok(size)
    }
}
