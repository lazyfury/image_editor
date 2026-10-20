# image_editor — TODO

这是 image_editor 自己的项目级待办（独立 git 仓库，通过 git 依赖引用上游 igui）。
**从上往下做**；每条写清「改哪里 / 怎么验收」。与 igui 仓的路线图分开，别混。

约定（见 `AGENTS.md`）：每阶段 **API → 测试 → 实现 → 集成**，结束出报告等确认；
验证只允许程序化（DrawList 命令 / 布局矩形 / 回调 / 后端像素缓冲），**不做截图**。

最近落地：单窗口 + `igui_app` 运行时（`src/app/application.rs` 用
`WinitPlugin`/`WgpuPlugin`/输入插件组装，业务逻辑是一个 `AppLogic`）；「新建
项目」现在是主窗口内的**模拟桌面浮动窗口**（`ui/window/` 的 `Window` +
`ui/new_document.rs` 的 `NewDocumentModal`：标题栏拖动移动、四条边 / 角拖动
缩放、关闭按钮 / 遮罩，Esc / 点外取消，输入全吞掉），不再开第二个原生窗口；
侧栏「文件 / 历史 / 属性」标签页；透明棋盘格背景；默认 128×128 像素画布 +
1px 硬边笔 + 最近邻采样；移动图层后画笔对准光标、画布外像素不裁。

再最近：**原生文件对话框 + 保存流程 + 拖放**（Phase 23）——文件面板不再有
路径输入，每个按钮只发 `FileRequest` 意图，路径由 `io/dialog.rs` 手写的平台
FFI 选出来；「保存 / 另存为」分开；关闭脏工作区先确认；PNG 拖进窗口当新图层。

**当前主线：第 2 节的「绘画 / 修图路线图」**（绘画优先，一批一批落地）。

---

## 1. 主线：像素模式（已完成）

目标：任意尺寸都能画**硬边像素**，可选**方形笔**，并接到选项栏。
已定：默认 `hard = true`、`shape = Square`；两个独立开关（「像素」+「方形」）；
状态用 `Rc<Cell>` 管显示、`self.brush` 管绘制、每帧同步。

- [x] **P1 — `BrushTool` 硬边 + 形状**（`src/tools/brush.rs`）：
  `BrushShape { Round, Square }` + `BrushTool { hard, shape }`；`hard` 时圆心吸附
  像素网格（所有尺寸），`coverage` 二值（Round 比距离、Square 比整数边长 n，
  奇数居中 / 偶数把光标像素放左上）。测试：硬边只有纯色、软边有灰边、
  方形 3×3 / 2×2、size 1 不变。
- [x] **P2 — 选项栏开关**（`src/ui/options_bar.rs` + `src/ui/mod.rs`）：
  画笔配置加「像素」「方形」两个开关（`Rc<Cell>` + `dynamic_background`），
  `update` 同步进 `self.brush`；`EditorView` 暴露
  `pixel_mode`/`square_mode`/`brush_hard`/`brush_shape`/`brush_toggle_center`。
  `--selfcheck` 用一个新视图验证 size 3 硬边无灰边、软边（圆头）有灰边。
- [x] **P5 — 文档**：README / `AGENTS.md` 已补像素模式。
- [x] **P3 — 像素网格叠加**：像素模式开启且 `zoom >= GRID_MIN_ZOOM`(6) 时，在画布上
  画文档像素边界的 1px 线（`DrawCommand::Line`，屏幕空间、不随缩放变粗），只画
  「文档矩形 ∩ 画布区域」；测试断言放大+像素模式会多出网格线、缩小后不画。
- [x] **P4 — 像素完美连线**：1px 硬边笔改用 Bresenham 连接采样点（每个像素只压
  一次），去掉重复压点加深与斜线 L 型加粗；测试 45° 斜线正好 8 个像素。

已知取舍：软方形在整数尺寸上 `coverage` 会饱和成实心（没有灰边）；需要柔和方形
时再调整过渡带公式。

### 1.x 画笔扩展（已完成：硬度 / 每笔不透明度 / 预设）

动机是实测出来的两个缺陷：20px「软笔」+ 50% 不透明度画出来是 r≈1（全黑），
因为 (a) 软边只有 1px 抗锯齿、没有硬度参数，(b) 不透明度是「每个 stamp」而不是
「每一笔」，间距 = 直径/4 让同一像素被压 4 次。

- [x] **P1 硬度**（`src/tools/brush/tip.rs`）：`BrushTool.hardness` 0..=1，覆盖率
  从 `半径 × 硬度` 一路线性淡到 `半径 + 0.5`；`hardness = 1` 与旧版软边逐像素相等
  （tip 测试对着旧公式验）。选项栏加「硬度 −/+」（5% 步进）。
- [x] **P2 每笔不透明度**（`src/tools/brush/mask.rs` + `PixelBuffer::blend_from /
  erase_from`）：落笔开一张覆盖率蒙版，重复压到的像素按**最大覆盖率**从落笔前
  快照重算，所以 50% 就是 50%、沿笔迹不变深；两笔相交才是 `1-(1-0.5)²`。
  橡皮对称（`erase_from`）。测试：半透明笔迹整条一致、两笔 75%、蒙版只升不降。
- [x] **P3 预设**（`src/tools/brush/preset.rs`）：`BrushPreset` + 6 个内置预设，
  选项栏「预设 ▾」用 `Overlays::menu` 列出来、当前项打 `✓`、选中回写「像素 /
  方形」开关（否则每帧同步会覆盖回去）。参数不命中任何预设时标签显示「自定义」。
- [x] **模块拆分**：`tools/brush.rs` 868 行按关注点拆成 `brush/mod.rs`（工具 +
  笔触 + undo）、`tip.rs`（笔尖几何）、`mask.rs`（蒙版 + 合成）、`preset.rs`。
- [x] **验证**：`--selfcheck` 新增 `check_brush_presets`（点开下拉 → 帧里有全部预设
  → 点「柔边圆笔」→ 参数 / 开关 / 标签全跟着变 → 硬度 − 生效）；注意覆盖层的内容
  在 `layout` 里才建出来，自检里点完必须再 layout 一帧才能读帧。

已知取舍：不透明度现在按「一笔封顶」，所以低不透明度笔画比以前**明显变淡**（那是
正确的）；没有 flow（笔内累积）与压感 —— 鼠标没有压感源，笔尖也没有纹理格式。

### 1.y 钢笔 / 路径（已完成）

- [x] **几何**（`src/canvas/path.rs`）：`Anchor`（两侧控制柄）+ `Path`（锚点序列 +
  开闭），自适应细分展平成折线；预览按 `0.25 / zoom` 展平、光栅化按 0.25 文档像素。
  测试：直线段只有两端点、曲线不出控制多边形、容差越细点越多、闭合多一段绕回。
- [x] **状态机**（`src/tools/pen.rs`）：单击 = 尖角、拖动 = 对称控制柄、点回第一个
  锚点 = 闭合、`Enter` 收尾 / `Esc` 取消 / `Backspace` 删点，收尾后再点空白开新路径。
- [x] **接线**：`ActiveTool::Pen`（`P`、`pen-tool` 图标、工具栏单独一组）、
  `input.rs` 的悬停转发（不按键也要橡皮筋）与键位优先级、`is_dragging` 带拖柄状态。
- [x] **动作 + 预览**（`ui/pen.rs` / `ui/paint.rs` / 选项栏）：建立选区（偶奇光栅化）、
  填充（`fill_selection`）、描边（`BrushTool::stroke_path`，段内直线、间距/蒙版/undo
  全复用）、清除；预览 = 双色虚线 + 锚点方块 + 控制柄 + 橡皮筋。
- [x] **验证**：`--selfcheck` 新增 `check_pen`（点三下 -> 帧里有锚点/路径线/锚点数
  -> Enter 收尾 -> 点回起点闭合 -> 建立选区面积 ≈3872 -> 描边只加一步 undo 且路径上
  有像素 -> 填充只加一步且有像素 -> 清除后显示「没有路径」）。注意自检的 `click_at`
  **不会**自己 `update()`，点完要手动调一次。

已知取舍：路径不进撤销栈、也不进 `.igui` 工作区（和选区一样）；多子路径的并 / 差 / 交
还没做（自交路径按偶奇规则挖空）；控制柄只有对称的（没有 `Alt` 独立柄、`Ctrl` 拖锚点）；
已收尾的路径不能回点锚点再编辑。

---

## 2. 绘画 / 修图路线图（绘画优先，按批落地）

判断标准：**先补"画不准 / 画不完 / 画不爽"，再补修图**。每条都写清「改哪里 /
怎么验收」；一批做完出一次报告。`[ ]` 未做 / `[~]` 进行中 / `[x]` 完成。

### 第 1 批：绘画手感

- [x] **笔刷光标圈**：画布上跟随指针画当前笔刷的轮廓（屏幕空间、1px、亮/暗双色
  描边保证任何底色可见），像素模式显示方框、软笔显示圆，另加一个中心十字。
  - 改哪里：`src/ui/paint.rs`（新增 `paint_brush_cursor`，排在文档/网格之后、
    选区之前）；尺寸取 `self.brush.size`、形状取 `tip().shape`；只画在
    「画布区域 ∩ 文档范围」内，指针不在画布上时不画。
  - 咋验收：单测断言「鼠标在画布上时帧里多出该尺寸的轮廓、离开后消失、
    尺寸随 `brush.size` 变」；`--selfcheck` 新增 `check_brush_cursor`
    （指针在画布上有圈、移到菜单栏就没有）。实现是 2px 黑底 + 1px 白线 +
    中心十字，圆头 `StrokeCircle` / 像素模式 `StrokeRect`。
- [x] **对称绘画**（关 / 垂直 / 水平 / 四向）：每个 dab 额外在镜像位置压一次。
  - 实现：镜像放在**像素级**——`Symmetry::mirror_pixel`（`x' = width - 1 - x`）
    由 `brush/mask.rs::Writer::put` 对每个落笔像素再做一遍，所以像素画也严格对称、
    越界自动丢掉，Bresenham / 软笔 / 描边路径全部自动跟着对称。
    选项栏「对称」+「切换」按钮循环 关 → 垂直 → 水平 → 四向。
  - 验收：单测（垂直 `x' = w-1-x`、四向 4 个通道、越界丢弃、循环顺序）、
    `Writer` 级对称合成、UI「切换后画笔跟手且两侧落笔」、`--selfcheck` `check_symmetry`。
- [x] **平滑强度可调（含关掉）**：现在固定是「二次中点平滑」，像素画党要笔到手到。
  - 实现：`BrushTool::smooth`（0..=1，默认 1）。平滑 0 时把「起点 / 终点」从
    中点换成锚点本身（`Vec2::lerp`），曲线退化成折线、笔迹不再滞后半个事件段；
    两个端点的定义一致，所以段与段之间仍然连续。选项栏「平滑 −/+」（10% 步进）。
  - 验收：单测「平滑 1：一次 move 后光标处像素还没落笔、抬笔才补完」「平滑 0：
    move 一下立刻落笔、折角保持折角」、UI「− 到底 = 0% / + 到顶 = 100%」、
    `--selfcheck` `check_brush_smooth`。
- [x] **笔刷预设持久化**：自定义参数重启后还在。
  - 实现：`src/io/preferences.rs`（配置目录 `preferences`，`key=value`，坏行跳过 /
    越界夹住）+ `src/ui/preferences.rs`（视图只做 `preferences()` /
    `apply_preferences()` / `take_preferences()` 快照桥，**读写文件是宿主的活**，
    所以测试与 `--selfcheck` 永远不碰用户配置目录）。宿主在装配时
    `load_preferences()`、每帧问一次 `take_preferences()`，变了才写盘。
  - 验收：`io::preferences` 4 条（默认值 = 像素硬笔、round-trip、坏行/越界、
    文件不存在）+ `ui::preferences` 4 条（新视图无可写快照、应用后开关同步、
    改一次只报一次、对称码 round-trip）。

### 第 2 批：选区（绘画的"边界"）

- [x] **全选 / 反选**：`Selection::all` / `Selection::invert`（掩码取反 + 按新掩码
  重算轮廓），菜单「选择」里两项接上真实动作（`MenuAction::{SelectAll, InvertSelection}`），
  快捷键 `Ctrl/Cmd+A` / `Shift+Ctrl/Cmd+I`（纯函数 `selection_shortcut`）。
  验收：单测（全选面积/边框数、反选互补、反选两次掩码还原）、宿主快捷键单测、
  `--selfcheck` `check_selection_menu`（真的点开「选择」菜单点两项）。
- [x] **加选 / 减选**：框选 / 套索 / 魔棒按住 `Shift` 取并、`Alt` 取差。
  - 实现：`Selection::{union, subtract}`（掩码集合运算 → `from_mask` 重算面积 / 包围盒 /
    合并轮廓）+ `canvas::MergeMode { Replace, Add, Subtract }`（`from_modifiers` 里
    Alt 优先，`apply(base, next)` 空结果 → 没有选区）。视图按下时记
    `merge` / `merge_base`，宿主每帧把 `InputEvent::ModifiersChanged` 转发进
    `EditorView::set_modifiers`（`igui_core::Key` 没有 Shift/Alt 变体，视图自己收不到）。
  - 验收：单测（并集 / 差集面积、挖空 → None、尺寸不同不 panic、修饰键映射）；
    `ui::tests::shift_adds_and_alt_subtracts_from_the_selection`（真实拖拽）；
    `--selfcheck::check_selection_merge`（Shift 面积翻倍、Alt 变小）。
- [x] **羽化**：`Selection::feathered(radius)` —— 掩码从 `bool` 升级成 **`u8`
  覆盖率**（0 = 未选中、255 = 完全选中），两遍盒式模糊（越界按 0，和 PS 一样
  贴着画布边缘的那一侧也会向内淡出）。
  - 裁剪升级成覆盖率：`Writer::put` 用 `coverage × clip.coverage()` 衰减落笔、
    `fill_selection` 按覆盖率 `blend_from`（硬选区里就是 0 / 1，行为不变）。
  - 菜单「选择 → 羽化」（半径固定 8px，`FEATHER_RADIUS`；以后接半径输入框就换掉它），
    没选区时置灰。
  - 验收：`canvas::selection` 三条（边缘变淡 / 中心仍满覆盖 / 半径 ≤ 0 是 no-op /
    面积按覆盖率折算）、`tools::brush::mask::a_feathered_selection_fades_the_stamp…`、
    `ui::tests::feathering_softens_the_selection_and_the_fill_follows_it`（填出来的
    边缘是中间色）、`--selfcheck` 在「选择」菜单里点「羽化」并核对覆盖率。

### 第 3 批：图层（合成器）

- [ ] **图层混合模式**：`BlendMode` 从只有 `Normal` 扩到 正片叠底 / 滤色 / 叠加 /
  柔光 / 变亮 / 变暗 / 差值；`.igui` 的 `u8` 判别值只追加不重排。
  - 改哪里：`src/document/layer.rs`（枚举 + `label()`）、`src/renderer/cpu.rs`
    （每个模式的像素公式，先算通道再套 alpha）、属性面板换成可点下拉。
  - 咋验收：单测每个模式在已知两色上的合成结果（和公式对照），以及
    `Normal` 结果不变。
- [ ] **Alpha 锁定（锁定透明像素）**：图层开关，画笔只在 `before.a > 0` 的像素上写。
  - 改哪里：`Layer`（`alpha_locked: bool`，进 `LayerMetaCommand` 与 `.igui`）、
    `src/tools/brush/mask.rs::Writer::put`（锁定且 `base.a == 0` 直接跳过）。
- [ ] **复制图层 / 合并向下**：`LayerCommand` + 图层菜单（"复制图层"占位已存在）。
- [ ] **图层右键菜单**（Phase 11 前置已具备一部分，等 `Overlays` 指针锚点）。

### 第 4 批：变换

- [ ] **水平/垂直翻转、旋转 90°**（整层或整图；菜单占位已存在）：`PixelBuffer::{flip_h,
  flip_v, rotate90}` 无插值 → `PaintCommand::capture` 一条 undo。
- [ ] **图像大小 / 画布裁剪**（菜单占位已存在）：`Document::resize` / `crop`，
  重采样先最近邻，补上双线性。
- [ ] **自由变换（`Ctrl/Cmd+T`）**：选区或整层的平移 / 缩放 / 任意旋转（反向映射
  + 双线性），四角柄 + 预览叠加。分两步：先整层，再选区。

### 第 5 批：修图（先做"滤镜框架"）

- [ ] **滤镜框架**：`apply_to_region(|x, y, color| color)` = 快照 → 改区域内像素 →
  `PaintCommand::capture`（一条 undo）。所有一次性调整都走它。
- [ ] **灰度 / 反相 / 亮度对比度 / 色相饱和度**（"滤镜"菜单占位已存在）。
- [ ] **高斯模糊**（可调半径，区域化；先做可分离的盒式近似）。
- [ ] **裁剪到画布 / 应用图标**：`--selfcheck` 断言"作用区域外的像素不变"。

### 第 6 批：修图进阶（可选）

- [ ] **克隆图章**：采样点 + 复用画笔的间距 / 蒙版 / undo。
- [ ] **减淡 / 加深**：画笔流程 + 提亮/压暗的混合公式（绘画打光也能用）。
- [ ] **涂抹（smudge）**：每个 dab 取上一次的颜色混进去。

### 明确不做

- 压感（winit 鼠标没有压感源）、纹理/图案笔刷（打破"无纹理"约束）、
  液化 / 调整图层 / 图层蒙版系统、画布旋转（要重做全部命中测试）。

### 1.w 选区剪贴板（已完成）

- [x] **Ctrl/Cmd+X / C / V + Delete**：`tools/clipboard.rs::PixelClipboard`
  （按选区包围盒抓一块 RGBA、alpha 乘过覆盖率 → 羽化边缘贴回去也是软的；粘贴
  放回原位置并按**当前选区**再裁一次；`clear_selection` 按覆盖率把 alpha 拉 0）。
  视图侧 `ui/clipboard.rs` 用 `PixelRegion::in_buffer` 算区域、`capture_in` 提交
  **一条** undo；宿主共享一块剪贴板（跨工作区复制粘贴），并接
  `clipboard_shortcut`（Ctrl/Cmd+X/C/V）与 `Delete/Backspace`（钢笔优先，先删它
  自己的锚点）。「编辑」菜单的剪切 / 复制 / 粘贴从占位换成真动作（没选区 / 空
  剪贴板时置灰）。
- 验收：`tools::clipboard` 5 条（裁剪到包围盒 / 羽化 alpha / 原位贴回 / 按选区裁 /
  只清选区）、`ui::clipboard` 4 条（剪切→粘贴 round-trip + 各一步 undo / 删除一步
  undo 且重复删不加 / 空选区空剪贴板是 no-op / 换选区粘贴被裁）、宿主
  `clipboard_shortcuts_cut_copy_and_paste`、UI「编辑菜单真的能剪切复制粘贴」、
  `--selfcheck::check_clipboard`。
- [x] **粘贴为新图层**：`EditorView::paste_as_new_layer`（`AddLayerCommand`，位置 =
  剪贴板原点，一步 undo，不动当前图层），「编辑」菜单「粘贴为新图层」+
  `Shift+Ctrl/Cmd+V`。
- [x] **系统剪贴板（macOS `NSPasteboard`）**：`src/io/clipboard.rs` —— 复制时
  `write_image` 把选区写成 `public.png`（失败只记 debug 日志），粘贴时进程内没有
  内容就去 `read_image` 要一张 PNG（`NSPasteboardTypePNG`），外来图片放选区左上角 /
  文档左上角（`foreign_origin`）。Cargo 加了 `NSPasteboard` / `NSData` feature。
  其它平台是空实现（写返回 `Invalid`、读 `None`）。
- [x] **TIFF 兜底**：`read_image` 先取 `public.png`（项目自己的解码器），没有再取
  `public.tiff` 交给 AppKit 的 `NSBitmapImageRep` 解出来、重新编码成 PNG 走同一条
  解码路径（`decode_pasteboard_image`）。Cargo 加了 `NSBitmapImageRep` +
  `NSImageRep` feature（类型要求两个 feature 同时开）。
  测试：`a_tiff_only_clipboard_image_is_decoded` —— 用 AppKit 把 PNG 转成 TIFF
  造数据，只喂 TIFF 也能读出正确的像素。
- 已知：只问 PNG / TIFF 两种类型（JPEG / GIF only 的 App 读不到，要的话按
  `pasteboard.types()` 遍历 + `NSBitmapImageRep` 逐个试）；内存剪贴板和系统剪贴板
  是两份（复制会同时写，粘贴优先用内存里的）。

## 3. 紧接着

- [x] **CPU 合成成本收口**（`src/renderer/cpu.rs`）：图层缓冲现在可能比文档大，
  之前的慢路会遍历整块缓冲。改成只遍历「落在渲染目标里的那部分」，合成成本
  只跟文档尺寸有关，不再随缓冲增长（`DirtyRegion` 留给文档变大时再说）。
- [~] **棋盘格**：视图菜单加「显示棋盘格」开关（`MenuAction::ToggleCheckerboard`）；
  **屏幕空间恒定大小**仍未做（现在烘在显示纹理里，改成独立叠加改动偏大，暂缓）。
  文件：`src/canvas/checkerboard.rs`、`src/ui/mod.rs`。
- **最近邻只在 wgpu**（**暂不做，canvas 先不管**）：给 `draw_backend_canvas` 加
  `imageSmoothingEnabled` 等价开关。文件：`crates/platform/canvas/igui_backend_canvas`（igui 主仓）。
- [x] **图层缓冲回收**：图层菜单加「裁到文档」（`Document::crop_layer_to_document`）：
  按 `position` 摆好后裁回文档尺寸、`position` 归零，丢掉画布外像素。
- **`--selfcheck` 覆盖**：棋盘格 / 移动映射目前只有单元测试。
- [x] **UI 暴露**：属性面板显示图层 `position` / 缓冲尺寸；视图菜单可开关棋盘格。

### 1.z 布局：菜单栏放到最上（已完成，方案 A）

原来工作区标签条是**宿主**的、钉在窗口最顶 32px，编辑器整页用 `set_top_inset(32)`
让开；所以顺序是「标签条 → 菜单栏 → 选项栏」。现在改成 Photoshop 那种
**菜单栏在最上、工作区标签在它下面**：

- `EditorView`：菜单栏和选项栏之间多了一个 0 高的「标签带」占位行
  （`set_tab_band(h)`，`h = 0` 就折叠）；`menu_bar_rect()` / `options_bar_rect()`
  给宿主和测试量位置。`set_top_inset` 删掉（不再需要页面级偏移）。
- `WorkspaceTabs::set_top(y)`：标签条钉在 `y`，宿主在 `layout()` 里用
  `editor.menu_bar_rect().bottom()` 每帧同步（菜单栏高度是主题算出来的，不写死常量）。
- 验收：`ui::tests` 两条（菜单贴顶 + 标签带正好 32 + 标签条落在带子里；标签带
  为 0 时选项栏紧贴菜单）+ 点击穿透测试、`--selfcheck::check_chrome_order`
  （菜单 top ≈ 0 → 标签条贴菜单下沿 → 不压选项栏）。
- [x] **菜单弹窗被标签条挡住**：绘制顺序改成「标签条 → 编辑器 → 浮层」
  （`application::paint_screen`，之前是「编辑器 → 标签条」，菜单下拉跨过标签带那一条
  时会被切掉）。测试：宿主单测比对命令流里「主页」（标签条）与「导入 PNG…」（菜单项）
  的先后，故意换回旧顺序会失败。
- **可选方案 B（保留）**：把菜单栏提到宿主级 chrome（主页也有菜单、菜单与工作区
  解耦）。代价：`MenuAction` 的十二个动作现在实现在 `EditorView::apply_menu_action`
  （依赖文档 / 历史 / 选区 / 画笔），要改成「宿主转发给当前编辑器 + 无编辑器时置灰」，
  锚点节点与点标题开关菜单的命中测试跟着搬。真需要主页菜单时再做。

## 性能：实测与优化计划（4K 很卡的调查）

用一次性探针实测（已删除），对象是 128² / 1080p / 4K 三种文档，单位为「一次操作」。
关键结论：**画笔本身很便宜，贵的是「每次脏帧都把整张文档重算 + 整块上传」和
「抬笔时整层 diff」**，而 debug 构建把它们放大了 20–90 倍。

### 实测（3840×2160，三档构建）

| 操作（4K） | dev 默认（opt0） | dev `opt-level=2` | release |
|---|---|---|---|
| 每帧「合成 + 棋盘格 + 克隆」 | **443 ms** | **10.5 ms** | 5.8 ms |
| 合成本身 | 275 ms | 7.9 ms | 3.1 ms |
| 棋盘格（`paint_backdrop`） | 169 ms | 3.1 ms | 3.5 ms |
| 缓冲区克隆（`take_texture_upload`） | 1.3 ms | 0.8 ms | 0.8 ms |
| 落笔（快照 33MB + 蒙版 8.3MB） | 2.2 ms | 2.1 ms | 2.3 ms |
| 抬笔（`PixelRegion::diff` 整层） | **230 ms** | **3.5 ms** | 3.1 ms |
| 60 个 move（40px 软笔） | 5.2 ms | 0.57 ms | 0.48 ms |
| `update()` / `paint()`（命令列表） | 36 µs / 0.9 ms | 10 µs / 0.2 ms | 5 µs / 0.2 ms |
| `Selection::all(4K)` | **190 ms** | **15.6 ms** | 15.3 ms |
| `Selection::area()` | 44 ms | 2.1 ms | 0.4 ms |
| 全选后的蚂蚁线（每帧） | 12 371 条 `Line` / 0.6 ms | 同 | 同 |

1080p 同口径：每帧 111 ms → 3.9 ms（dev opt2）→ 2.4 ms（release）；抬笔 58 ms → 1.4 ms。
128² 一档本来就快（dev opt0 每帧 1.2 ms），所以小画布感觉不到。

### 已完成

- [x] **dev 构建开优化**（`Cargo.toml`：`[profile.dev] opt-level = 2`，
  依赖 `opt-level = 3`）。只改一行，4K 每帧 443 ms → 10.5 ms（42×），抬笔
  230 ms → 3.5 ms（65×），而且保留 `debug_assertions` / 溢出检查 / 行号表。
  `cargo run` 默认就是这档，所以「很卡」的主观感受立刻改善。

### 显示毛刺（已修）

现象：高分辨率文档里笔迹看起来「毛刺」很重。实测排除了画笔引擎 —— 4K、200px 软笔
斜着画一条，上边缘相对直线最大偏差 1.8px、线宽无断列（`--ignored` 探针量过，已删）。
真正的原因是**显示采样**：文档纹理一直用 `TextureFilter::Nearest`，而 4K 适配窗口是
≈0.18 倍，最近邻缩小等于丢像素，细笔迹在屏幕上断成虚线/锯齿。

- [x] `canvas::ImageSmoothing::for_zoom`：倍率 ≥ 1 用最近邻、否则线性；宿主每帧把它
  翻成 `WgpuBackend::set_texture_filter`（变了才调），并把 `scale_factor` 同步给视图。
- [x] 最近邻时把文档矩形对齐到设备像素网格（`scale_factor` 的倒数），消掉 1:1 缩放
  时 1px / 2px 交替的锯齿。
- 后续（像素画手感）：缩放吸附到整数倍（1/2/3/4/6/8…），这样非整数倍率的「有的像素
  5 个点、有的 6 个点」也一起解决。

### 待做（按收益排序）

- [x] **P0 脏矩形合成**：`EditorView` 记一个 `dirty: Option<PixelRegion>`
  （每次 dab 并自己的 bbox；换图层 / 换文档 / 撤销 → 全图），
  `CpuRenderer::render_region(document, target, region)` 只合成那一块，
  棋盘格也只刷脏区。4K 笔触每帧从 10.5 ms → 预估 < 1 ms（release 5.8 → ~0.3 ms）。
  验收：单测「合成结果与整图合成逐像素相同」「脏区外像素不变」。
- [x] **P1 抬笔不再整层 diff**：`Writer` 里增量维护 bbox（O(1)），
  抬笔用 `PaintCommand::capture_in(bbox, before, after)`；`before` 快照留到
  有第一笔时再按需裁剪。4K 抬笔 3.5 ms → 预估 < 0.2 ms，历史内存从 33 MB/笔
  降到「笔迹 bbox」。
  验收：单测「整层 diff 与 bbox 记录的撤销区域一致」「撤销/重做像素不变」。
- [x] **P2 选区不再扫整张掩码**：`Selection::{all, rect}` 直接给矩形轮廓（4 条边，
  不扫 mask）；`area` / `bounds` 构造时缓存一次（`is_empty` 复用）；
  `mask_edges` 改成按行 / 列 run 合并。实测（4K）：`Selection::all` 15.6 → **0.11 ms**，
  `area()` 2.1 ms → **O(1)**；蚂蚁线 12 371 → **819 条**命令。
- [ ] **P3 上传**：`WgpuBackend::update_texture` 只能整张写（origin 0、extent 全图），
  部分上传要上游加 `update_texture_region`（记进 `docs/upstream.md`）。
  在上游补齐之前：合成/上传节流到 ~30 Hz（画笔的 move 仍然每帧处理），
  以及 `take_texture_upload` 不再整块 `clone()`（改成借用，测试里再 clone）。
- [ ] **P4 内存 / 历史上限**：`History` 加步数 + 字节上限（4K 一笔 33 MB 快照，
  P1 之后按 bbox 存），`Document` 增加「图层缓冲上限」提示。

## 结构改进（持续）

`src/ui/mod.rs` 从 4245 行拆到 530 行（纯移动 / 低风险重构，门禁全程保持绿）：

- [x] **拆 `ui/mod.rs`**：`tests` / `paint` / `accessors` / `tools` / `sync` /
  `menus` / `input` / `layout` / `io` 各成 `src/ui/*.rs`。
- [x] **拆 `EditorView::new`**：构造拆到 `ui/build/{mod,tree,assemble}.rs`，
  `Wiring` 串起共享状态槽与 NodeRef 收集器。
- [x] **拆 `app/application.rs`**：1020 -> 522 行代码，拆到
  `app/application/{mod,files,shortcuts,plugins}.rs`。
- [x] **自动化门禁**：`.github/workflows/ci.yml`（fmt / `clippy -D warnings` /
  test / `--selfcheck`）；`cargo clippy` 已归零。
- [x] **修正 MSRV / 打包**：`rust-version` 1.76 -> 1.80（`LazyLock`）；
  加 `[profile.release]`（thin LTO / 单 codegen unit / strip）。
- [ ] **拆 `selfcheck::check()`（约 500 行）**：各段串行共享 `view` 状态，
  拆成吃 `&mut EditorView` 的 `check_*` 时要注意顺序依赖。
- [ ] **拆 `ui/new_document.rs`（508 行代码）/ `document/history.rs`（485）**。
- [ ] **测试辅助**：`viewport()` 各处尺寸不同（有意），`click()` 里只有
  `toolbar.rs` / `gradient_editor.rs` 的 `SceneTree` 版完全重复（其余接收类型
  不同）；要收敛就抽一个 `testing::click_tree`，别为 `viewport` 造共享函数。
- [ ] **`docs/upstream.md`**：集中记「需要改 igui 才能做」的事（目前两件：
  `igui_winit` 的 `CloseRequested` 直接 `event_loop.exit()`，没有宿主钩子；
  `Overlays::confirm` / `modal` 固定两个页脚按钮，三选一（保存 / 不保存 /
  取消）的未保存确认框套不上，要一个自定义页脚的 modal）与 tag 升级步骤。

### 可扩展

- [x] **魔棒**（`W`）：`tools::fill::magic_wand_region` 复用洪水填充的容差扫描，
  返回文档掩码 → `Selection::from_mask`；与框选 / 套索同一组（选区组），
  容差 / 「连续」与油漆桶共用，`Esc` 清空。
  `Selection` 的轮廓改成**线段列表**（矩形 4 段 / 套索 N 段 / 魔棒是掩码边界边），
  描边仍由 `ui/selection.rs` 画马赛克。
- [ ] **图层混合模式**：`BlendMode` 目前只有 `Normal`（`document/layer.rs` TODO v1.1）。
- [ ] **继续接上游已有组件**（都在当前 pin 的 v0.3.0 里，只是当初没接）：
  `Overlays::confirm` 换掉自造 `ui/confirm.rs`（169 行 + 同帧关闭补丁）、
  `Overlays::tips` 给工具栏 / 选项栏加悬停 tooltip、`Overlays::message_tone`
  做保存 / 导出 toast（需给 `EditorView::update` 传 `dt` 并调 `overlays.update`）、
  `List::on_context` + `menu_at` 做图层右键菜单、`ScrollView` 包住属性面板 /
  首页、`Select` 收窄选项栏、新建文档尺寸可直接键入（`TextInput`）。
- [ ] **合成只刷脏区**：`CpuRenderer` 现在是整张文档重合成。实测（release）
  512² 0.5ms / 1080p 4ms / 4K 12.7ms，画笔每帧都要合成一次 —— 加了高分辨率预设
  （4K / A4 300dpi）之后这条变重要：按笔迹包围盒（脏矩形）只合成那一块，
  4K 的笔触就能回到 ~1ms 量级。
- [ ] **`History` 上限**：目前无步数 / 字节上限，大文档多步编辑会一直涨。
- [ ] **工具元数据表**：新增一个工具要改 7+ 处（枚举 / `ALL` / 标签 / 图标 /
  `tool_groups` / `begin..end` 的 match / 快捷键）；可收成一张 `ToolSpec` 表。

## 4. 计划阶段（Phase 10+）

复用 `igui_components::Overlays` 与 `examples/file_browser` 的工作线程 /
`EventLoopProxy` 模式：

- [x] **Phase 10 — 文件选择（已改用原生对话框）**：原计划用 `List` 覆盖层自绘
  文件浏览器；实际改成 `io/dialog.rs` 的**手写平台 FFI** 原生对话框（macOS
  `NSOpenPanel`/`NSSavePanel`、Windows `comdlg32`、其它 Unix `zenity`/`kdialog`），
  文件面板按钮只发意图、宿主解析路径。见 Phase 23。
- [ ] **Phase 11 — 图层右键菜单**：图层行右键在指针处弹出上下文菜单（重命名 /
  复制 / 删除 / 显示隐藏 / 上移下移 / 向下合并 / 不透明度）。两个前置（增量）：
  `Overlays` 支持**原始矩形 / 指针位置**锚点；`igui_ui` 增加右键回调
  （`PointerButton::Right` 已存在，但 `handle_input` 目前只处理左键）。

### 选区与填充（Phase 12–16，已完成）

前置：现在的选区是 `AppState.selection: Option<PixelRegion>`（只能是矩形），
自由选区 / 按选区填充都需要**任意形状**的选区。

- [x] **Phase 12 — 选区模型（任意区域）**：新增 `src/canvas/selection.rs` 的
  `Selection`：`width/height` + `Vec<bool>` 像素掩码 + `outline: Vec<Vec2>`
  （文档坐标，描边用）；构造 `Selection::rect(PixelRegion, size)` /
  `Selection::polygon(points, size)`（扫描线光栅化，偶奇规则）；查询
  `contains` / `bounds` / `outline` / `is_empty`。`AppState.selection` 改成
  `Option<Rc<Selection>>`，画笔的 `clip` 也改成 `Option<Rc<Selection>>`
  （共享，克隆便宜）。`ui/selection.rs` 的描边改成吃 `outline`（矩形 = 4 点，
  套索 = 折线），边框跟着路径走。验收：矩形 `contains` 与
  `PixelRegion::contains` 一致；三角 / 凹多边形掩码包含正确像素；矩形 outline
  是 4 点；现有框选 / 画笔裁剪 / Esc 全部回归。
- [x] **Phase 13 — 自由选区（套索）**：`ActiveTool::LassoSelect`，快捷键 `L`，
  图标 `lasso-select`。`EditorView` 拖动时收集文档坐标折线，抬笔
  `Selection::polygon`；拖动中实时画折线（复用 marching ants）。验收：套索三角
  内部被选中、外部不选；画笔只在套索内落笔；Esc 清空；描边跟着折线。
- [x] **Phase 14 — 填充引擎 + 油漆桶**：`src/tools/fill.rs` 的
  `flood_fill(buffer, seed, tolerance, clip) -> Option<PixelRegion>`（连续扫描线；
  可选全局填充）。`PaintBucketTool`：颜色 = 前景色、容差、连续 / 全局。落笔前
  快照整层、抬笔 `PixelRegion::diff` → 一条 undo。选项栏加「容差 −/+」「连续」
  开关；工具栏 `ActiveTool::PaintBucket`，快捷键 `G`，图标 `paint-bucket`。
  验收：在封闭区域里填满、不越过边界；容差影响匹配范围；有选区时只填选区内；
  连续 / 全局行为不同；一次填充 = 一步 undo。
- [x] **Phase 15 — 矩形填充**：`src/tools/shape_fill.rs` 拖动矩形，抬笔用前景色
  填充（按选区裁剪）；拖动中画预览（虚线框 + 半透明填充）。工具栏
  `ActiveTool::RectangleFill`，快捷键 `R`，图标 `square`。验收：拖出的矩形正好
  被填、选区外不填、预览在拖动中出现、一步 undo。
- [x] **Phase 16 — 自由填充**（待确认语义）：自由套索填充 —— 拖动自由折线，
  抬笔把它围成的区域立刻用前景色填充（不留下持久选区）。验收：凹多边形围成的
  区域被填、一步 undo。
- [ ] **图标**：往 `assets/icons/` 补 `lasso-select` / `paint-bucket` / `square`
  （Lucide ISC），否则新工具没有图标（按钮仍在，只是空图标）。

## Done（近期）

- **图层重命名改用上游 `TextInput` 浮层**（`ui/rename.rs`）：原来是在状态栏里
  手写按键缓冲（`InputEvent::TextInput` + `Backspace`，没光标 / 选区 / 剪贴板 /
  IME），现在是 `Overlays::popover(Placement::Above)` 里一个上游 `TextInput`
  （`autofocus`，自带光标 / 选区 / Ctrl+C/V / 中文 IME preedit），Enter / 「确定」
  提交、Esc / 点外部取消；删掉 `ui/sync.rs` 的 `handle_rename_input` /
  `sync_rename_status`，顺带把系统剪贴板接进视图（`EditorView::set_clipboard`）。

- **多工作区 + `.igui` 工作区文件 + 主页画廊**（Phase 18–22）：`io/workspace.rs`
  自包含二进制容器（多图层 + PNG 像素，零新依赖）；`io/recent.rs` 配置目录下的
  最近列表；`app/workspaces.rs` 的 `Workspaces` + `ui/workspace_tabs.rs` 顶部标签条
  （切换 / 关闭 / 新建 / 主页，脏标记 `●`）；「文件」面板加「保存工作区」/
  「打开工作区」；主页画廊列出打开中 / 最近工作区（名字 / 尺寸 / 图层数）。
- **渐变工具（多节点 ramp）**（`document/gradient.rs` + `tools/gradient.rs` +
  `ui/gradient_editor.rs`）：`Gradient` 是一串色标（position + Color，≥2，按位置
  排序），采样定位区间 + RGBA 插值；默认端点来自前景/背景，未自定义时每帧跟随，
  改过节点就冻结。五种类型（线性 / 对称 / 径向 / 角度 / 菱形）；拖一条轴实时改
  图层预览、松手一条 undo；选项栏切类型 / 「反向」/「编辑…」，编辑器是色带 +
  可拖动节点 + 「前景色/背景色/删除/重置」。快捷键 `N`，并进油漆桶组。
- **工具栏分组 + 长按展开**（`app/tool_groups.rs` + `ui/toolbar.rs`）：9 个工具
  收成 7 个组按钮（移动 / 画笔 / 橡皮 / 选区 / 油漆桶 / 填充形状 / 吸管），
  组按钮画当前「脸」的
  图标 + 标签（图标用 `foreground` 动态画）+ 右下小三角；短按激活脸，长按
  （350ms / 6px 容差）用 `Overlays::popover(Placement::Right)` 展开浮层选工具；
  快捷键 / 浮层选中的工具会更新所属组的脸。等阈值期间 `EditorView::needs_frame`
  为 true，宿主持续出帧。
- **画布内容裁剪 + 去重复绘制**：v0.3.0 单树模型下 `igui_ui::paint` 会再画一遍世界
  canvas item，文档原来挂成场景 `Node2D`，第二遍落在所有 UI 之上。改成
  `EditorView::paint` 先 `clip_rect(canvas_area)`，手动按相机画文档图像 + 像素
  网格 + 选区，再 `igui_ui::paint` 画 UI；删掉 `document_node`/
  `sync_document_node`/`CanvasCamera::transform`。测试断言文档图像只画一次、
  紧跟在画布区域裁剪之后。
- **选区描边改成「马赛克线段」**（`ui/selection.rs`）：之前用半透明的
  `palette.selection` 画 1px 线，浅色画布上几乎看不见；改成沿选区四条边铺
  黑白相间的等长线段（5px 段 / 2px 宽），亮底 / 暗底都看得清。测试断言黑白段
  都存在、线段总长等于周长。
- **「新建文档」改成模拟桌面的浮动窗口**（`src/ui/window/` 的 `Window`）：
  自带 `SceneTree`，标题栏拖动移动、四条边 / 角拖动缩放（`RESIZE_BORDER` 命中带，
  钳到 `min_size` 与视口），右上关闭按钮悬停变红，遮罩 + Esc / 点窗口外取消，
  模态吞掉全部输入；`NewDocumentModal` 把尺寸 / 背景控件与「取消 / 创建」行
  构建进去。单测覆盖居中 / 拖动 / 缩放 / 最小尺寸 / 点按钮 / 点外 / Esc /
  关闭按钮。
- **项目内 Card + 调色盘 + 历史面板**：`ui/card.rs` 统一侧栏卡片外观
  （surface + 内边距 + 行间距，无边框 / 圆角），文件 / 图层 / 属性 / 历史和调色盘
  都用它；左侧独立调色盘面板（`ui/palette.rs`，宽度可拖；**HSV 取色器** ——
  饱和/明度方块 + 色相条，用 `Component::on_pointer` 映射指针 —— 加前景 / 背景与
  16 个预设色块）；右侧栏「历史」面板（虚拟化 `List`，`History::undo_labels`/
  `redo_labels`，点一步撤 / 重做到那里，`ui/history_panel.rs`）。
- **撤销补全 + 右侧栏可调**：移动工具（`SetLayerPositionCommand`）、新建 / 删除图层
  （`AddLayerCommand`/`RemoveLayerCommand`）、重命名 / 可见性 / 不透明度 / 排序
  （`LayerMetaCommand`，不克隆像素）、裁到文档（`CropLayerCommand`）全部入历史；
  标签页面板与「图层」之间加 `ResizeHandle::horizontal` 分隔条（可拖高度，
  `clamp_panel_heights` 保证图层不被挤没）。
- **侧栏标签页（`ui/tabs.rs`）**：文件 / 历史 / 属性三块内容收进一个卡片，
  上面 `TabsView` 的标签按钮（选中态底色 + accent 下划线），下面只显示当前页；
  激活态是 `Rc<Cell<SidebarTab>>`，`EditorView` 在激活页变化时调
  `tabs::show_active`（`set_visible` + `mark_dirty`，可见性变化不会自动让布局
  失效，不标脏内容会停在 `(0, 0)`），所以没打开的标签不参与布局 / 绘制
  （历史 `List` 也不建行池）。`ui/tabs.rs` 的单元测试覆盖只有一页可见与
  「隐藏后再露出会重新排布」，`--selfcheck` 覆盖真实点击切换。
- **棋盘格开关**：视图菜单「显示棋盘格」（可关，导出不受影响）。
- **图层菜单「裁到文档」**：`Document::crop_layer_to_document` 回收被移动撑大的缓冲
  （`cropping_a_layer_to_the_document_drops_off_canvas_pixels`）。
- **属性面板显示图层几何**：`偏移 (x, y) · 缓冲 W×H`（`the_properties_panel_shows_layer_geometry`）。
- **CPU 合成只遍历可见范围**（`src/renderer/cpu.rs`）：合成成本不再随图层缓冲尺寸
  增长（`a_layer_larger_than_the_document_composites_only_the_visible_part`）。
- **像素模式 P1–P4**：`BrushShape { Round, Square }` + `BrushTool { hard, shape }`
  （默认硬边方形，任意尺寸圆心吸附像素网格、`coverage` 二值）；选项栏加「像素」
  「方形」两个开关（`Rc<Cell>` + `dynamic_background`，`update` 同步进画笔）；
  像素模式 + `zoom >= 6` 画屏幕空间像素网格；1px 硬边笔走 Bresenham（无重复压点 /
  斜线加粗）。`--selfcheck` 验证硬边无灰边 / 软边有灰边。
- 透明棋盘格背景（显示用合成，导出 PNG 仍保留 alpha）。
- 默认 128×128 像素画布 + 1px 硬边笔 + `TextureFilter::Nearest`。
- 移动图层后画笔对准光标；图层缓冲按「当前范围 ∪ 文档范围」扩展，
  **画布外像素不裁掉**，历史命令区域同步平移。
