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
正确的）；没有 flow（笔内累积）与压感 —— 鼠标没有压感源。

**笔画系统只保留基础印章笔**（纹理 / 颗粒 / 散布 / 素材目录 / 预设文件 / 定义画笔
预设 / 湿画 全部移除，2026-02）：`BrushTool` 只剩 直径 / 不透明度 / 硬度 / 像素·
方形 / 对称 / 平滑 / 间距，预设退化成 6 条纯参数组合（`preset::PRESETS`），
`preferences` 只存这些外观字段。要恢复高级笔刷得先重新讨论需求。

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

### 1.aa 默认使用原生菜单栏（已完成）

目标：macOS 上**默认**把「文件 / 编辑 / 图像 / 图层 / 选择 / 滤镜 / 视图 / 帮助」放进
系统菜单栏（`NSApp.mainMenu`），窗口内不再画那一行；无头（`--selfcheck` / `--dump`）
与非 macOS 继续用自绘菜单。这是「UI 全部由 igui 绘制」的一个明确例外（系统菜单栏
不在窗口里），README 的架构段要补一句。

- **开关**：`Options { native_menu: bool }` 默认 `true`，`--ui-menu` 关掉切回自绘；
  宿主据此调 `EditorView::set_menu_bar_visible(false)`。
- **一张菜单表**（`ui/menu.rs`）：抽出 `MenuSpec` / `MenuEntry::{Item { label,
  shortcut, action, enabled, checked }, Separator}`，`menu_content` 与原生菜单都从它
  生成，杜绝两套菜单漂移。
- **平台层**（新 `src/app/native_menu/{mod.rs, macos.rs}`）：手写 objc2-app-kit 的
  `NSMenu` / `NSMenuItem`（和 `io/dialog/macos.rs` 一样的风格，**不引 `muda` /
  `tray-icon`**）；在 `AppLogic::init` 之后、主线程上 `setMainMenu`；App 菜单补
  Quit / Hide / Hide Others / Show All。`NSMenuItem` 的 target/action 只往
  `Rc<RefCell<Vec<MenuAction>>>` 塞动作，宿主每帧 `update` 取走再转发给当前编辑器
  （无工作区时置灰 —— 代价同 §1.z 方案 B：`apply_menu_action` 要能被宿主转发）。
- **状态同步**：`MenuState`（can_undo / can_redo / has_selection / can_paste）+ 棋盘格
  勾选，签名变了才 `setEnabled` / `setState`（不每帧重建 NSMenu）。
- **快捷键先留在 winit**：原生项不设 key equivalent，继续走 `shortcuts.rs` ——
  `Cmd+A` 不能被菜单抢走（重命名输入框里的全选会失效）；只有 App 菜单的
  Quit / Hide 例外。
- **布局**：`set_menu_bar_visible(false)` 折叠画菜单的那一行（`set_tab_band` 同一套路），
  `menu_bar_rect()` 返回 `None`，宿主把标签条钉到 `y = 0`。
- **验收**：单测「菜单表 -> 自绘项口径一致（标签 / 禁用 / 勾选）」、
  「隐藏菜单栏后 `menu_bar_rect() == None` + 选项栏贴顶」、
  「标签条落在 `y = 0`」；`--selfcheck::check_chrome_order` 加原生变体；真机点一次
  系统菜单项，用 `tracing` 打点确认走到了 `apply_menu_action`（不截图）。

落地（`cargo test` 471 条 + `--selfcheck` 全绿）：

- `ui/menu.rs`：`MenuEntry` / `MenuState`（多了 `has_document` / `checkerboard`）+
  `menu_spec(index, state)`；`menu_content` 只是把这张表画成下拉（勾选项前缀 `✓ `）。
  单测：每个菜单非空、禁用态跟 `MenuState` 走、没有文档时整张置灰、棋盘格勾选。
- `src/app/native_menu/{mod,macos,other}.rs`：`MenuTarget`（`define_class!`）接
  `menuAction:`，`tag` -> 动作表 -> 静态队列；`setAutoenablesItems(false)`，每帧只在
  `MenuState` 变了才 `setEnabled` / `setState`。
- 宿主：`Options::native_menu`（默认 `true`，`--ui-menu` 关）、`AppLogic::init` 里装、
  `update` 里先取动作再同步状态；`workspace.rs::configure_editor` 与 `init` 折叠自绘
  那一行；`layout` 用 `menu_bar_rect().map_or(0.0, …)` 钉标签条。
- 验证：`app/application/menu.rs` 的转发有两条宿主单测（有工作区时动作生效、
  没工作区时丢掉且不改标题）；`ui/tests/chrome.rs` 一条折叠布局；真实运行
  `cargo run -- --frames 20` 确认装上（8 个下拉 / 41 项 / 顶层 9），并临时用
  `performActionForItemAtIndex` 验过「启用项 -> target -> 队列」这条链。
- **叫醒事件循环**（修「点了菜单要动一下鼠标才刷新」）：`igui_winit` 跑
  `ControlFlow::Wait`，只在 winit 事件里 `request_redraw`；系统菜单的点击发生在
  AppKit 的 run loop 里，不产生 winit 事件。现在宿主把
  `window.request_redraw()` 的闭包（[`native_menu::Wake`]）交给平台层，`menuAction:`
  入队后立刻调一次，下一帧就取动作并重画。临时探针确认回调里确实调到了宿主闭包。
- 已知：真机用手点系统菜单项没做（没有截图 / 辅助功能自动化可用，只验到上面那条
  链路）；原生项不显示 ⌘ 提示（快捷键仍归 `shortcuts.rs`，见上）。

### 1.ab 菜单栏能力补齐（计划）

现在 `ui/menu.rs` 的 `menu_spec` 里还有 12 个占位项（`MenuAction::Placeholder`），
按「便宜 -> 贵」分四批做，每批一个 commit + 一次验收：

1. **文件 / 编辑（几乎不用新领域代码）**
   - 文件：`保存` / `另存为…` / `关闭工作区` / `最近打开`（子菜单或平铺前 5 条）。
     全部复用现有 `Intent::File(FileRequest::*)` 与宿主 `handle_file_requests`；
     没有路径时「保存」等同另存为（宿主已这样实现）。`最近打开` 读
     `io::load_recent()`（主页已经在用）。
   - 编辑：`首选项…`（打开 §1.ad 的设置窗口）、`键盘快捷键…`（§1.ad）。
   - 改哪里：`ui/menu.rs` 的 `file_menu` / `edit_menu` + `ui/menus.rs::apply_menu_action`
     （`MenuAction` 加 `Save` / `SaveAs` / `CloseWorkspace` / `OpenRecent(i)` /
     `OpenSettings` / `OpenShortcuts`）。宿主转发本来就是通的（§1.aa）。
   - 验收：单测「占位项数量降到 0 之后 `menu_spec` 里没有 Placeholder」；宿主单测
     「菜单『保存』真的写出文件」（假对话框，和现有保存测试同一套夹具）。
2. **图层（命令层已有大半）**
   - `新建图层` / `复制图层` / `删除图层` / `上移一层` / `下移一层` / `向下合并` /
     `不透明度`（最后一条要输入框，先给 25/50/75/100 四档子菜单）。
   - 改哪里：`document/history/layers.rs` 补 `DuplicateLayer` / `MergeDown`，
     `PixelBuffer::blend_over` 做合并；`ui/menus.rs` 接 `Manager` 的现有命令。
   - 验收：单测「复制图层不改像素且可一步撤销」「向下合并后像素 == 两层合成」
     「上移/下移不越过首尾」。
3. **图像（要新领域 API）**
   - `图像大小…` / `裁剪` / `水平翻转` / `垂直翻转` / `旋转 90°`。
   - 改哪里：`document/mod.rs` 加 `Document::resize`（最近邻缩放，尺寸输入复用
     §1.ad 的窗口或新建文档表单）/ `crop(rect)`；`document/pixel_buffer.rs` 加
     `flip_h` / `flip_v` / `rotate90`（都是 `PixelBuffer -> PixelBuffer`，纯函数最好测）。
     每个动作一条 `PaintCommand`（整层替换）或图层结构命令，撤销一步到位。
   - 验收：单测逐像素（翻转两次回到原图、旋转四次回到原图、crop 后尺寸与像素、
     resize 后角点颜色）；`--selfcheck` 走一次「翻转 -> 撤销」。
4. **滤镜（先搭框架，再做三个）**
   - `滤镜/灰度` / `反相` / `高斯模糊…`（半径取固定几档）。
   - 改哪里：新 `src/filters/`（`mod.rs` = `Trait`? 不引依赖，纯 `PixelBuffer` 变换：
     `grayscale` / `invert` / `blur(radius)`，可分离盒式模糊近似高斯）；
     `ui/menus.rs` 包成 `PaintCommand`（只在选区内生效，`Selection` 已有掩码）。
   - 验收：单测灰度系数 / 反相两次复原 / 模糊后总亮度守恒、边缘不越界；
     大图耗时记一条（4K 一次模糊的目标值）。
5. **原生菜单的 ⌘ 提示（最后做）**
   - 现在原生菜单项不设 key equivalent（`Cmd+A` 会被菜单抢走，重命名输入框全选失效，
     见 §1.aa）。做法：把快捷键字符串变成**菜单表的一列**（`MenuEntry::shortcut`
     已有），`native_menu` 按它 `setKeyEquivalent` + modifier mask，然后从
     `app/application/shortcuts.rs` 删掉重复处理的键，只留非菜单键（工具字母、
     `[` `]`、`+`/`-` 等）。
   - 验收：单测「每个带 key equivalent 的菜单项在 `shortcuts.rs` 里不再单独处理」
     （同一张表比对）；真机在重命名输入框里按 `Cmd+A` 只选文本。

### 1.ac 工具选项栏：高度一致（计划）

实测（`EditorView::options_bar_rect()`，深色主题，1280×800）：

| 工具 | 高度 |
|---|---|
| 移动 / 框选 / 套索 / 矩形填充 / 自由填充 / 吸管 | 24.25 |
| 画笔 / 橡皮 / 魔棒 / 油漆桶 / 钢笔 | 32 |
| 渐变 | 40 |

所以切工具时**画布整体上下跳 16px**（选项栏 top 恒为 32，bottom 24.25~72）。

- 改哪里：`ui/options_bar/mod.rs::options_bar` 给整行和每个 `*_row` 定死
  `min_size.height = OPTIONS_BAR_HEIGHT`（取 `theme.row_height()`，约 32）+
  `Align::Center`；渐变行的色带预览压到 ~20px（它现在是 40 的元凶）；提示文字用
  单行 `Text::caption` 不换行。
- 验收：单测「切遍 10 个工具，`options_bar_rect()` 高度都等于 `OPTIONS_BAR_HEIGHT`，
  且 `bottom()` 不变」；`--selfcheck` 里同一断言（切工具的循环与 §1.ac 的探针同款）。

### 1.ad 模拟窗口：设置 / 关于 / 快捷键（计划）

`ui/window/` 的 `Window`（模拟桌面的浮动窗口）已经被 `ui/confirm.rs`、`ui/new_document`
用着。把「标题 + 正文 + 页脚按钮 + Esc / 关闭 = 取消」抽成 `ui/dialog.rs` 的
`Dialog` 壳，三个新窗口都建在它上面，宿主把 `modal: Option<Dialog>` 换成一个枚举
（新建文档 / 设置 / 关于 / 快捷键），输入全吞、Esc 关、同一时刻只开一个。

- **设置**（`ui/settings.rs`）：编辑 `io::EditorPreferences`（笔刷默认值
  size/opacity/hardness/smooth/pixel/square/symmetry/spacing、`record_timeline`），
  保存即 `save_preferences` 并套到当前工作区；「菜单栏用系统菜单栏（重启生效）」
  这类开关只读显示。
  验收：单测「改一个值 -> 写回配置文件 -> 重新读出一致」；模态开着时系统菜单栏
  整张置灰（已有 `native_menu_state` 测试覆盖）。
- **关于**（`ui/about.rs`）：版本（`CARGO_PKG_VERSION`）、igui tag、字体（随包
  老宋体 / 系统字体）、当前文档尺寸 / 图层数、渲染后端。数据从 `EditorView` 与
  `fonts::bundled_face()` 现取，不写死。
  验收：单测「文本里出现版本号与当前文档尺寸」。
- **快捷键**（`ui/shortcuts.rs`）：一张 `ScrollView` 表，数据来自
  `app/application/shortcuts.rs` + `app/tool_groups.rs` + 菜单表的 shortcut 列 ——
  **一份数据两处显示**（菜单 / 这个窗口），不手抄。
  验收：单测「表里的每一条在菜单表或工具表里能找到」（反向也查一遍）。
- 入口：「帮助 → 关于 image_editor」「帮助 → 快捷键…」「编辑 → 首选项…」，
  原生菜单走同一批 `MenuAction`（§1.aa 的转发已通）。
- **待确认**：「工具属性栏另外给一个单独的模拟窗口」两种读法 ——
  (A) 选项栏里的编辑类子面板（渐变编辑器、笔刷预设表）从气泡改成独立浮窗；
  (B) 整条选项栏可以脱离顶部、变成可拖动的浮窗（Photoshop 的「工具选项」面板）。
  计划先按 (A) 落，`(B)` 只留接口（`Window` 已经支持拖动 / 缩放）。

### 1.ae 现有能力梳理（计划）

三件事，避免 README / todo / 应用内说法三份漂移：

1. **一张能力表**（`README.md` 新节「能力矩阵」）：按 工具 / 文件 / 编辑 / 图像 /
   图层 / 选择 / 滤镜 / 视图 / 面板 / 快捷键 十行，每项标 `可用` / `占位` /
   `未做`。来源：`ui/menu.rs::menu_spec`（菜单能点到的）+ `app/tool_groups.rs`
   （工具）+ `app/application/shortcuts.rs`（键）；新增菜单项时这张表跟着改。
2. **清 todo**：把已完成但还写着「[ ]」的条目核对一遍（§1.ab 落地后占位项归零，
   §2 的绘画线也要同步），过时条目删掉而不是留着「已完成」。
3. **应用内可见**：关于 / 快捷键两个窗口（§1.ad）读同一批数据；状态栏的占位
   提示句改成「该功能见 帮助 → 快捷键 / 能力」这种指向，而不是各写一句。
   验收：单测「`menu_spec` 里的占位项数量」当作一个数字断言住（只减不增），
   README 的能力矩阵与它一致（手工核对一次，之后由 1 的同一张表兜住）。

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
- [x] **拆 `ui/tools.rs`**（584 行、`begin_tool` 151 行）-> `ui/tools/`
  （mod = 分发 + `with_ctx` / `drive`，paint / select / pen / adjust 各管一类工具）。
  重复 20 次的 `ToolContext` 样板收成一处，`begin/continue/end_tool` 变成一行一臂。
- [x] **拆 `selfcheck.rs`**（1591 行、`check()` 509 行）-> `selfcheck/mod.rs`
  （420 行：夹具 + `check()` 166 行 + 报告）+ `selfcheck/checks/`（pipeline / chrome /
  pen / brush / fill / selection / clipboard / layer）。验收：`failures.push` 146 ->
  146、`--selfcheck` 报告逐字相同。
- [x] **分层规则上测试**：`src/architecture.rs`——核心层只能向下依赖，且只有
  `app` / `ui` 能碰控件栈（`igui_ui` / `igui_scene` / ...）；违反时报 `文件:行`。
- [x] **12 条请求通道 -> 一个意图队列**：`ui/intents.rs` 的 `Intent`（工具 /
  菜单 / 选项栏 / 钢笔 / 重命名 / 文件）+ `Intents`（`push` / `take` /
  `take_where` / `take_file`）。回调只 `intents.push(..)`，`update` 一次取走、按
  到达顺序处理，`Intent::File` 留在队列里给宿主。`EditorView` 字段 126 -> 115，
  `update()` 少 11 段 `if let Some(..) = self.*_request.take()`。
  验收：`--selfcheck` 逐字不变（253 控件 / 1023 命令）、测试 +3 条队列语义。
- [x] **`EditorView` 第一簇：布局**（`ui/layout.rs::Layout`）：三条分隔条的宽度 /
  高度 + 它们拖动 / 夹取的节点 + 侧栏标签页的当前页 / 按钮 / 内容节点，共 14 个
  字段收成一个 `layout` 字段。字段 115 -> 102（`NodeId` 38 -> 31）。
  验收：`--selfcheck` 逐字不变、拖分隔条 / 切标签页的测试全过。
- [x] **`EditorView` 第二簇：渐变**（11 个字段：色带 / 绘制引擎 / 类型 / 反向 /
  「自定义过」/ 选中节点 / 编辑器开关 / 3 个节点 / 按钮表）
  -> `ui/gradient_editor.rs::GradientState`。字段 102 -> 92（`NodeId` 31 -> 28）。
  验收：`--selfcheck` 逐字不变、渐变类型 / 编辑器的测试全过。
- [x] **`EditorView` 第三簇：选项栏**：27 个字段（19 个控件句柄 + 4 张按钮表 +
  4 个「显示源」共享格）-> `options_bar::OptionsState`（`EditorView.options`）。
  字段 92 -> 66。工具引擎（`brush` / `pen` / `paint_bucket` …）**保持散着**：
  它们名字已经说明职责、且被绘制逻辑到处引用，包一层只是把 `self.brush` 变成
  `self.tools.brush`。
  验收：test 449 passed、`--selfcheck` 逐字不变。
- [x] **拆 `ui/tests.rs`**（2435 行 / 88 个测试）-> `ui/tests/` 14 个功能文件 +
  `mod.rs`（夹具：`viewport` / 三种粒度的 `click*` / `label_text` /
  `paint_commands` / `paint_a_stroke` / `brush_view` / `select_tab` …）。
  最大 397 行（brush），测试数 88 -> 88。
  验收：`cargo test` 434 passed 不变、`--selfcheck` 逐字不变。
- [x] **构造函数**：`build_tree`（294）拆成 `build/` 四步 —— `cells.rs`
  （`Wiring::new`：所有共享格默认值）/ `panels.rs`（选项栏、工具栏 | 调色盘 |
  画布 | 右栏）/ `page.rs`（纵列顺序）/ `tree.rs`（27 行，只串起来挂树）；
  `assemble.rs` 的 11 段 `NodeRef -> NodeId` 收成 `slots()` / `ids()` 两个小函数，
  245 -> 213（剩下的是一条 `EditorView` 字面量，不再拆）。
  验收：`--selfcheck` 控件数 / 命令数逐字不变。
- [x] **`ui/menu.rs` -> 一张菜单表**（Phase 24）：八个 `*_menu` 返回
  `Vec<MenuEntry>`（`menu_spec`），`menu_content` 只负责把表画成下拉 —— 同一张表
  喂 macOS 原生菜单栏（§1.aa）。之前是「每个菜单一个函数、各自直接建 `Menu`」+ 4 个
  状态位收进 `MenuState`；现在 `MenuState` 多了 `has_document` / `checkerboard`。
- [x] **`ui/options_bar.rs::options_bar`**（223）-> 四行各一个函数
  （`rows::{brush_row, pen_row, bucket_row, gradient_row}`）；16 个参数收成
  `BarInputs`（意图队列 + 各工具的开关格）+ `OptionSlots`（按钮节点收集器），
  `#[allow(clippy::too_many_arguments)]` 去掉。文件 538 -> `options_bar/{mod,rows}.rs`
  （269 + 340）。
- [ ] **单函数过长**：`ui/build/assemble.rs::finish`（213，一条字面量，不拆）、
  `ui/gradient_editor.rs::gradient_editor_content`（160）、
  `ui/new_document.rs::build_content`（147）、`ui/layer_panel.rs::action_buttons`
  （137）。
- [x] **`app/application/mod.rs`**（953）-> `mod.rs`（402：Options / run /
  `ImageEditorLogic` 字段 + `AppLogic`）+ `home.rs`（83）+ `modal.rs`（51）+
  `workspace.rs`（107，含从 `update()` 抽出的 `sync_active_workspace`）+
  `tests.rs`（336，宿主级测试）。`update()` 66 -> 60 行但只剩一串「步骤调用」。
  验收：test 437 passed、`--selfcheck` 逐字不变。
- [x] **`ui/new_document.rs`**（843）-> `new_document/{mod 234, form 189,
  widgets 159, tests 324}`：对话框壳的转接 / 表单布局 / 小控件 / 测试各一层；
  `build_content` 的 8 个参数收成 `FormWiring`（共享格），去掉 `too_many_arguments`。
  评估过「把 `Window` 的转接再抽一层 `Modal`」：`new_document` 与 `confirm` 各只有
  6 行转发（measurer / event / layout / paint），抽出来只是多一层转发 —— 不做。
- [x] **`ui/window/mod.rs`**（824）-> `window/{mod 248, chrome 170, gesture 86,
  geometry 161, tests 361}`：状态与生命周期 / 三段外观 / 手势 / 纯几何 / 测试各一层。
  `Window::new` 从 120 行降到 45 行（外观交给 `chrome::frame`）。
  验收：test 437 passed、`--selfcheck` 逐字不变。
- [x] **`document/history.rs`**（694）-> `history/{mod 185, paint 106, layers 262,
  tests 165}`：接口与栈 / 像素命令 / 图层命令 / 测试各一层；`document/mod.rs` 的
  `pub use` 路径不变。
- [x] **`canvas/selection.rs`**（627）-> `selection/{mod 210, feather 62, merge 72,
  outline 71, tests 235}`：掩码与查询 / 羽化 / 合并 / 轮廓 / 测试各一层。
  `canvas/mod.rs` 的 `pub use selection::{MergeMode, Selection}` 不变。
- [x] **`tools/fill.rs`**（556）-> `fill/{flood 146, bucket 96, selection 89,
  tests 233}`：纯算法（容差 / 连续 / 魔棒区域）/ 油漆桶工具 / 按选区填充 / 测试。
  `tools/mod.rs` 的 `pub use` 路径不变。
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

## 编辑过程记录（延时动画的口子）

需求：把整段编辑过程「**全部保存**」下来，后面做「延时动画 / 过程回放」。

选型（结论）：**内存里录，落盘只在自然断点。**

- 录制跟着编辑会话长：每步 = 时间戳 + 命令名 + 一块像素补丁（区域 + **改后**
  像素）。不做「每笔写一次盘」——编辑期间碰磁盘会拖住 UI 线程；而且时间线必须
  独立于撤销栈（被撤销 / 被 32 步上限挤掉的步骤也要留着，否则回放会缺帧）。
- 落盘只在**保存工作区 / 关闭工作区**时，写同名 `.igtl`（`x.igui` -> `x.igtl`）。
  格式手写二进制、像素不压缩（保存时一次性写，换来回放零解码依赖）；读写一对在
  `io/timeline.rs`，`--selfcheck` 里有「录一笔 -> 落盘 -> 读回一致」的回环检查。
- 默认**关**：一步一个补丁（4 字节/像素），4K 长画会吃内存。开关是偏好
  `preferences` 里的 `record_timeline=0|1`，宿主在开工作区时套到 `History`。

落地：

- [x] `Command::replay()` + `ReplayPatch`（只有 `PaintCommand` 有；图层结构类
  命令只留 label，回放时当一次「切」）。
- [x] `document/history/recorder.rs`：`Recorder` / `RecordedStep` / `StepPatch`，
  时间戳记到毫秒，默认 4096 步上限（到上限丢最旧）。
- [x] `io/timeline.rs`：`encode/decode_timeline`、`write/read_timeline`、
  `timeline_path_for`（`io/bytes.rs` 的读写器与 `.igui` 共用）。
- [x] 偏好：`EditorPreferences { brush, record_timeline }`（原来只存笔刷外观的
  `BrushPreferences` 成为它的一个字段）。
- [x] 宿主：开工作区按偏好设、保存 / 关闭时 `flush_timeline`，状态栏报步数与路径。
- [x] 验证：`--selfcheck` 的 `check_timeline`（录一笔 -> 落盘 -> 读回一致 -> 关掉
  后不再累加）、宿主级 `saving_also_writes_the_timeline_when_recording_is_on`、
  格式 / 偏好的单测。
- [ ] **延时动画本身**：读 `.igtl` 按 `at` 排节奏、补丁按区域贴回一张画布逐帧
  出图；再决定是应用内播放还是导出 GIF / MP4（`png` 已有，GIF 要新依赖）。
- [ ] **图层结构类命令的补丁**（现在时间线里只有 label）：要么存「变化那层的
  整层像素」，要么记「层 id + 增删 / 顺序」，回放时重建图层栈。

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

- **关闭激活工作区后画布不刷新**（`app/application/files.rs`）：后端只有一张
  `DOCUMENT_TEXTURE`，`close_workspace` 删完只更新了标题，没让接手的那个工作区
  重合成，于是画布还停在已关闭文档的图像上（看起来像「关标签没反应」）。现在
  关的是激活标签时给新激活的工作区 `mark_texture_dirty()`；关后台标签不动当前
  纹理（不为 4K 文档白做一次整图合成）。验收：宿主测试分别断言“关激活 → 剩下
  的工作区产出一帧合成”和“关后台 → 不产出”。

- **画廊预览图**（`ui/home.rs` + `io/thumbnail.rs` + `app/application/home.rs`）：
  每条画廊条目左边一张缩略图（最长边 96px，等比居中在 56px 方框里，
  `PixelBuffer::scaled_to_fit` 面积平均降采样）；打开中的工作区用内存文档合成
  （`EditorView::thumbnail`），最近文件走配置目录下的
  `thumbnails/<路径哈希>.png` 缓存（保存时写、工作区更新后失效），像素由宿主
  `sync_home_thumbnails` 传到后端（纹理 id 从 2 起按下标分配，线性采样）。
  验收：`home::tests` 断言缩略图发出 `DrawImage` 且保持比例 / 装得进方框、
  `io::thumbnail` 缓存的往返与失效、宿主测试断言打开中的工作区带排好队的缩略图。

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
