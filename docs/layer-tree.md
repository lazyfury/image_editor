# 图层能力：树形分组 / 蒙版 / 拖拽 / 上下文菜单（调研 + 设计）

状态：**已实现**（4 批全部落地；画布蒙版红色叠加、逐格点击、图层缩略图随后补上，
当前 534 个测试 + `--selfcheck` 绿）。拍板结果：
隔离组、蒙版第一批就支持画笔编辑、新建组两项都给（新建空组 + 编组当前节点）、
不做多选、缩略图后置。§2 的「不做」列表仍然成立 —— 合并图层、剪贴蒙版、
调整图层、多选都不在这份设计里。

落地时的偏差（都是有意的，按重要性排）：

1. **画布上的蒙版预览**：已实现为「快速蒙版」式的红色叠加 —— `active_mask` 时
   `take_texture_upload` 在合成 / 棋盘格之后把被遮住的像素染红，只存在于显示纹理
   （导出 / 缩略图不受影响，有测试）。没有做的是「整块画布显示蒙版灰度」那种
   独立视图，也没有给蒙版单开纹理。
2. **蒙版的画笔颜色按亮度转灰度**（黑 = 隐藏、白 = 显示），橡皮写黑；油漆桶 /
   渐变 / 移动 / 裁切 / 剪切在蒙版编辑时被拦掉并提示（只有画笔 / 橡皮能写蒙版）。
3. **右键菜单的「重命名」锚在面板的「重命名」按钮上**（`List` 没有逐格点击，
   拿不到行内热点位置）。
4. **`.igtl` 时间线**对蒙版笔画只记 label、不记像素补丁（格式里没有 target 字段）；
   图层笔画照旧有补丁。
5. **逐格点击**：已随 igui **v0.5.1** 落地（`List::on_cell`）。眼睛 = 文字列、
   三角 / 蒙版 = icon leads，点格子不会顺带激活整行；单元格下标见 `ui/layer_panel.rs`
   （0 缩进 / 1 三角 / 2 蒙版 / 3 眼睛 / 4 名字 / 5 不透明度）。
6. 组的「新建组」在右键菜单里是**新建空组**（放在目标节点上方）；`Cmd+G` 与菜单栏
   的「编组」才是把当前节点裹进组。

## 0. 结论摘要

| 问题 | 结论 |
|---|---|
| 现在能做到什么？ | 平铺 `Vec<Layer>`，图层面板只有一个列表 + 7 个作用在「当前图层」的按钮；右键 / 拖拽 / 分组 / 蒙版都没有。 |
| 树形分组要重构模型吗？ | **不重构成递归树**：保持 `Document.layers: Vec<Layer>`，加 `parent` + 「子树在数组里连续」的不变量（前序 DFS）。现有约 100 处 `.layers` 用法基本不动。 |
| 合成要不要改？ | 要：分组是**隔离组**（先合成到区域大小的临时缓冲，再按组的不透明度 / 蒙版压回父级）；蒙版按覆盖率乘 alpha。脏矩形仍然成立。 |
| 拖拽排序要不要改 igui？ | **不用**。`ListState::rows()` / `offset()` / `scroll_by()` 都是公开的（`list.rs`），应用层自己拦截指针 + 画投放指示线即可。 |
| 上下文菜单呢？ | 现成：`List::on_context(|row, position|)`（`list.rs:597`）+ `Overlays::menu_at(position, …)`（`overlay/mod.rs:315`）+ `MenuItem`（`.disabled()` / `.shortcut()`）。 |
| 逐格点击 | 已解决：igui v0.5.1 的 `List::on_cell(row, cell)`（leads 在前、列在后；点格子不顺带激活整行），眼睛 / 三角 / 蒙版各点各的。 |
| 工作量 | 约 **6–7 人日**（§9 四批），每批 1–2.5 天。 |

## 1. 调研：现状与可用的积木

### 1.1 模型（`src/document/`）

- `Document { layers: Vec<Layer>, active_layer: Option<LayerId>, … }`，`layers[0]` 是**最底层**
  （`document.rs`）。图层是扁平数组，「顺序」就是数组顺序。
- `Layer { id, name, visible, opacity, blend_mode, position, pixels }`（`layer.rs`）。
  没有 parent、没有子节点、没有蒙版。
- 增删改排序都在 `Document`：`add_layer_with_pixels`（push）、`remove_layer`（相邻回落）、
  `move_layer(id, new_index)`、`rename/visible/opacity/position`、`ensure_layer_covers_document`、
  `crop_layer_to_document`。
- 撤销：`LayerStackMeta`（`history/layers.rs`）只快照 **id / name / visible / opacity /
  blend / position / active**，像素不走它（`PaintCommand` 单独记录区域差异）。
- 像素命令 `PaintCommand { layer: LayerId, region, before, after, label }`
  （`history/paint.rs`），画笔开笔画的时候用 `ensure_layer_covers_document` 对齐缓冲、
  抬笔用 `capture_in` 只存差异。

### 1.2 合成（`src/renderer/cpu.rs`）

- `render_region` 清区域 → 按 `document.layers` **从下到上**顺序 `blend_layer`；
  缩放到区域、图层偏移在 `blend_layer` 里换算；`render_region` 与整图结果一致
  是现有的硬测试（`a_partial_render_matches_the_full_one_inside_the_region`）。
- `sample_pixel`（吸管）把同一套规则在单像素上重算一遍 —— 也是扁平循环。
- 结构改动（增删图层 / 可见性 / 排序 / 不透明度）由视图 `mark_texture_dirty()` 触发整图重合成，
  画笔笔画只脏自己的包围盒。

### 1.3 图层面板（`src/ui/layer_panel.rs`）

- 用 `igui_components::List`（虚拟化）：`source(index) -> Vec<String>` 给三列
  「眼睛 / 名字 / 不透明度」，`index 0` 是**最上面的图层**（`len - 1 - index` 翻转）。
- 选中行写 `Document.select_layer(id)`；`refresh_layers()`（`ui/sync.rs`）负责
  `count` / `selected` / `invalidate`。
- 7 个按钮全部作用在「当前图层」：`+ 图层`、`− 删除`、`重命名`、`显示/隐藏`、
  不透明度 `−`/`+`、`↑`/`↓`。
- 没有缩略图（行里只有文字与 `◉`/`○`）。

### 1.4 igui 侧现成的积木（不用改上游）

| 需求 | 现成 API | 位置 |
|---|---|---|
| 行缩进 | `List::spacer(|index| f32)` | `list.rs` |
| 行图标（自绘） | `List::icon(width, |ctx, rect, index|)` | `list.rs` |
| 右键 | `List::on_context(|index, position|)` | `list.rs:597` |
| 指针位置的菜单 | `Overlays::menu_at(position, content)` | `overlay/mod.rs:315` |
| 菜单项（禁用 / 快捷键提示） | `Menu::new(theme)` + `MenuItem::disabled()` | `components/menu.rs` |
| 行几何（拖拽用） | `ListState::rows()` / `visible_range()` / `offset()` / `scroll_by()` | `list.rs:235-283` |
| 双击事件 | `InputEvent::DoubleClick`（宿主已发） | `igui_core` / `igui_winit` |
| 拾取行（双击 / 拖拽） | `igui_ui::control(&tree, id).rect` | `igui_ui` |

**逐格点击**是唯一的缺口：`List` 的行只有整行 click / secondary，`LeadIcon` 明确
`MouseFilter::Ignore`。所以 v1 不把眼睛 / 三角 / 蒙版缩略图做成独立热点。

### 1.5 文件格式（`src/io/workspace.rs`）

```text
magic "IGUIWS01"
u32 width, height; [u8;4] background; string name
i32 active_layer_index; u32 layer_count
每层: string name, u8 visible, f32 opacity, u8 blend, i32 x/y,
      u32 pixel_w/h, u32 png_len + png
```

扁平、无 parent、无蒙版、无 `expanded`。`read_workspace_summary` 只读头。
`.igtl` 时间线只记录命令名 + 像素补丁，与图层结构无关。

### 1.6 工具与「当前图层」的耦合

`ctx.document.active_layer_mut()` 直接拿 `pixels` 写：
- 画笔（`tools/brush/mod.rs`，`writer()` / `begin_stroke()`：扩缓冲 + 快照 + 落笔）、
  油漆桶 / 渐变、移动工具（改 `position`）、裁到文档、粘贴为新图层。
- 一旦 `active_layer` 是**组**，这些路径都会写进一个 0×0 缓冲或改错对象，
  所以「组不可画」必须在模型层挡住（§5）。

## 2. 目标与非目标

目标（本设计覆盖）：

1. **树形分组**（文件夹）：组可以嵌套；组有自己的可见性 / 不透明度 / 蒙版；可展开折叠。
2. **蒙版**：栅格层与组都能挂一张蒙版（像素蒙版），影响合成 alpha；可从选区创建、
   反相、启用 / 停用、删除；第二批支持画笔直接编辑。
3. **拖拽排序 / 换父**：拖到兄弟之间 = 改顺序；拖到组中段 = 放进组；非法目标拒绝；
   拖动时画投放指示线、边缘自动滚动、悬停组自动展开。
4. **切换**：整行选中（组也可选中）、双击组展开 / 折叠、右键菜单里的可见性 / 蒙版开关、
   菜单栏「图层」菜单与快捷键。
5. **上下文菜单**：右键任意行弹出，项目随节点类型 / 选区 / 蒙版状态启停。

非目标（明确不做，避免范围爆炸）：

- 混合模式（`BlendMode` 仍然只有 `Normal`；组的模式固定 `Normal`，不做 pass-through）。
- 剪贴蒙版（alt 点击两行之间的「剪贴到下一层」）、矢量蒙版、调整图层、智能对象。
- 多选图层（`List` 是单选；编组、删除都以「当前节点」为单位）。
- 合并图层 / 合并可见 / 拼合图像（后续单独一条）。
- 图层缩略图（需要每层一张纹理；v1 先用字形，见 §6.4 的后续项）。
- 画布上直接拖拽图层内容（移动工具已经能做，不改）。

## 3. 数据模型

### 3.1 用「扁平数组 + parent + 连续子树」而不是递归树

```rust
pub enum LayerKind { Raster, Group }

pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub kind: LayerKind,
    /// 父节点（`None` = 根）；组才有子节点。
    pub parent: Option<LayerId>,
    /// 组是否展开（UI 状态；不进撤销，但写进工作区文件）。
    pub expanded: bool,
    pub visible: bool,
    pub opacity: f32,
    pub blend_mode: BlendMode,
    pub position: Point,
    pub pixels: PixelBuffer,          // 组用 `PixelBuffer::new(0, 0)`，永不渲染
    pub mask: Option<LayerMask>,
}

pub struct LayerMask {
    /// 灰度图：`r == g == b` = 覆盖率，`a = 255`。尺寸与图层缓冲一致。
    pub pixels: PixelBuffer,
    pub enabled: bool,
}
```

`Document.layers` 仍然是**一个数组**，顺序是**前序 DFS、从下到上**：

```text
[ 组A, 子1, 子2, 组B, 子3, 根C ]        # 组A 在它的子节点之前；子节点按从下到上
```

不变量（写进 `Document` 的文档与测试）：

1. `layers` 的前序 DFS 与 `parent` 自洽：每个节点的子树在数组里是**连续区间**
   `start..start+len`，`parent` 的 index 小于子节点。
2. `parent` 必须指向 `Group` 节点；不允许环（由 1 自动保证）。
3. 组没有像素（`pixels` 为空、`position` 恒为 `ZERO`）；渲染时跳过。

为什么不用递归枚举 `LayerNode { Raster(Layer), Group(Group { children }) }`：

- 现有 ~100 处 `.layers` 访问（`document.rs` 42、`cpu.rs` 15、`workspace.rs` 11、
  `history/layers.rs` 10、面板 / 剪贴板 / 自检…）都是「数组 + `layer_index`」的写法，
  扁平数组能保住 `layers[0]`、`iter()`、`layer(id)` 这些语义，改动集中在少数新 API；
- 撤销 / 文件格式 / 渲染本来就是按顺序遍历，数组最直接；
- 递归枚举的真实代价在「取第 n 行」「拖拽目标换算」，扁平数组反而更容易。

代价（必须用测试钉住）：任何一次的插入 / 删除 / 移动都必须维护**子树连续**；
`move_layer` 不能再用「remove + insert 单个元素」，要按子树搬。

### 3.2 `Document` 新增 API（模型层，纯函数、可单测）

```rust
impl Document {
    // 查询
    pub fn is_group(&self, id: LayerId) -> bool;
    pub fn children(&self, parent: Option<LayerId>) -> &[LayerId]  // 或迭代器
    pub fn subtree_range(&self, id: LayerId) -> Option<Range<usize>>;
    /// 前序 DFS（从下到上）的渲染顺序（就是 `layers` 本身，供文档说明）。
    pub fn render_order(&self) -> impl Iterator<Item = &Layer>;
    /// 面板行：从上到下、考虑折叠；`depth` 是缩进层级。
    pub fn rows(&self) -> Vec<LayerRow>;

    // 变更（全部返回 `bool` 表示是否真的改了）
    pub fn insert_subtree(&mut self, nodes: Vec<Layer>, parent: Option<LayerId>, index: usize) -> bool;
    pub fn remove_subtree(&mut self, id: LayerId) -> Option<Vec<Layer>>;
    pub fn move_node(&mut self, id: LayerId, parent: Option<LayerId>, index: usize) -> bool;
    pub fn can_move(&self, id: LayerId, parent: Option<LayerId>, index: usize) -> bool;
    pub fn set_expanded(&mut self, id: LayerId, expanded: bool);

    // 渲染 / 活动目标
    pub fn effective_visible(&self, id: LayerId) -> bool;   // 含祖先
    pub fn active_surface(&self) -> Option<ActiveSurface>;  // Raster / Mask / None
}

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
```

`rows()` 是面板 / 自检 / 拖拽目标**共用的唯一行模型**（渲染顺序 → 显示顺序只在这里翻转），
避免每个调用点各算一套下标（现有面板就是自己翻 `len-1-index`）。

### 3.3 组语义

- **隔离组**（isolated）：子节点先合成到透明缓冲，再按组的 `opacity` / `visible` / `mask`
  压回父级。不做 Photoshop 的 pass-through（那需要把组的混合模式透传下去，和
  「组有自己的不透明度」组合出很多边界情况）。
- 组的 `visible = false` → 整棵子树都不画（`effective_visible` 用祖先与自身相与）。
- 组的 `opacity` 作用在**合成后的组结果**上（不是逐个乘到子层）。
- 组不参与移动工具 / 画笔 / 裁到文档；`active_surface()` 对组返回 `None`。

## 4. 合成

### 4.1 递归 + 区域大小的临时缓冲

```text
render_region(document, target, region):
    clear(region)
    for root in document.roots():          # 从下到上
        composite_node(root, target, region, region_origin = 0)

composite_node(node, target, region):
    if !node.visible: return
    match node.kind:
      Raster:
        if let Some(mask) = node.mask.enabled:
            blend_layer_with_mask(target, node, region)   # alpha *= coverage
        else:
            blend_layer(target, node, region)             # 现有实现
      Group:
        let mut scratch = pool.take(region.size)          # 透明、区域大小
        for child in children(node):                      # 从下到上
            composite_node(child, &mut scratch, region)
        apply opacity + group mask to scratch (逐像素 alpha)
        blend_scratch_into(target, scratch, region)
```

- 临时缓冲只开 `region` 大小：画笔一笔时区是几十×几十，4K 文档也不会每帧分配
  8 MB × 组数。整图合成（导出 / 结构改动）才会开到文档大小。
- 复用池：`CpuRenderer` 现在是零大小 `Copy` 结构；临时缓冲放进 `render_region` 的
  局部 `Vec<PixelBuffer>`（递归深度 = 组嵌套深度，通常 < 5）。
- `blend_layer` 改成「目标有区域原点 + 图层偏移」的坐标换算（现有代码已经这样算，
  只是目标原点隐含为 0）。
- `sample_pixel`（吸管）走同一棵树的递归版本，规则一致。
- 蒙版覆盖率：`coverage = mask.pixels.get_pixel(x, y).r as f32 / 255.0`，
  `alpha *= coverage`；`coverage == 0` 直接跳过（省一次混合）。

### 4.2 脏矩形与不变量

- 规则不变：**结构性改动整图重合成**（增删/移动/分组/可见性/不透明度/蒙版开关），
  画笔只脏包围盒。
- 新增硬测试：**有组 / 有蒙版时，`render_region` 与 `render` 在区域内逐像素一致**
  （扩展现有 `a_partial_render_matches_the_full_one_inside_the_region`）。
- 组的 scratch 是区域大小，所以「局部合成 = 整图合成的一部分」这条性质天然成立。

### 4.3 性能预算

| 场景 | 现有 | 加组 / 蒙版 |
|---|---|---|
| 1 万层 × 4K 的整图 | O(层数 × 区域) | 同量级（组不增加像素遍历，只多一次 scratch→target 的混合） |
| 画笔一笔（64×64 区域，3 层 1 组） | ~0.1 ms | +scratch 分配/清零（池化后 ≈ 0.02 ms） |
| 缩略图 / 导出 | 一次整图 | 同 |

`render_region` 每次分配 scratch 是唯一的可见成本，池化后可以忽略；测量口径沿用
`--selfcheck` 的帧统计 + `todo.md` 的性能表。

## 5. 工具 / 活动目标

### 5.1 三种活动对象

| `active_layer` 指向 | 画笔 / 填充 / 渐变 / 移动 | 说明 |
|---|---|---|
| 栅格层 | 正常 | 现有行为 |
| 组 | **不响应**（状态栏提示「这是图层组」） | 避免写 0×0 缓冲 |
| 栅格层 + `active_mask = true` | 写蒙版（第 4 批） | 颜色按亮度转成覆盖率 |

- `Document.active_mask: bool`：只在 `active_layer` 有蒙版时为 `true`；换选中节点 /
  选中组时自动复位为 `false`。
- `active_surface()` 返回 `{ id, target: Layer | Mask, pixels, position }`，
  画笔 / 桶 / 渐变都改成通过它取写目标（`ensure_layer_covers_document` 对蒙版同样适用，
  因为蒙版与图层缓冲同尺寸同位置）。
- `PaintCommand` 增加 `target: PaintTarget { Layer, Mask }`（`translate_layer` / `capture_in`
  不变，只多一个字段），撤销 / 时间线按 target 写回。
- 移动工具 / 裁到文档 / 剪贴板 `paste as layer`：遇到组直接忽略并提示；粘贴时插到
  当前组的**末尾**（在组内），没有选中组就插到根末尾。

## 6. UI 设计

### 6.1 行模型与列

`List` 的 leads 在前、columns 在后，所以顺序是：

```
leads:    [缩进 spacer][▸/▾ icon 14px][蒙版 icon 16px]
columns:  [眼睛 22px][名字 flexible][不透明度 44px]
```

- 缩进：`List::spacer(|row| depth as f32 * 12.0)`。
- 三角 / 蒙版字形：`List::icon`（自绘）；未展开的组画 `▸`，展开画 `▾`；有蒙版的行
  画 `◧`（停用画 `◨`），没有蒙版留空。
- 眼睛 / 名字 / 不透明度：现有三列不变。
- **行数据要缓存**：`rows()` 是 O(节点数)；`List` 的 `source` 每绑一行调一次，直接算会
  O(n²)。面板持一份 `Rc<RefCell<Vec<LayerRow>>>`，`refresh_layers()` 里重建一次，
  `source(index)` 只按下标读。
- 折叠的组：`rows()` 不返回它的子节点；`refresh_layers()` 更新 `count` + `selected`
  并 `invalidate()`，`List::sync` 重新绑定即可。
- 行 ≡ `LayerRow`（以 id 为准）；`selected` 用
  `rows().iter().position(|row| row.id == active_layer)` 换算成行下标。

### 6.2 交互表（v1，全部不需要逐格点击）

| 操作 | 行为 |
|---|---|
| 单击行 | 选中该节点（组也可选中）；`active_mask = false` |
| 双击行（`InputEvent::DoubleClick` + 自己按行几何命中） | 组：切换展开 / 折叠；栅格层：打开重命名浮层 |
| 右键行 | 在指针处打开上下文菜单（`List::on_context` + `Overlays::menu_at`） |
| 右键空白处 | 菜单「新建图层 / 新建组 / 粘贴为新图层」 |
| 拖拽行 | 排序 / 换父（§6.3） |
| 面板按钮 | 保留；`↑`/`↓` 改成**同父内**上移 / 下移（不再用绝对下标） |
| `Enter` / `F2` | 重命名当前节点（面板有焦点时；v1 简化为面板按钮 + 菜单） |
| 键盘 `←` / `→` | 折叠 / 展开当前组（若焦点不在文本控件；由视图在 `event` 里处理） |

### 6.3 拖拽：应用层实现，不改 igui

**状态**（`EditorView`）：

```rust
struct LayerDrag {
    from: LayerId,
    pointer: Vec2,
    /// 超过 4px 才算拖拽（否则就是一次点击）。
    started: bool,
    target: Option<DropTarget>,
}

enum DropTarget {
    Before(LayerId),
    After(LayerId),
    Into(LayerId),        // 组的中段
    Root,                 // 列表下方空白 / 拖出组
}
```

**命中**：`ListState::rows()` 给出池里的行节点（视口顺序），配合
`igui_ui::control(&tree, row).rect` 得到矩形；数据下标是
`ListState::visible_range().start + slot`（池比视口多一行，先按列表容器矩形裁掉）。
行内纵向位置分三段：上 25% → `Before`、下 25% → `After`、中间 50% → 目标行是组就
`Into`，不是组就取最近的一侧。

**事件**：视图在 `PointerDown` 记下 `from`（不消费，让 `List` 正常选中）；`PointerMove`
超过阈值后消费事件，更新 `target`；靠近列表上下边缘时 `layer_state.scroll_by(±)` 自动滚动；
拖到组中段悬停 400ms 自动展开（并重算行几何）；`PointerUp` 消费并把
`(from, target)` 交给一条 `LayerMoveCommand`（记录 `from`/`to` 的 `(parent, index)`），
非法目标（拖进自己的子树 / 拖到原位）不产生命令，只提示。

**绘制**：在 `EditorView::paint` 的 UI 之后画：`Before/After` 在行边界画一条 2px 高亮线，
`Into` 把目标组整行描边；拖动中的源行不特殊处理（v1 不做半透明拖影）。

**为什么不用上游的 `drag_callback`**：`List` 的行没有挂 drag 回调，而
`set_on_drag` 需要行节点；行节点会随滚动复用，挂在应用侧反而简单。`ListState`
把行节点、行范围、滚动偏移都暴露了，这一层不需要改 igui。

### 6.4 上下文菜单（`Overlays::menu_at`）

| 组 | 菜单项 | 启用条件 |
|---|---|---|
| 通用 | 重命名 / 复制节点 / 删除节点 | 有活动节点 |
| 组 | 新建组 / 取消编组 | 当前是组 / 父是组 |
| 蒙版 | 添加蒙版（从选区）/ 删除蒙版 / 启用蒙版 / 反相蒙版 | 选区存在 / 有蒙版 / 有蒙版 / 有蒙版 |
| 排序 | 上移一层 / 下移一层（同父） | 有兄弟可换 |
| 其他 | 裁到文档 | 栅格层且缓冲 > 文档 |

菜单项的启用规则由 `Document` 纯函数决定（`can_*`），菜单内容与菜单栏「图层」菜单
**共用一张表**（沿用 `menu_spec` 的思路），避免两套文案漂移。

**菜单栏「图层」**：现在的 5 个占位项（`ui/menu.rs` 的 `layer_menu`）替换成
新建图层 / 新建组 / 复制 / 删除 / 编组 / 取消编组 / 蒙版子项（`MenuEntry` 没有子菜单，
先平铺）/ 上移 / 下移 / 裁到文档。

**快捷键**（宿主 `shortcuts.rs` 新增 `layer_shortcut`）：`Cmd+Shift+N` 新建图层、
`Cmd+G` 编组、`Shift+Cmd+G` 取消编组、`Cmd+J` 复制节点、`Cmd+Backspace` 删除节点
（`Backspace` 已经先被「删选区内容」截走 —— 顺序要写在文档里）。

### 6.5 属性面板 / 状态栏

- 属性面板：名字行显示 `组「名字」` / 栅格层；detail 行加 `不透明度`、`蒙版`、
  `子节点 N`；组不显示偏移 / 缓冲尺寸。
- 状态栏：选中组时工具名后面显示「（组）」；`active_mask` 时显示「（蒙版）」。

### 6.6 后续（不在 v1）

- 图层缩略图：**已实现**（`ui/layer_thumbs.rs`）。每层一张 ≤22px 的小纹理，
  `List::icon` 里 `draw_image`；只重建视口内的行 + 250ms 节流，纹理 id 进程内
  全局递增（多个工作区共用后端），删除图层的纹理不回收（后端没有删除接口，
  数量随会话增长，可接受）。
- 逐格点击（眼睛 / 三角 / 蒙版缩略图）：给 igui `List` 的 lead 加点击钩子
  （`LeadIcon` 改成可点击的 `MouseFilter::Stop` + `on_lead_click(lead, row, position)`），
  升一次 tag。
- 剪贴蒙版、合并、pass-through 组。

## 7. 历史 / 撤销

| 操作 | 命令 | 备注 |
|---|---|---|
| 改名 / 可见性 / 不透明度 / 位置 / 蒙版开关 / 展开 | `LayerMetaCommand`（快照扩展） | 元数据快照增加 `parent`、`expanded`、`mask.enabled`；`expanded` 不入快照（不算编辑，不产生撤销步） |
| 新建层 / 组 / 复制 | `AddLayerCommand` → `InsertNodesCommand { nodes, parent, index }` | 一次插入一棵子树 |
| 删除（含隐藏子节点） | `RemoveLayerCommand` → `RemoveNodeCommand { nodes, parent, index }` | 删组要把子树一起存 |
| 拖拽 / 上移下移 / 编组 / 取消编组 | `MoveNodeCommand { id, before: (parent,index), after: (parent,index) }` | 一次拖拽 = 一条命令 |
| 编组 | `GroupCommand { group, members: Range }` | 新建组 + 把当前节点搬进去，两步合成一条命令 |
| 取消编组 | `UngroupCommand` | 组消失、子节点按原顺序落到父级原位置 |
| 蒙版像素（画笔） | `PaintCommand`（`target: Mask`） | 与图层像素同一套差异记录 |
| 蒙版从选区 / 反相 / 删除 | `MaskCommand`（存前/后整张蒙版，或差异） | 蒙版是灰度缓冲，尺寸 = 图层缓冲；选区创建时整张写一次，直接存前后更简单 |

- 现有一笔 = 一步 undo 的语义不变。
- `LayerStackMeta` 的 `restore` 要按**子树连续**的规则重建：按快照逐节点 `parent` 重排，
  测试覆盖「拖动组之后再撤销」。

## 8. 文件格式 v2

```text
magic "IGUIWS02"
u32 width, height; [u8;4] background; string name
i32 active_index; u8 active_mask; u32 node_count
每个节点（前序 DFS）:
  string name
  u8  kind              # 0 = 栅格，1 = 组
  i32 parent_index      # -1 = 根
  u8  visible; f32 opacity; u8 blend; i32 position.x/y
  u8  expanded
  u8  has_mask          # 有蒙版时后面跟一张 PNG（灰度 RGBA）
  [string mask_png]     # 仅 has_mask
  栅格层: u32 pixel_w, pixel_h; u32 png_len + png
  组:     不写像素（读的时候 `PixelBuffer::new(0,0)`）
```

- **向后兼容**：magic `IGUIWS01` 继续可读（扁平树、`parent = None`、无蒙版、`expanded = true`）；
  `read_workspace_summary` 不变（`layers` 字段 = 节点数）。
- 写文件一律 v2（老版本本仓不再产出 v1）。
- `active_index` 仍是数组下标（前序 DFS），`active_mask` 新增；读回来时校验
  「active 是组时 active_mask 归 false」。
- `.igtl` 时间线不受影响（它只存命令名 + 像素补丁，图层 id 仍是进程内 id）。

### 8.1 迁移测试

- v1 字节（用现有 `sample()` 生成）解码 → 扁平树、每层 parent = None；
- v2 round-trip：嵌套组 + 组蒙版 + 折叠状态 + `active_mask`；
- 越界 parent / 组带像素 / 层数是 0 的坏文件 → `IoError::Invalid`（不 panic）。

## 9. 分批落地（全部完成）

### 第 1 批：模型 + 合成（约 2 天）— [x]

- `LayerKind` / `parent` / `expanded` / `mask`；
- `Document` 的树 API（§3.2）+ 不变量测试（`Document::validate`）；
- 渲染器递归 + 隔离组 + 蒙版覆盖率 + `sample_pixel`（区域合成 = 整图）；
- 保护现有工具：组不可写（`active_surface_mut`），移动 / 裁剪 / 画笔在组上 no-op；
- 文件格式 v2 + v1 兼容（`IGUIWS02` / `IGUIWS01`）；
- 验收：`cargo test`；渲染测试（组不透明度 / 隐藏组 / 组蒙版 / 区域一致）；
  旧 `.igui` 能打开。

### 第 2 批：图层面板树形 + 上下文菜单 + 菜单栏 / 快捷键（约 1.5 天）— [x]

- `layer_list` 换成 `rows()` 驱动：缩进、三角、蒙版字形、折叠；
- 右键菜单（`on_context` + `menu_at`）+ 菜单栏「图层」+ `layer_shortcut`；
- 面板按钮 `↑`/`↓` 改成同父移动；属性面板 / 状态栏显示组 / 蒙版；
- 验收：面板测试（缩进、折叠后行数、右键菜单项与启用状态、上移只在同父内）；
  `--selfcheck` 加一条「建组 + 折叠 + 展开」的行数检查。

### 第 3 批：拖拽（约 1 天）— [x]

- 行几何命中（`ListState::rows` + `control().rect`）、阈值、自动滚动、悬停展开；
- `LayerMoveCommand`；非法目标拒绝 + 状态栏提示；
- 投放指示线绘制；
- 验收：单测覆盖「目标换算」（给定指针 Y 与行矩形 → `Before/After/Into`）、
  「不能拖进自己的子树」；拖动一条命令一步撤销。

### 第 4 批：蒙版编辑（约 2–2.5 天）— [x]（画布预览另开一条）

- `Document.active_mask` + `active_surface()` 接到画笔 / 桶 / 渐变 / `PaintCommand.target`；
- 蒙版像素画的颜色 → 覆盖率（黑 = 隐藏 / 白 = 显示，灰度按亮度）；
- 画布显示：`active_mask` 时把蒙版灰度作为图像、红色半透明叠加（或「显示蒙版」开关）；
- 蒙版从选区 / 反相 / 删除 / 启用（第 2 批已有菜单，这里接实现与撤销）；
- 验收：蒙版落笔只改蒙版、撤销只撤回蒙版；`active_mask` 换节点自动复位；
  渲染测试覆盖「画黑 → 隐藏」。

## 10. 测试矩阵（按硬规则：一条测试一条规则）

| 层 | 测试 |
|---|---|
| 模型 | 前序 DFS 连续 / `move_node` 维护连续 / 不能移进自己子树 / `rows()` 折叠与深度 / `effective_visible` 含祖先 |
| 合成 | 组不透明度 = 对合成结果乘一次（不是逐子层）/ 隐藏组整棵不画 / 组蒙版乘 alpha / 层级蒙版 / 区域合成 = 整图对应区域 / 吸管与合成一致 |
| 历史 | 拖动组撤销回原位 / 删组撤销带回子节点 / 编组 / 取消编组 / 蒙版笔画一步撤销 |
| 文件 | v1 读、v2 round-trip、坏文件拒绝 |
| 面板 | 行数随折叠变化 / 选中行与 `active_layer` 同步 / 右键菜单启用规则 / 上移只在同父内 |
| 拖拽 | 目标换算（三段的边界）/ 非法目标 / 一步撤销 |
| 自检 | 建组 + 折叠 + 展开后的行数与文字；组不透明度影响一个像素 |

## 11. 风险 / 开放问题 / 工作量

| 风险 | 影响 | 缓解 |
|---|---|---|
| 子树连续不变量被某个插入路径破坏 | 渲染顺序错、撤销错位 | 所有插入 / 删除 / 移动只走 §3.2 的 API；`debug_assert` 校验 + 属性测试 |
| 组 scratch 的区域语义写错（坐标原点） | 局部合成出现错位 | 先写「区域合成 = 整图」测试，再实现组 |
| 蒙版画笔的「颜色 → 覆盖率」不直观 | 用户不知道画黑是隐藏 | 第 4 批加画布蒙版显示 + 状态栏提示；先上「从选区创建 / 反相」也能用 |
| 拖拽在 `List` 的回收行上抖动 | 行几何在滚动时变 | 每帧重新读 `rows()` + rect，不缓存 NodeId → rect |
| 逐格点击缺失 | 眼睛 / 三角不能单独点 | v1 双击 / 右键替代；需要时给上游 `List` 加 lead 点击钩子（一次 tag 升级） |
| 与混合模式 / 剪贴蒙版的需求相互牵扯 | 设计返工 | 组固定隔离 + `Normal`，枚举预留；不做 pass-through |

**开放问题（已拍板）**：

1. 组的语义：**隔离组**（已实现）。
2. 蒙版第一批就能画笔编辑（已实现；画布上的蒙版预览后置）。
3. 「新建组」两项都给：右键「新建空组」+ `Cmd+G` / 菜单「编组当前节点」（已实现）。
4. 多选：v1 不做。
5. 图层缩略图：后置（需要每层一张纹理 + 只传脏层的缓存）。

## 12. 工作量估算

| 批次 | 内容 | 估时 |
|---|---|---|
| 1 | 模型 + 合成 + 文件 v2 | 2 天 |
| 2 | 面板树形 + 上下文菜单 + 菜单栏 / 快捷键 | 1.5 天 |
| 3 | 拖拽排序 / 换父 | 1 天 |
| 4 | 蒙版编辑（画笔 / 显示） | 2–2.5 天 |
| 合计 | 可独立验收的 4 批 | **6.5–7 天** |

不做项（合并 / 剪贴蒙版 / pass-through / 多选 / 缩略图）不在估算内。
