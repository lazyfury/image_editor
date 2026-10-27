//! 图层命令：增删（含子树）、顺序 / 换父、元数据、编组、蒙版、移动、裁到文档。
//!
//! 元数据类命令只存非像素状态（顺序 + 每层 name / parent / visible / opacity /
//! blend / position / 蒙版启用 + 当前节点），所以移动 / 重命名 / 可见性这些撤销
//! 代价很小。蒙版的**像素**和图层像素一样走 [`PaintCommand`](super::PaintCommand)。

use super::super::document::Document;
use super::super::id::LayerId;
use super::super::layer::{BlendMode, Layer, LayerMask};
use super::super::pixel_buffer::PixelBuffer;
use super::super::point::Point;
use super::Command;

#[derive(Debug, Clone, PartialEq)]
struct LayerMeta {
    id: LayerId,
    name: String,
    parent: Option<LayerId>,
    visible: bool,
    opacity: f32,
    blend_mode: BlendMode,
    position: Point,
    mask_enabled: Option<bool>,
}

/// 图层栈的非像素状态：结构（parent）+ 每层元数据 + 当前节点。
///
/// 只存元数据，**不存像素**，所以移动 / 重命名 / 可见性 / 不透明度 / 排序
/// 这些操作的撤销代价极小。`expanded` / 蒙版像素 / 节点类型不在快照里。
#[derive(Debug, Clone, PartialEq)]
pub struct LayerStackMeta {
    layers: Vec<LayerMeta>,
    active: Option<LayerId>,
    active_mask: bool,
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
                    parent: layer.parent,
                    visible: layer.visible,
                    opacity: layer.opacity,
                    blend_mode: layer.blend_mode,
                    position: layer.position,
                    mask_enabled: layer.mask.as_ref().map(|mask| mask.enabled),
                })
                .collect(),
            active: document.active_layer,
            active_mask: document.active_mask,
        }
    }

    /// 把元数据 / 顺序 / 当前节点写回；像素不动（按 id 复用原 `Layer`）。
    fn restore(&self, document: &mut Document) {
        let mut current = std::mem::take(&mut document.layers);
        let mut restored = Vec::with_capacity(current.len());
        for meta in &self.layers {
            if let Some(index) = current.iter().position(|layer| layer.id == meta.id) {
                let mut layer = current.remove(index);
                layer.name.clone_from(&meta.name);
                layer.parent = meta.parent;
                layer.visible = meta.visible;
                layer.opacity = meta.opacity;
                layer.blend_mode = meta.blend_mode;
                layer.position = meta.position;
                if let (Some(enabled), Some(mask)) = (meta.mask_enabled, layer.mask.as_mut()) {
                    mask.enabled = enabled;
                }
                restored.push(layer);
            }
        }
        restored.append(&mut current);
        document.layers = restored;
        document.active_layer = self.active;
        document.active_mask = self.active_mask;
        document.touch();
    }
}

/// 图层非像素状态的变更：移动 / 重命名 / 可见性 / 不透明度 / 排序 / 蒙版开关。
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

/// 插入一棵（已经按前序 DFS 排好的）子树；undo 时整棵移除、redo 时插回。
#[derive(Debug)]
pub struct AddLayerCommand {
    id: LayerId,
    nodes: Option<Vec<Layer>>,
    parent: Option<LayerId>,
    index: usize,
    before_active: Option<LayerId>,
    label: &'static str,
}

impl AddLayerCommand {
    /// 在 `parent` 的第 `index` 个子位插入一个图层。
    pub fn new(
        layer: Layer,
        parent: Option<LayerId>,
        index: usize,
        before_active: Option<LayerId>,
    ) -> Self {
        Self::nodes(vec![layer], parent, index, before_active, "新建图层")
    }

    /// 插入一棵子树（复制组 / 粘贴多层）。
    pub fn nodes(
        nodes: Vec<Layer>,
        parent: Option<LayerId>,
        index: usize,
        before_active: Option<LayerId>,
        label: &'static str,
    ) -> Self {
        Self {
            id: nodes[0].id,
            nodes: Some(nodes),
            parent,
            index,
            before_active,
            label,
        }
    }
}

impl Command for AddLayerCommand {
    fn execute(&mut self, document: &mut Document) {
        let Some(nodes) = self.nodes.take() else {
            return;
        };
        if document.insert_subtree(nodes.clone(), self.parent, self.index) {
            document.active_layer = Some(self.id);
            document.active_mask = false;
        } else {
            self.nodes = Some(nodes);
        }
    }

    fn undo(&mut self, document: &mut Document) {
        if let Some((nodes, _, _)) = document.remove_subtree(self.id) {
            self.nodes = Some(nodes);
            document.active_layer = self.before_active;
            document.active_mask = false;
        }
    }

    fn label(&self) -> &'static str {
        self.label
    }
}

/// 删除一个节点（组会带走整棵子树）；undo 时整棵插回原位置。
#[derive(Debug)]
pub struct RemoveLayerCommand {
    id: LayerId,
    nodes: Option<Vec<Layer>>,
    parent: Option<LayerId>,
    index: usize,
    before_active: Option<LayerId>,
    label: &'static str,
}

impl RemoveLayerCommand {
    pub fn new(id: LayerId, before_active: Option<LayerId>) -> Self {
        Self {
            id,
            nodes: None,
            parent: None,
            index: 0,
            before_active,
            label: "删除图层",
        }
    }

    /// 显式指定标签（删组时用「删除组」）。
    pub fn labeled(mut self, label: &'static str) -> Self {
        self.label = label;
        self
    }
}

impl Command for RemoveLayerCommand {
    fn execute(&mut self, document: &mut Document) {
        if let Some((nodes, parent, index)) = document.remove_node(self.id) {
            self.nodes = Some(nodes);
            self.parent = parent;
            self.index = index;
        }
    }

    fn undo(&mut self, document: &mut Document) {
        if let Some(nodes) = self.nodes.take() {
            if document.insert_subtree(nodes, self.parent, self.index) {
                document.active_layer = self.before_active;
                document.active_mask = false;
            }
        }
    }

    fn label(&self) -> &'static str {
        self.label
    }
}

/// 拖拽 / 上移下移 / 换父的目标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveTarget {
    /// 放到某个兄弟的下面（同一父级）。
    Before(LayerId),
    /// 放到某个兄弟的上面（同一父级）。
    After(LayerId),
    /// 放进组的顶部。
    Into(LayerId),
    /// 直接给 `(parent, 搬完之后的 index)`。
    Position(Option<LayerId>, usize),
}

/// 拖拽 / 上移下移 / 换父：记录搬动前后的位置。
#[derive(Debug)]
pub struct MoveLayerCommand {
    id: LayerId,
    before: (Option<LayerId>, usize),
    target: MoveTarget,
    before_active: Option<LayerId>,
}

impl MoveLayerCommand {
    pub fn new(document: &Document, id: LayerId, target: MoveTarget) -> Self {
        let before = document
            .layer(id)
            .map(|layer| {
                let siblings = document.children(layer.parent);
                let index = siblings
                    .iter()
                    .position(|child| *child == id)
                    .unwrap_or_default();
                (layer.parent, index)
            })
            .unwrap_or((None, 0));
        Self {
            id,
            before,
            target,
            before_active: document.active_layer,
        }
    }

    fn apply(&self, document: &mut Document, target: MoveTarget) -> bool {
        match target {
            MoveTarget::Before(sibling) => document.move_before(self.id, sibling),
            MoveTarget::After(sibling) => document.move_after(self.id, sibling),
            MoveTarget::Into(group) => document.move_into(self.id, group),
            MoveTarget::Position(parent, index) => document.move_node(self.id, parent, index),
        }
    }
}

impl Command for MoveLayerCommand {
    fn execute(&mut self, document: &mut Document) {
        if self.apply(document, self.target) {
            document.active_layer = Some(self.id);
            document.active_mask = false;
        }
    }

    fn undo(&mut self, document: &mut Document) {
        if document.move_node(self.id, self.before.0, self.before.1) {
            document.active_layer = self.before_active;
            document.active_mask = false;
        }
    }

    fn label(&self) -> &'static str {
        "移动图层"
    }
}

/// 把当前节点裹进一个新组（`Cmd+G`）。
#[derive(Debug)]
pub struct GroupCommand {
    group: Option<Layer>,
    group_id: LayerId,
    member: LayerId,
    member_before: Option<(Option<LayerId>, usize)>,
    before_active: Option<LayerId>,
}

impl GroupCommand {
    pub fn new(document: &Document, member: LayerId, name: impl Into<String>) -> Self {
        let group = Layer::group(name);
        let member_before = document.layer(member).map(|layer| {
            let siblings = document.children(layer.parent);
            let index = siblings
                .iter()
                .position(|child| *child == member)
                .unwrap_or_default();
            (layer.parent, index)
        });
        Self {
            group_id: group.id,
            group: Some(group),
            member,
            member_before,
            before_active: document.active_layer,
        }
    }
}

impl Command for GroupCommand {
    fn execute(&mut self, document: &mut Document) {
        let Some(group) = self.group.take() else {
            return;
        };
        let Some((parent, index)) = self.member_before else {
            return;
        };
        // 组占据成员原来的位置，成员成为组里唯一的子节点。
        if document.insert_subtree(vec![group], parent, index) {
            document.move_into(self.member, self.group_id);
            document.active_layer = Some(self.group_id);
            document.active_mask = false;
        }
    }

    fn undo(&mut self, document: &mut Document) {
        // 先把成员搬回原位，再删掉空组。
        if let Some((parent, index)) = self.member_before {
            document.move_node(self.member, parent, index);
        }
        if let Some((mut nodes, _, _)) = document.remove_subtree(self.group_id) {
            if let Some(group) = nodes.pop() {
                self.group = Some(group);
            }
        }
        document.active_layer = self.before_active;
        document.active_mask = false;
    }

    fn label(&self) -> &'static str {
        "编组"
    }
}

/// 取消编组：把子节点按原顺序放到组原来的位置。
#[derive(Debug)]
pub struct UngroupCommand {
    group_id: LayerId,
    group: Option<Layer>,
    members: Vec<Layer>,
    parent: Option<LayerId>,
    index: usize,
    before_active: Option<LayerId>,
}

impl UngroupCommand {
    pub fn new(document: &Document, group: LayerId) -> Option<Self> {
        let layer = document.layer(group)?;
        if !layer.is_group() {
            return None;
        }
        Some(Self {
            group_id: group,
            group: None,
            members: Vec::new(),
            parent: layer.parent,
            index: 0,
            before_active: document.active_layer,
        })
    }
}

impl Command for UngroupCommand {
    fn execute(&mut self, document: &mut Document) {
        let Some((mut nodes, parent, index)) = document.remove_subtree(self.group_id) else {
            return;
        };
        if nodes.is_empty() {
            return;
        }
        let group = nodes.remove(0);
        let mut members = nodes;
        // 子节点的 parent 重新挂到组的父级，插回组原来的位置（顺序不变）。
        for member in &mut members {
            member.parent = parent;
        }
        document.insert_subtree(members.clone(), parent, index);
        document.active_layer = members.last().map(|layer| layer.id).or(self.before_active);
        document.active_mask = false;
        self.group = Some(group);
        self.members = members;
        self.parent = parent;
        self.index = index;
    }

    fn undo(&mut self, document: &mut Document) {
        let Some(group) = self.group.take() else {
            return;
        };
        for member in &self.members {
            document.remove_layer(member.id);
        }
        document.insert_subtree(vec![group], self.parent, self.index);
        document.active_layer = self.before_active;
        document.active_mask = false;
    }

    fn label(&self) -> &'static str {
        "取消编组"
    }
}

/// 蒙版结构变更：挂上 / 替换 / 删除 / 反相 / 启用。像素整张存（蒙版比图层小）。
#[derive(Debug)]
pub struct MaskCommand {
    layer: LayerId,
    before: Option<LayerMask>,
    after: Option<LayerMask>,
    label: &'static str,
}

impl MaskCommand {
    pub fn new(
        layer: LayerId,
        before: Option<LayerMask>,
        after: Option<LayerMask>,
        label: &'static str,
    ) -> Self {
        Self {
            layer,
            before,
            after,
            label,
        }
    }

    /// 写回 `mask`（`None` = 删掉）。
    fn write(document: &mut Document, layer: LayerId, mask: Option<&LayerMask>) -> bool {
        match mask {
            Some(mask) => document.set_layer_mask(layer, mask.clone()),
            None => document.remove_layer_mask(layer).is_some(),
        }
    }
}

impl Command for MaskCommand {
    fn execute(&mut self, document: &mut Document) {
        Self::write(document, self.layer, self.after.as_ref());
    }

    fn undo(&mut self, document: &mut Document) {
        Self::write(document, self.layer, self.before.as_ref());
    }

    fn label(&self) -> &'static str {
        self.label
    }
}

/// 把图层（连同蒙版）裁到文档大小；undo 时恢复裁剪前的像素与位置。
#[derive(Debug)]
pub struct CropLayerCommand {
    id: LayerId,
    before: PixelBuffer,
    before_mask: Option<PixelBuffer>,
    position: Point,
}

impl CropLayerCommand {
    pub fn new(
        id: LayerId,
        before: PixelBuffer,
        before_mask: Option<PixelBuffer>,
        position: Point,
    ) -> Self {
        Self {
            id,
            before,
            before_mask,
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
            if let (Some(mask), Some(pixels)) = (layer.mask.as_mut(), self.before_mask.clone()) {
                mask.pixels = pixels;
            }
            document.touch();
        }
    }

    fn label(&self) -> &'static str {
        "裁到文档"
    }
}
