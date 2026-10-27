//! 文档：尺寸、图层树和当前选中节点。
//!
//! `layers` 是一个**前序 DFS、从下到上**的扁平数组：
//!
//! ```text
//! [ 组A, 子1, 子2, 组B, 子3, 根C ]
//! ```
//!
//! 每个节点的子树在数组里是**连续区间**，`parent` 指向所属组（根为 `None`）。
//! 不变量见 [`Document::validate`]（测试与 debug 断言用）。这一层选择扁平原
//! 因：面板 / 合成 / 文件格式本来就按顺序遍历，而现有大量代码用
//! `layers[i]` / `layer_index(id)`，扁平数组能保住这些语义。
//!
//! 撤销历史（Phase 6）**不**在这里：它是编辑会话状态，存在
//! [`crate::app::state::AppState`]；命令定义在 [`super::history`]。

use std::ops::Range;

use super::color::Color;
use super::id::{DocumentId, LayerId};
use super::layer::{clamp_opacity, Layer, LayerKind, LayerMask};
use super::pixel_buffer::PixelBuffer;
use super::point::Point;

/// 新建文档的默认尺寸（§29 Phase 2 的验收标准；像素图测试用的小画布）。
pub const DEFAULT_WIDTH: u32 = 128;
pub const DEFAULT_HEIGHT: u32 = 128;

/// 当前编辑的像素目标（图层缓冲或它的蒙版）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PixelTarget {
    Layer,
    Mask,
}

/// 面板里的一行：`Document::rows()` 的输出，图层面板 / 自检 / 拖拽共用。
#[derive(Debug, Clone, PartialEq)]
pub struct LayerRow {
    pub id: LayerId,
    pub depth: u16,
    pub is_group: bool,
    pub expanded: bool,
    pub has_mask: bool,
    pub mask_enabled: bool,
    pub visible: bool,
    pub name: String,
    pub opacity: f32,
}

/// 一次可以落笔的像素目标：当前图层或它的蒙版。
pub struct PaintSurface<'a> {
    pub id: LayerId,
    pub target: PixelTarget,
    /// 蒙版或图层缓冲；坐标系与 `position` 一致。
    pub pixels: &'a mut PixelBuffer,
    pub position: Point,
}

/// 一个图像文档。
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub id: DocumentId,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// 前序 DFS、从下到上；每个节点的子树是连续区间。
    pub layers: Vec<Layer>,
    /// 当前节点（可能是组）。
    pub active_layer: Option<LayerId>,
    /// 当前编辑目标是不是蒙版（只在当前节点有蒙版时有效）。
    pub active_mask: bool,
    pub background: Color,
    pub dirty: bool,
    /// 每次会改动文档的操作 +1；视图用它判断要不要重合成 / 刷新图层列表。
    revision: u64,
}

#[allow(dead_code)] // 一部分查询 / 构造只被测试与后续阶段用到（沿用原模块的取舍）。
impl Document {
    /// 一个默认尺寸（带白色背景图层）的文档。
    pub fn new(name: impl Into<String>, width: u32, height: u32) -> Self {
        Self::with_background(name, width, height, Color::WHITE)
    }

    /// 带指定背景色、且背景图层已填充该色的文档。
    pub fn with_background(
        name: impl Into<String>,
        width: u32,
        height: u32,
        background: Color,
    ) -> Self {
        let layer = Layer::new("背景", PixelBuffer::filled(width, height, background));
        let id = layer.id;
        Self {
            id: DocumentId::next(),
            name: name.into(),
            width,
            height,
            layers: vec![layer],
            active_layer: Some(id),
            active_mask: false,
            background,
            dirty: false,
            revision: 0,
        }
    }

    /// 没有图层的空文档（测试和"新建透明文档"用）。
    pub fn empty(name: impl Into<String>, width: u32, height: u32, background: Color) -> Self {
        Self {
            id: DocumentId::next(),
            name: name.into(),
            width,
            height,
            layers: Vec::new(),
            active_layer: None,
            active_mask: false,
            background,
            dirty: false,
            revision: 0,
        }
    }

    /// 变更计数（从 0 开始，每次改动 +1）。
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// 标记文档已改动：置脏并让变更计数前进。命令（[`super::history`]）在
    /// 写回像素后也调它，视图据此重合成。
    pub(crate) fn touch(&mut self) {
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
    }

    // -- 查询 ------------------------------------------------------------

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|layer| layer.id == id)
    }

    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|layer| layer.id == id)
    }

    /// 图层在栈里的下标（`0` = 最底层 / 最左侧）。
    pub fn layer_index(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|layer| layer.id == id)
    }

    pub fn active_layer(&self) -> Option<&Layer> {
        self.active_layer.and_then(|id| self.layer(id))
    }

    pub fn active_layer_mut(&mut self) -> Option<&mut Layer> {
        let id = self.active_layer?;
        self.layer_mut(id)
    }

    /// 当前节点是不是组。
    pub fn active_is_group(&self) -> bool {
        self.active_layer().is_some_and(|layer| layer.is_group())
    }

    /// 指定目标（图层 / 蒙版）的像素缓冲，只读。
    pub fn surface_pixels(&self, id: LayerId, target: PixelTarget) -> Option<&PixelBuffer> {
        let layer = self.layer(id)?;
        match target {
            PixelTarget::Layer => Some(&layer.pixels),
            PixelTarget::Mask => layer.mask.as_ref().map(|mask| &mask.pixels),
        }
    }

    /// 当前可落笔的目标：组没有像素、以及没有蒙版却选了蒙版时返回 `None`。
    pub fn active_surface_mut(&mut self) -> Option<PaintSurface<'_>> {
        let id = self.active_layer?;
        let active_mask = self.active_mask;
        let index = self.layer_index(id)?;
        let layer = &mut self.layers[index];
        if layer.is_group() {
            return None;
        }
        let position = layer.position;
        if active_mask {
            let mask = layer.mask.as_mut()?;
            Some(PaintSurface {
                id,
                target: PixelTarget::Mask,
                pixels: &mut mask.pixels,
                position,
            })
        } else {
            Some(PaintSurface {
                id,
                target: PixelTarget::Layer,
                pixels: &mut layer.pixels,
                position,
            })
        }
    }

    /// 直接子节点（从下到上）。O(n) 扫描，调用点在面板 / 命令里，足够。
    pub fn children(&self, parent: Option<LayerId>) -> Vec<LayerId> {
        self.layers
            .iter()
            .filter(|layer| layer.parent == parent)
            .map(|layer| layer.id)
            .collect()
    }

    /// 节点的子树在 `layers` 里的连续区间（前序 DFS）。
    pub fn subtree_range(&self, id: LayerId) -> Option<Range<usize>> {
        let start = self.layer_index(id)?;
        let mut end = start + 1;
        while end < self.layers.len() && self.is_descendant_index(end, start) {
            end += 1;
        }
        Some(start..end)
    }

    fn is_descendant_index(&self, mut index: usize, ancestor: usize) -> bool {
        let ancestor_id = self.layers[ancestor].id;
        loop {
            let Some(parent) = self.layers[index].parent else {
                return false;
            };
            if parent == ancestor_id {
                return true;
            }
            match self.layer_index(parent) {
                Some(parent_index) => index = parent_index,
                None => return false,
            }
        }
    }

    /// `id` 是不是 `ancestor` 的祖先（含自己）。
    pub fn is_in_subtree(&self, id: LayerId, ancestor: LayerId) -> bool {
        let Some(mut index) = self.layer_index(id) else {
            return false;
        };
        loop {
            if self.layers[index].id == ancestor {
                return true;
            }
            let Some(parent) = self.layers[index].parent else {
                return false;
            };
            match self.layer_index(parent) {
                Some(parent_index) => index = parent_index,
                None => return false,
            }
        }
    }

    /// 节点本身可见且所有祖先都可见。
    pub fn effective_visible(&self, id: LayerId) -> bool {
        let Some(mut index) = self.layer_index(id) else {
            return false;
        };
        loop {
            if !self.layers[index].visible {
                return false;
            }
            let Some(parent) = self.layers[index].parent else {
                return true;
            };
            match self.layer_index(parent) {
                Some(parent_index) => index = parent_index,
                None => return false,
            }
        }
    }

    /// 面板行：从上到下、考虑折叠；`depth` 是缩进层级。
    pub fn rows(&self) -> Vec<LayerRow> {
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); self.layers.len()];
        let mut roots = Vec::new();
        for (index, layer) in self.layers.iter().enumerate() {
            match layer.parent.and_then(|parent| self.layer_index(parent)) {
                Some(parent_index) => children[parent_index].push(index),
                None => roots.push(index),
            }
        }
        let mut rows = Vec::with_capacity(self.layers.len());
        // 显示顺序是最上面一行在最前，而数组是「从下到上」。
        for index in roots.into_iter().rev() {
            self.push_rows(index, 0, &children, &mut rows);
        }
        rows
    }

    fn push_rows(
        &self,
        index: usize,
        depth: u16,
        children: &[Vec<usize>],
        rows: &mut Vec<LayerRow>,
    ) {
        let layer = &self.layers[index];
        rows.push(LayerRow {
            id: layer.id,
            depth,
            is_group: layer.is_group(),
            expanded: layer.expanded,
            has_mask: layer.mask.is_some(),
            mask_enabled: layer.mask.as_ref().is_some_and(|mask| mask.enabled),
            visible: layer.visible,
            name: layer.name.clone(),
            opacity: layer.opacity,
        });
        if layer.expanded {
            for child in children[index].iter().rev() {
                self.push_rows(*child, depth + 1, children, rows);
            }
        }
    }

    /// 调试 / 测试用的树不变量：子树连续、parent 指向组、没有环。
    #[doc(hidden)]
    pub fn validate(&self) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for (index, layer) in self.layers.iter().enumerate() {
            if !seen.insert(layer.id) {
                return Err(format!("重复的节点 id：{}", layer.id));
            }
            if let Some(parent) = layer.parent {
                let parent_layer = self
                    .layer(parent)
                    .ok_or_else(|| format!("{} 的 parent 不存在", layer.id))?;
                if !parent_layer.is_group() {
                    return Err(format!("{} 的 parent 不是组", layer.id));
                }
                let parent_index = self.layer_index(parent).expect("parent exists");
                if parent_index >= index {
                    return Err(format!("{} 的 parent 不在它前面（pre-order）", layer.id));
                }
            }
            let Some(range) = self.subtree_range(layer.id) else {
                return Err(format!("{} 不在数组里", layer.id));
            };
            if range.start != index {
                return Err(format!("{} 的子树起点错了", layer.id));
            }
        }
        Ok(())
    }

    // -- 变更 ------------------------------------------------------------

    /// 在最上面加一个全透明图层，并设为当前图层。
    pub fn add_layer(&mut self, name: impl Into<String>) -> LayerId {
        self.add_layer_with_pixels(name, PixelBuffer::new(self.width, self.height))
    }

    /// 在当前节点上方加一个带指定像素的图层，并设为当前图层。
    ///
    /// 「上方」的规则：当前是组 → 放进组的顶部；否则插到同一父级的当前节点
    /// 之后。像素尺寸不要求等于文档（导入的图片可以更大/更小，合成时按
    /// `position` 裁剪）。
    pub fn add_layer_with_pixels(
        &mut self,
        name: impl Into<String>,
        pixels: PixelBuffer,
    ) -> LayerId {
        let layer = Layer::new(name, pixels);
        let id = layer.id;
        let (parent, index) = self.insert_slot_above_active();
        self.insert_subtree(vec![layer], parent, index);
        self.active_layer = Some(id);
        self.active_mask = false;
        id
    }

    /// 在当前节点上方加一个空组，并设为当前图层。
    pub fn add_group(&mut self, name: impl Into<String>) -> LayerId {
        let group = Layer::group(name);
        let id = group.id;
        let (parent, index) = self.insert_slot_above_active();
        self.insert_subtree(vec![group], parent, index);
        self.active_layer = Some(id);
        self.active_mask = false;
        id
    }

    /// 「新节点放在哪里」：组的顶部，或同一父级里当前节点之后。
    pub fn insertion_slot(&self) -> (Option<LayerId>, usize) {
        self.insert_slot_above_active()
    }

    /// 「新节点放在哪里」：组的顶部，或同一父级里当前节点之后。
    fn insert_slot_above_active(&self) -> (Option<LayerId>, usize) {
        let Some(active) = self.active_layer.and_then(|id| self.layer(id)) else {
            return (None, self.children(None).len());
        };
        if active.is_group() {
            return (Some(active.id), self.children(Some(active.id)).len());
        }
        let siblings = self.children(active.parent);
        let position = siblings
            .iter()
            .position(|id| *id == active.id)
            .map_or(siblings.len(), |index| index + 1);
        (active.parent, position)
    }

    /// 删除一个节点（组会带走整棵子树），返回被删掉的节点本身。
    pub fn remove_layer(&mut self, id: LayerId) -> Option<Layer> {
        let (nodes, _, _) = self.remove_node(id)?;
        nodes.into_iter().next()
    }

    /// 删除节点及其子树，同时修正「当前节点」。
    ///
    /// 删掉的是当前节点（或当前节点在被删的子树里）时，选中同父级的下一个兄弟，
    /// 没有就退回上一个 / 父组。
    pub fn remove_node(&mut self, id: LayerId) -> Option<(Vec<Layer>, Option<LayerId>, usize)> {
        let (nodes, parent, index) = self.remove_subtree(id)?;
        let lost = self
            .active_layer
            .is_some_and(|active| self.layer(active).is_none());
        if lost {
            let siblings = self.children(parent);
            self.active_layer = siblings
                .get(index)
                .or_else(|| index.checked_sub(1).and_then(|prev| siblings.get(prev)))
                .copied()
                .or(parent);
            self.active_mask = false;
        }
        Some((nodes, parent, index))
    }

    pub fn rename_layer(&mut self, id: LayerId, name: impl Into<String>) -> bool {
        let Some(index) = self.layer_index(id) else {
            return false;
        };
        self.layers[index].name = name.into();
        self.touch();
        true
    }

    pub fn set_layer_visible(&mut self, id: LayerId, visible: bool) -> bool {
        let Some(index) = self.layer_index(id) else {
            return false;
        };
        if self.layers[index].visible == visible {
            return false;
        }
        self.layers[index].visible = visible;
        self.touch();
        true
    }

    /// 设置不透明度，钳到 `0.0..=1.0`。
    pub fn set_layer_opacity(&mut self, id: LayerId, opacity: f32) -> bool {
        let Some(index) = self.layer_index(id) else {
            return false;
        };
        self.layers[index].opacity = clamp_opacity(opacity);
        self.touch();
        true
    }

    /// 设置图层相对文档原点的像素偏移（Phase 8 的移动工具）。组没有位置。
    /// 位置没变时返回 `false`，不前进 `revision`。
    pub fn set_layer_position(&mut self, id: LayerId, position: Point) -> bool {
        let Some(index) = self.layer_index(id) else {
            return false;
        };
        if self.layers[index].is_group() || self.layers[index].position == position {
            return false;
        }
        self.layers[index].position = position;
        self.touch();
        true
    }

    /// 组展开 / 折叠（UI 状态，不当作编辑内容）。
    pub fn set_expanded(&mut self, id: LayerId, expanded: bool) -> bool {
        let Some(index) = self.layer_index(id) else {
            return false;
        };
        if self.layers[index].expanded == expanded {
            return false;
        }
        self.layers[index].expanded = expanded;
        true
    }

    /// 选中一个存在的节点（组也可以选中），并复位「编辑蒙版」。
    pub fn select_layer(&mut self, id: LayerId) -> bool {
        if self.layer(id).is_some() {
            self.active_layer = Some(id);
            self.active_mask = false;
            true
        } else {
            false
        }
    }

    /// 切换「编辑当前节点的蒙版」；没有蒙版 / 组没有蒙版时返回 `false`。
    pub fn set_active_mask(&mut self, active: bool) -> bool {
        if !active {
            self.active_mask = false;
            return true;
        }
        let Some(layer) = self.active_layer() else {
            return false;
        };
        if layer.is_group() || layer.mask.is_none() {
            return false;
        }
        self.active_mask = true;
        true
    }

    // -- 蒙版 ------------------------------------------------------------

    /// 挂上 / 替换蒙版。
    pub fn set_layer_mask(&mut self, id: LayerId, mask: LayerMask) -> bool {
        let Some(index) = self.layer_index(id) else {
            return false;
        };
        self.layers[index].mask = Some(mask);
        self.touch();
        true
    }

    /// 摘掉蒙版并返回它。
    pub fn remove_layer_mask(&mut self, id: LayerId) -> Option<LayerMask> {
        let index = self.layer_index(id)?;
        let mask = self.layers[index].mask.take()?;
        if self.active_layer == Some(id) {
            self.active_mask = false;
        }
        self.touch();
        Some(mask)
    }

    /// 启用 / 停用蒙版（不删像素）。
    pub fn set_mask_enabled(&mut self, id: LayerId, enabled: bool) -> bool {
        let Some(mask) = self.layer_mut(id).and_then(|layer| layer.mask.as_mut()) else {
            return false;
        };
        if mask.enabled == enabled {
            return false;
        }
        mask.enabled = enabled;
        self.touch();
        true
    }

    /// 蒙版反相（`coverage -> 255 - coverage`）。
    pub fn invert_layer_mask(&mut self, id: LayerId) -> bool {
        let Some(mask) = self.layer_mut(id).and_then(|layer| layer.mask.as_mut()) else {
            return false;
        };
        for pixel in mask.pixels.data.chunks_exact_mut(4) {
            let value = 255 - pixel[0];
            pixel[0] = value;
            pixel[1] = value;
            pixel[2] = value;
            pixel[3] = 255;
        }
        self.touch();
        true
    }

    // -- 树操作 ----------------------------------------------------------

    /// 把一棵**已经按前序 DFS 排好**的子树插到 `parent` 的第 `index` 个子位。
    ///
    /// `index` 是「子节点序号」（0 = 最下面）；超出范围夹到末尾。
    pub fn insert_subtree(
        &mut self,
        mut nodes: Vec<Layer>,
        parent: Option<LayerId>,
        index: usize,
    ) -> bool {
        if nodes.is_empty() {
            return false;
        }
        if let Some(parent) = parent {
            match self.layer(parent) {
                Some(layer) if layer.is_group() => {}
                _ => return false,
            }
        }
        let flat = self.child_flat_index(parent, index);
        nodes[0].parent = parent;
        self.layers.splice(flat..flat, nodes);
        self.touch();
        debug_assert!(self.validate().is_ok(), "{:?}", self.validate());
        true
    }

    /// 删除节点及其子树，返回（子树、父节点、在父级里的序号）。
    pub fn remove_subtree(&mut self, id: LayerId) -> Option<(Vec<Layer>, Option<LayerId>, usize)> {
        let range = self.subtree_range(id)?;
        let parent = self.layers[range.start].parent;
        let index = self
            .children(parent)
            .iter()
            .position(|child| *child == id)?;
        let removed: Vec<Layer> = self.layers.drain(range).collect();
        self.touch();
        Some((removed, parent, index))
    }

    /// 把节点（连同子树）搬到 `parent` 的第 `index` 个子位。
    ///
    /// `index` 是**搬完之后**在兄弟列表里的位置（0 = 最下面）；超出范围夹到末尾。
    pub fn move_node(&mut self, id: LayerId, parent: Option<LayerId>, index: usize) -> bool {
        if !self.can_move(id, parent, index) {
            return false;
        }
        let Some(range) = self.subtree_range(id) else {
            return false;
        };
        let nodes: Vec<Layer> = self.layers.drain(range).collect();
        self.insert_subtree(nodes, parent, index);
        true
    }

    /// 把 `id` 放到 `target` 的下面（同一父级，`id` 成为 `target` 的下一个）。
    pub fn move_before(&mut self, id: LayerId, target: LayerId) -> bool {
        let Some(target_layer) = self.layer(target) else {
            return false;
        };
        let parent = target_layer.parent;
        let siblings = self.children(parent);
        let Some(target_index) = siblings.iter().position(|child| *child == target) else {
            return false;
        };
        // 搬走自己会把它后面的兄弟整体前移一位。
        let shift = usize::from(
            self.layer(id).is_some_and(|layer| layer.parent == parent)
                && self.layer_index(id) < self.layer_index(target),
        );
        self.move_node(id, parent, target_index.saturating_sub(shift))
    }

    /// 把 `id` 放到 `target` 的上面（同一父级）。
    pub fn move_after(&mut self, id: LayerId, target: LayerId) -> bool {
        let Some(target_layer) = self.layer(target) else {
            return false;
        };
        let parent = target_layer.parent;
        let siblings = self.children(parent);
        let Some(target_index) = siblings.iter().position(|child| *child == target) else {
            return false;
        };
        let shift = usize::from(
            self.layer(id).is_some_and(|layer| layer.parent == parent)
                && self.layer_index(id) < self.layer_index(target),
        );
        self.move_node(id, parent, target_index + 1 - shift)
    }

    /// 把 `id` 放进组的**顶部**（组的最后一个子节点）。
    pub fn move_into(&mut self, id: LayerId, group: LayerId) -> bool {
        self.move_node(id, Some(group), usize::MAX)
    }

    /// 能不能把 `id` 搬到 `parent` 的第 `index` 个子位。
    pub fn can_move(&self, id: LayerId, parent: Option<LayerId>, _index: usize) -> bool {
        let Some(layer) = self.layer(id) else {
            return false;
        };
        match parent {
            None => true,
            Some(parent) => {
                !self.is_in_subtree(parent, id)
                    && self.layer(parent).is_some_and(|node| node.is_group())
                    && layer.id != parent
            }
        }
    }

    /// 把节点移到根层的 `new_index`（向下兼容旧 `move_layer` 的调用）。
    pub fn move_layer(&mut self, id: LayerId, new_index: usize) -> bool {
        self.move_node(id, None, new_index)
    }

    /// 同父级内上移 / 下移一层；到顶 / 到底时返回 `false`。
    pub fn nudge_layer(&mut self, id: LayerId, delta: i32) -> bool {
        let Some(layer) = self.layer(id) else {
            return false;
        };
        let siblings = self.children(layer.parent);
        let Some(index) = siblings.iter().position(|child| *child == id) else {
            return false;
        };
        let target = index as i64 + delta as i64;
        if target < 0 || target as usize >= siblings.len() {
            return false;
        }
        self.move_node(id, layer.parent, target as usize)
    }

    /// `parent` 的第 `index` 个子节点在 `layers` 里的扁平位置。
    fn child_flat_index(&self, parent: Option<LayerId>, index: usize) -> usize {
        let children = self.children(parent);
        match children.get(index) {
            Some(child) => self.layer_index(*child).unwrap_or(self.layers.len()),
            None => match parent {
                Some(parent) => self
                    .subtree_range(parent)
                    .map_or(self.layers.len(), |range| range.end),
                None => self.layers.len(),
            },
        }
    }

    // -- 像素缓冲 --------------------------------------------------------

    /// 把图层缓冲区（以及蒙版）扩到至少覆盖整张文档，返回内容在 **buffer
    /// 索引** 上的平移量 `(dx, dy)`（已覆盖时返回 `(0, 0)`）。
    ///
    /// 画笔落笔前调用。扩展是"当前缓冲区范围 ∪ 文档范围"的并集：
    ///
    /// - 文档范围内处处可落笔（图层移空、重新露出的区域也变回透明像素）；
    /// - 移出画布的像素**不裁掉**，留在缓冲区里，还能再移回来；
    /// - 内容在屏幕上的位置不变，只把 `position` 改成并集左上角。
    ///
    /// 补空间只会补在左侧 / 上方，所以内容索引是**非负**平移的；调用方（画笔）
    /// 需要用返回的 `(dx, dy)` 同步平移历史命令记录的区域。组没有像素，返回
    /// `(0, 0)`。
    pub fn ensure_layer_covers_document(&mut self, id: LayerId) -> (i32, i32) {
        let Some(index) = self.layer_index(id) else {
            return (0, 0);
        };
        let layer = &self.layers[index];
        if layer.is_group() {
            return (0, 0);
        }
        let old_x = layer.position.x as i64;
        let old_y = layer.position.y as i64;
        let left = old_x.min(0);
        let top = old_y.min(0);
        let right = (old_x + layer.pixels.width as i64).max(self.width as i64);
        let bottom = (old_y + layer.pixels.height as i64).max(self.height as i64);
        let width = (right - left) as u32;
        let height = (bottom - top) as u32;
        if left == old_x
            && top == old_y
            && width == layer.pixels.width
            && height == layer.pixels.height
        {
            return (0, 0);
        }
        let dx = (old_x - left) as i32;
        let dy = (old_y - top) as i32;
        let layer = &mut self.layers[index];
        layer.pixels = layer.pixels.placed(width, height, dx, dy);
        if let Some(mask) = layer.mask.as_mut() {
            mask.pixels = mask.pixels.placed(width, height, dx, dy);
        }
        layer.position = Point::new(left as i32, top as i32);
        self.touch();
        (dx, dy)
    }

    /// 把图层缓冲（连同蒙版）裁回文档大小：可见部分按 `position` 摆放，
    /// `position` 归零，画布外的像素丢弃。已经对齐或组时返回 `false`。
    pub fn crop_layer_to_document(&mut self, id: LayerId) -> bool {
        let Some(index) = self.layer_index(id) else {
            return false;
        };
        let layer = &self.layers[index];
        if layer.is_group()
            || (layer.position == Point::ZERO
                && layer.pixels.width == self.width
                && layer.pixels.height == self.height)
        {
            return false;
        }
        let x = layer.position.x;
        let y = layer.position.y;
        let layer = &mut self.layers[index];
        layer.pixels = layer.pixels.placed(self.width, self.height, x, y);
        if let Some(mask) = layer.mask.as_mut() {
            mask.pixels = mask.pixels.placed(self.width, self.height, x, y);
        }
        layer.position = Point::ZERO;
        self.touch();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_document_is_the_default_size_with_a_background_layer() {
        let document = Document::new("测试", DEFAULT_WIDTH, DEFAULT_HEIGHT);
        assert_eq!((document.width, document.height), (128, 128));
        assert_eq!(document.name, "测试");
        assert_eq!(document.layers.len(), 1);
        assert_eq!(document.background, Color::WHITE);
        assert!(!document.dirty);

        let background = document.active_layer().expect("一个背景图层");
        assert_eq!(background.name, "背景");
        assert!(background.visible);
        assert_eq!(background.pixels.get_pixel(0, 0), Color::WHITE);
    }

    #[test]
    fn add_layer_stacks_on_top_and_selects_it() {
        let mut document = Document::new("d", 4, 3);
        let background = document.layers[0].id;
        let added = document.add_layer("Layer 1");

        assert_eq!(document.layers.len(), 2);
        assert_eq!(document.layers.last().unwrap().id, added, "加在最上面");
        assert_eq!(document.active_layer, Some(added));
        assert!(document.dirty);
        // 新图层是全透明的，尺寸跟文档一致。
        let layer = document.layer(added).unwrap();
        assert_eq!(layer.pixels.get_pixel(0, 0), Color::TRANSPARENT);
        assert_eq!((layer.pixels.width, layer.pixels.height), (4, 3));
        assert_ne!(added, background);
    }

    #[test]
    fn add_layer_with_pixels_keeps_the_given_size_and_content() {
        let mut document = Document::new("d", 4, 3);
        let pixels = PixelBuffer::filled(2, 5, Color::RED);
        let id = document.add_layer_with_pixels("导入", pixels);

        let layer = document.layer(id).expect("imported layer");
        assert_eq!((layer.pixels.width, layer.pixels.height), (2, 5));
        assert_eq!(layer.pixels.get_pixel(0, 0), Color::RED);
        assert_eq!(document.active_layer, Some(id));
    }

    #[test]
    fn removing_the_active_layer_selects_a_neighbour() {
        let mut document = Document::new("d", 2, 2);
        let bottom = document.layers[0].id;
        let top = document.add_layer("top");
        assert_eq!(document.active_layer, Some(top));

        let removed = document.remove_layer(top).expect("top 存在");
        assert_eq!(removed.id, top);
        assert_eq!(document.active_layer, Some(bottom), "回落到下面的图层");
    }

    #[test]
    fn removing_the_last_layer_leaves_no_active_layer() {
        let mut document = Document::new("d", 2, 2);
        let only = document.layers[0].id;
        assert!(document.remove_layer(only).is_some());
        assert!(document.layers.is_empty());
        assert_eq!(document.active_layer, None);
        assert!(document.active_layer().is_none());
        // 删不存在的图层返回 None，不改状态。
        assert!(document.remove_layer(only).is_none());
    }

    #[test]
    fn rename_select_and_visibility_only_touch_existing_layers() {
        let mut document = Document::new("d", 2, 2);
        let bottom = document.layers[0].id;
        let top = document.add_layer("top");

        assert!(document.rename_layer(top, "上层"));
        assert_eq!(document.layer(top).unwrap().name, "上层");

        assert!(document.select_layer(bottom));
        assert_eq!(document.active_layer, Some(bottom));

        assert!(document.set_layer_visible(bottom, false));
        assert!(!document.layer(bottom).unwrap().visible);

        // 不存在的 id 一律 false，且不影响选中。
        let ghost = Layer::new("ghost", PixelBuffer::new(1, 1)).id;
        assert!(!document.rename_layer(ghost, "x"));
        assert!(!document.set_layer_visible(ghost, true));
        assert!(!document.set_layer_opacity(ghost, 0.5));
        assert!(!document.select_layer(ghost));
        assert_eq!(document.active_layer, Some(bottom));
    }

    #[test]
    fn expanding_a_layer_to_cover_the_document_keeps_off_canvas_pixels() {
        let mut document = Document::new("d", 4, 1);
        let id = document.layers[0].id;
        document.layers[0].pixels.set_pixel(0, 0, Color::RED);

        // 右移 2：内容索引右移 2 补左侧空间，但红色像素不丢。
        assert!(document.set_layer_position(id, Point::new(2, 0)));
        assert_eq!(document.ensure_layer_covers_document(id), (2, 0));
        let layer = document.layer(id).unwrap();
        assert_eq!(layer.position, Point::ZERO);
        assert_eq!(
            layer.pixels.get_pixel(2, 0),
            Color::RED,
            "移到画布外的内容还在"
        );
        assert_eq!(
            layer.pixels.get_pixel(0, 0),
            Color::TRANSPARENT,
            "空出的位置透明"
        );

        // 已覆盖文档 -> no-op。
        assert_eq!(document.ensure_layer_covers_document(id), (0, 0));

        // 左移 3：`position` 可以变负，内容仍留在缓冲区里。
        assert!(document.set_layer_position(id, Point::new(-3, 0)));
        let _ = document.ensure_layer_covers_document(id);
        let layer = document.layer(id).unwrap();
        assert_eq!(layer.position, Point::new(-3, 0));
        assert_eq!(layer.pixels.get_pixel(2, 0), Color::RED, "内容保留在缓冲区");
    }

    #[test]
    fn cropping_a_layer_to_the_document_drops_off_canvas_pixels() {
        let mut document = Document::new("d", 4, 2);
        let id = document.layers[0].id;
        document.layers[0].pixels.set_pixel(0, 0, Color::RED);
        // 右移 2 并扩到覆盖文档，缓冲会比文档大。
        assert!(document.set_layer_position(id, Point::new(2, 0)));
        document.ensure_layer_covers_document(id);
        assert!(document.layer(id).unwrap().pixels.width > 4);

        assert!(document.crop_layer_to_document(id));
        let layer = document.layer(id).unwrap();
        assert_eq!((layer.pixels.width, layer.pixels.height), (4, 2));
        assert_eq!(layer.position, Point::ZERO);
        assert_eq!(layer.pixels.get_pixel(2, 0), Color::RED, "可见的红像素还在");
        // 已经对齐 -> no-op。
        assert!(!document.crop_layer_to_document(id));
    }

    #[test]
    fn expanding_a_smaller_layer_grows_it_to_the_document() {
        let mut document = Document::new("d", 4, 4);
        let id = document.add_layer_with_pixels("small", PixelBuffer::filled(2, 2, Color::RED));
        assert!(document.set_layer_position(id, Point::new(1, 1)));

        assert_eq!(document.ensure_layer_covers_document(id), (1, 1));
        let layer = document.layer(id).unwrap();
        assert_eq!(
            (layer.pixels.width, layer.pixels.height),
            (4, 4),
            "补大到文档"
        );
        assert_eq!(layer.pixels.get_pixel(1, 1), Color::RED, "内容按偏移落下");
        assert_eq!(layer.pixels.get_pixel(0, 0), Color::TRANSPARENT);
    }

    #[test]
    fn opacity_is_clamped_through_the_document() {
        let mut document = Document::new("d", 2, 2);
        let id = document.layers[0].id;
        assert!(document.set_layer_opacity(id, 2.0));
        assert_eq!(document.layer(id).unwrap().opacity, 1.0);
        assert!(document.set_layer_opacity(id, -1.0));
        assert_eq!(document.layer(id).unwrap().opacity, 0.0);
    }

    #[test]
    fn move_layer_reorders_bottom_and_top() {
        let mut document = Document::new("d", 1, 1);
        let bottom = document.layers[0].id;
        let middle = document.add_layer("middle");
        let top = document.add_layer("top");
        assert_eq!(document.layers[0].id, bottom);

        // 把最底层移到最上面。
        assert!(document.move_layer(bottom, usize::MAX));
        assert_eq!(document.layers.last().unwrap().id, bottom);
        assert_eq!(document.layers[0].id, middle);
        assert_eq!(document.layers[1].id, top);

        // 再把底移回最底。
        assert!(document.move_layer(bottom, 0));
        assert_eq!(document.layers[0].id, bottom);
        assert_eq!(document.layers[2].id, top);
    }

    #[test]
    fn mutations_advance_the_revision_but_selection_does_not() {
        let mut document = Document::new("d", 2, 2);
        let start = document.revision();
        let id = document.add_layer("L");
        assert_eq!(document.revision(), start + 1);
        assert!(document.set_layer_visible(id, false));
        assert_eq!(document.revision(), start + 2);
        assert!(document.select_layer(document.layers[0].id));
        assert_eq!(document.revision(), start + 2, "选中不是像素改动");
    }

    // -- 树 --------------------------------------------------------------

    /// 背景 + 组[ A（下）, B（上） ] + 顶（根，最上）。
    struct Tree {
        document: Document,
        background: LayerId,
        group: LayerId,
        inner_a: LayerId,
        inner_b: LayerId,
        top: LayerId,
    }

    fn tree() -> Tree {
        let mut document = Document::new("d", 2, 2);
        let background = document.layers[0].id;
        let group = document.add_group("组");
        let inner_a = document.add_layer_with_pixels("A", PixelBuffer::new(2, 2));
        let inner_b = document.add_layer_with_pixels("B", PixelBuffer::new(2, 2));
        let top_layer = Layer::new("顶", PixelBuffer::new(2, 2));
        let top = top_layer.id;
        assert!(document.insert_subtree(vec![top_layer], None, usize::MAX));
        document.select_layer(top);
        document.validate().expect("树不变量");
        Tree {
            document,
            background,
            group,
            inner_a,
            inner_b,
            top,
        }
    }

    #[test]
    fn a_group_is_contiguous_with_its_children_and_parented_correctly() {
        let Tree {
            document,
            background,
            group,
            inner_a,
            inner_b,
            top,
        } = tree();
        assert_eq!(
            document.layers.iter().map(|l| l.id).collect::<Vec<_>>(),
            vec![background, group, inner_a, inner_b, top]
        );
        assert!(document.layer(group).unwrap().is_group());
        assert_eq!(document.layer(inner_a).unwrap().parent, Some(group));
        assert_eq!(document.layer(inner_b).unwrap().parent, Some(group));
        assert_eq!(document.children(Some(group)), vec![inner_a, inner_b]);
        assert_eq!(document.children(None), vec![background, group, top]);
        assert_eq!(document.subtree_range(group).unwrap(), 1..4);
        document.validate().expect("树不变量");
    }

    #[test]
    fn rows_are_top_first_with_depth_and_respect_collapse() {
        let Tree {
            mut document,
            group,
            inner_a,
            inner_b,
            ..
        } = tree();
        let rows = document.rows();
        assert_eq!(
            rows.iter()
                .map(|row| (row.name.as_str(), row.depth))
                .collect::<Vec<_>>(),
            vec![("顶", 0), ("组", 0), ("B", 1), ("A", 1), ("背景", 0)]
        );
        assert!(rows[1].is_group);

        assert!(document.set_expanded(group, false));
        let rows = document.rows();
        assert_eq!(
            rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
            vec!["顶", "组", "背景"],
            "折叠的组不列出子节点"
        );
        assert!(!rows
            .iter()
            .any(|row| row.id == inner_a || row.id == inner_b));
    }

    #[test]
    fn moving_a_group_moves_its_whole_subtree_and_keeps_it_contiguous() {
        let Tree {
            mut document,
            background,
            group,
            inner_a,
            inner_b,
            top,
        } = tree();
        // 组移到根的最上面：子树整体搬过去。
        assert!(document.move_node(group, None, usize::MAX));
        document.validate().expect("树不变量");
        assert_eq!(
            document.layers.iter().map(|l| l.id).collect::<Vec<_>>(),
            vec![background, top, group, inner_a, inner_b]
        );

        // 再把组放到根的最下面。
        assert!(document.move_node(group, None, 0));
        document.validate().expect("树不变量");
        assert_eq!(
            document.layers.iter().map(|l| l.id).collect::<Vec<_>>(),
            vec![group, inner_a, inner_b, background, top]
        );
    }

    #[test]
    fn a_group_cannot_be_moved_into_its_own_subtree() {
        let Tree {
            mut document,
            group,
            inner_a,
            ..
        } = tree();
        assert!(!document.can_move(group, Some(inner_a), 0), "A 不是组");
        let nested = document.add_group("子组");
        assert!(!document.can_move(nested, Some(nested), 0), "不能移进自己");
        assert!(!document.move_node(group, Some(group), 0));
        assert!(document.can_move(nested, Some(group), 0));
        assert!(document.move_node(nested, Some(group), 0));
        document.validate().expect("树不变量");
    }

    #[test]
    fn removing_a_group_drops_its_children_and_selects_a_neighbour() {
        let Tree {
            mut document,
            group,
            inner_a,
            inner_b,
            top,
            ..
        } = tree();
        assert!(document.select_layer(group));
        assert!(document.remove_layer(group).is_some());
        assert!(document.layer(inner_a).is_none() && document.layer(inner_b).is_none());
        assert_eq!(document.active_layer, Some(top), "选同位置的上一个兄弟");

        // 删组里的最后一层：回落到父组。
        let Tree {
            mut document,
            group,
            inner_a,
            inner_b,
            ..
        } = tree();
        assert!(document.select_layer(inner_b));
        assert!(document.remove_layer(inner_b).is_some());
        assert_eq!(document.active_layer, Some(inner_a));
        assert!(document.remove_layer(inner_a).is_some());
        assert_eq!(document.active_layer, Some(group));
        document.validate().expect("树不变量");
    }

    #[test]
    fn groups_are_not_paintable_and_have_no_position() {
        let Tree {
            mut document,
            background,
            group,
            ..
        } = tree();
        assert!(document.select_layer(group));
        assert!(document.active_surface_mut().is_none(), "组不可画");
        assert!(!document.set_layer_position(group, Point::new(1, 1)));
        assert_eq!(document.ensure_layer_covers_document(group), (0, 0));
        assert!(!document.crop_layer_to_document(group));

        // 选回栅格层就又能画了。
        assert!(document.select_layer(background));
        assert!(document.active_surface_mut().is_some());
    }

    #[test]
    fn effective_visibility_follows_the_ancestors() {
        let Tree {
            mut document,
            group,
            inner_a,
            ..
        } = tree();
        assert!(document.effective_visible(inner_a));
        assert!(document.set_layer_visible(group, false));
        assert!(!document.effective_visible(inner_a), "隐藏的组藏起整棵子树");
        assert!(document.set_layer_visible(group, true));
        assert!(document.effective_visible(inner_a));
    }

    #[test]
    fn nudging_stays_inside_the_parent() {
        let Tree {
            mut document,
            background,
            group,
            inner_a,
            inner_b,
            ..
        } = tree();
        // A 已经在组的最下面，不能再往下。
        assert!(!document.nudge_layer(inner_a, -1));
        assert!(document.nudge_layer(inner_a, 1));
        assert_eq!(document.children(Some(group)), vec![inner_b, inner_a]);
        // 背景是根里最下面的一层。
        assert!(!document.nudge_layer(background, -1));
        assert!(document.nudge_layer(background, 1));
        assert_eq!(document.children(None)[0], group);
    }

    #[test]
    fn masks_can_be_attached_inverted_and_toggled() {
        let mut document = Document::new("d", 2, 2);
        let id = document.layers[0].id;
        assert!(!document.set_active_mask(true), "没有蒙版时不能切到蒙版");

        let mut mask = LayerMask::opaque(2, 2);
        mask.pixels.set_pixel(0, 0, Color::BLACK);
        assert!(document.set_layer_mask(id, mask));
        assert_eq!(document.layer(id).unwrap().mask_coverage(0, 0), 0.0);
        assert!(document.set_active_mask(true));
        assert!(matches!(
            document.active_surface_mut().map(|surface| surface.target),
            Some(PixelTarget::Mask)
        ));

        assert!(document.invert_layer_mask(id));
        assert_eq!(document.layer(id).unwrap().mask_coverage(0, 0), 1.0);

        assert!(document.set_mask_enabled(id, false));
        assert_eq!(document.layer(id).unwrap().mask_coverage(0, 0), 1.0);
        assert!(document.remove_layer_mask(id).is_some());
        assert!(document.layer(id).unwrap().mask.is_none());
        assert!(!document.active_mask, "删掉蒙版后编辑目标回到图层");
    }

    #[test]
    fn growing_a_layer_grows_its_mask_with_the_same_offset() {
        let mut document = Document::new("d", 4, 4);
        let id = document.layers[0].id;
        assert!(document.set_layer_position(id, Point::new(2, 0)));
        let mut mask = LayerMask::opaque(4, 4);
        mask.pixels.set_pixel(0, 0, Color::BLACK);
        assert!(document.set_layer_mask(id, mask));

        assert_eq!(document.ensure_layer_covers_document(id), (2, 0));
        let mask = document.layer(id).unwrap().mask.as_ref().unwrap();
        assert_eq!((mask.pixels.width, mask.pixels.height), (6, 4));
        assert_eq!(mask.pixels.get_pixel(2, 0), Color::BLACK, "蒙版跟着平移");
    }

    #[test]
    fn new_layers_land_inside_the_active_group() {
        let Tree {
            mut document,
            group,
            ..
        } = tree();
        assert!(document.select_layer(group));
        let inside = document.add_layer_with_pixels("组内", PixelBuffer::new(2, 2));
        assert_eq!(document.layer(inside).unwrap().parent, Some(group));
        document.validate().expect("树不变量");
    }
}
