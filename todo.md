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

---

## 2. 紧接着

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
- [ ] **`History` 上限**：目前无步数 / 字节上限，大文档多步编辑会一直涨。
- [ ] **工具元数据表**：新增一个工具要改 7+ 处（枚举 / `ALL` / 标签 / 图标 /
  `tool_groups` / `begin..end` 的 match / 快捷键）；可收成一张 `ToolSpec` 表。

## 3. 计划阶段（Phase 10+）

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
