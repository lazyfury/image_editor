//! 图层命令：增删、顺序 / 元数据、移动、裁到文档。
//!
//! 元数据类命令只存非像素状态（顺序 + 每层 name / visible / opacity / blend /
//! position + 当前图层），所以移动 / 重命名 / 可见性这些撤销代价很小。

use super::super::document::Document;
use super::super::id::LayerId;
use super::super::layer::{BlendMode, Layer};
use super::super::pixel_buffer::PixelBuffer;
use super::super::point::Point;
use super::Command;

#[derive(Debug, Clone, PartialEq)]
struct LayerMeta {
    id: LayerId,
    name: String,
    visible: bool,
    opacity: f32,
    blend_mode: BlendMode,
    position: Point,
}

/// 图层栈的非像素状态：顺序 + 每层元数据 + 当前图层。
///
/// 只存元数据，**不存像素**，所以移动 / 重命名 / 可见性 / 不透明度 / 排序
/// 这些操作的撤销代价极小。
#[derive(Debug, Clone, PartialEq)]
pub struct LayerStackMeta {
    layers: Vec<LayerMeta>,
    active: Option<LayerId>,
}

impl LayerStackMeta {
    fn capture(document: &Document) -> Self {
        Self {
            layers: document
                .layers
                .iter()
                .map(|layer| LayerMeta {
                    id: layer.id,
                    name: layer.name.clone(),
                    visible: layer.visible,
                    opacity: layer.opacity,
                    blend_mode: layer.blend_mode,
                    position: layer.position,
                })
                .collect(),
            active: document.active_layer,
        }
    }

    /// 把元数据 / 顺序 / 当前图层写回；像素不动（按 id 复用原 `Layer`）。
    fn restore(&self, document: &mut Document) {
        let mut current = std::mem::take(&mut document.layers);
        let mut restored = Vec::with_capacity(current.len());
        for meta in &self.layers {
            if let Some(index) = current.iter().position(|layer| layer.id == meta.id) {
                let mut layer = current.remove(index);
                layer.name.clone_from(&meta.name);
                layer.visible = meta.visible;
                layer.opacity = meta.opacity;
                layer.blend_mode = meta.blend_mode;
                layer.position = meta.position;
                restored.push(layer);
            }
        }
        restored.append(&mut current);
        document.layers = restored;
        document.active_layer = self.active;
        document.touch();
    }
}

/// 图层非像素状态的变更：移动 / 重命名 / 可见性 / 不透明度 / 排序。
#[derive(Debug)]
pub struct LayerMetaCommand {
    before: LayerStackMeta,
    after: LayerStackMeta,
    label: &'static str,
}

impl LayerMetaCommand {
    /// 改动前的快照（随后做改动，再 `new(before, after)`）。
    pub fn capture(document: &Document) -> LayerStackMeta {
        LayerStackMeta::capture(document)
    }

    pub fn new(before: LayerStackMeta, after: LayerStackMeta, label: &'static str) -> Self {
        Self {
            before,
            after,
            label,
        }
    }
}

impl Command for LayerMetaCommand {
    fn execute(&mut self, document: &mut Document) {
        self.after.restore(document);
    }

    fn undo(&mut self, document: &mut Document) {
        self.before.restore(document);
    }

    fn label(&self) -> &'static str {
        self.label
    }
}

/// 移动一个图层（只改 `position`）。
#[derive(Debug)]
pub struct SetLayerPositionCommand {
    layer: LayerId,
    before: Point,
    after: Point,
}

impl SetLayerPositionCommand {
    pub fn new(layer: LayerId, before: Point, after: Point) -> Self {
        Self {
            layer,
            before,
            after,
        }
    }
}

impl Command for SetLayerPositionCommand {
    fn execute(&mut self, document: &mut Document) {
        document.set_layer_position(self.layer, self.after);
    }

    fn undo(&mut self, document: &mut Document) {
        document.set_layer_position(self.layer, self.before);
    }

    fn label(&self) -> &'static str {
        "移动图层"
    }
}

/// 新建一个图层（带像素）；undo 时移除、redo 时插回。
#[derive(Debug)]
pub struct AddLayerCommand {
    id: LayerId,
    layer: Option<Layer>,
    index: usize,
    before_active: Option<LayerId>,
}

impl AddLayerCommand {
    pub fn new(layer: Layer, index: usize, before_active: Option<LayerId>) -> Self {
        Self {
            id: layer.id,
            layer: Some(layer),
            index,
            before_active,
        }
    }
}

impl Command for AddLayerCommand {
    fn execute(&mut self, document: &mut Document) {
        if let Some(layer) = self.layer.take() {
            let index = self.index.min(document.layers.len());
            document.layers.insert(index, layer);
            document.active_layer = Some(self.id);
            document.touch();
        }
    }

    fn undo(&mut self, document: &mut Document) {
        if let Some(index) = document.layers.iter().position(|layer| layer.id == self.id) {
            self.layer = Some(document.layers.remove(index));
            document.active_layer = self.before_active;
            document.touch();
        }
    }

    fn label(&self) -> &'static str {
        "新建图层"
    }
}

/// 删除一个图层；undo 时插回原位置与原来的当前图层。
#[derive(Debug)]
pub struct RemoveLayerCommand {
    id: LayerId,
    layer: Option<Layer>,
    index: usize,
    before_active: Option<LayerId>,
}

impl RemoveLayerCommand {
    pub fn new(id: LayerId, before_active: Option<LayerId>) -> Self {
        Self {
            id,
            layer: None,
            index: 0,
            before_active,
        }
    }
}

impl Command for RemoveLayerCommand {
    fn execute(&mut self, document: &mut Document) {
        if let Some(index) = document.layer_index(self.id) {
            self.index = index;
            self.layer = document.remove_layer(self.id);
        }
    }

    fn undo(&mut self, document: &mut Document) {
        if let Some(layer) = self.layer.take() {
            let index = self.index.min(document.layers.len());
            document.layers.insert(index, layer);
            document.active_layer = self.before_active;
            document.touch();
        }
    }

    fn label(&self) -> &'static str {
        "删除图层"
    }
}

/// 把图层裁到文档大小；undo 时恢复裁剪前的像素与位置。
#[derive(Debug)]
pub struct CropLayerCommand {
    id: LayerId,
    before: PixelBuffer,
    position: Point,
}

impl CropLayerCommand {
    pub fn new(id: LayerId, before: PixelBuffer, position: Point) -> Self {
        Self {
            id,
            before,
            position,
        }
    }
}

impl Command for CropLayerCommand {
    fn execute(&mut self, document: &mut Document) {
        document.crop_layer_to_document(self.id);
    }

    fn undo(&mut self, document: &mut Document) {
        if let Some(layer) = document.layer_mut(self.id) {
            layer.pixels = self.before.clone();
            layer.position = self.position;
            document.touch();
        }
    }

    fn label(&self) -> &'static str {
        "裁到文档"
    }
}
