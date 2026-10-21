# image_editor — 用 igui 自己画的图像编辑器

一个精简版 Photoshop 风格的 2D 图像编辑器。**界面用 igui 的 UI 栈**
（`igui_theme` / `igui_components` / `igui_ui` + `igui_backend_wgpu`）实现，
不用 `egui`。

这是一个独立项目（自己的 git 仓库），通过 **git 依赖**引用上游的 igui
（`Cargo.toml` 里的 `https://github.com/lazyfury/igui`，按 `release.md` 记录
的 release tag 固定，当前 `v0.3.0`）—— 不再需要相邻的路径 checkout。图标用
核心的 `igui_svg`（零外部依赖）；PNG 编解码用 `png` crate；结构化日志用
`tracing`。

## 运行

```bash
cargo run
cargo run -- --selfcheck
cargo run -- --dump
cargo run -- --frames 120
cargo run -- --light
cargo run -- --pixel-font
```

## 主页与单窗口模态框

启动先进**主页**（`src/ui/home.rs` 的 `HomeView`）：

- 「新建项目」：在主窗口里弹出一扇**模拟桌面的浮动窗口**（`src/ui/window/`
  的 `Window` + `src/ui/new_document.rs` 的 `NewDocumentModal`）选画布尺寸和背景色
  —— 不再开第二个原生窗口（macOS 15 上多 surface 会崩）。窗口带标题栏（拖动
  移动）、四条边 / 角（拖动缩放）、关闭按钮和遮罩；右下角画三道 45° 斜线提示
  「这里能缩放」；标题栏比正文浅一档（深色主题下更亮，上沿跟着外框圆角），页脚
  有 52px 最小高度（按钮再矮底部操作区也不会缩没）。Esc / 点窗口外取消，输入
  全被它吞掉。点「创建」后，编辑器出现在**同一个窗口**里（从主页切到
  `EditorView`），窗口关闭；点「取消」或 Esc 就放弃。
  尺寸除了 ±1 的步进（1..=4096），还有 12 个预设一键到位：像素画的
  16/32/64/128/256/512 方块，以及高分辨率的 1920×1080、1080×1920（竖屏）、
  2560×1440、3840×2160（4K）、2048×2048、2480×3508（A4 300dpi）。大尺寸走 CPU
  合成：release 下 4K 一次全图合合约 13ms（1080p 约 4ms、512² 约 0.5ms），
  所以窗口里有一句提示，debug 构建下 4K 会明显卡。
- 「画廊」：占位（一张 `EmptyState`），以后放最近打开的文档。

宿主（`src/app/application.rs`）不再是手写的 winit `ApplicationHandler`，而是用
`igui_app` 运行时把平台插件组装起来：`igui_winit` 的 `WinitPlugin` / `WgpuPlugin` /
`PointerPlugin` / `KeyboardPlugin` / `ImePlugin` / 文本度量 / `ClipboardPlugin`，
业务逻辑只实现一个 `AppLogic`（主页 / 编辑器 / 模态框都在这一个逻辑里）。

## 阶段

按下面的 Phase 计划逐步实现，**一次一个 Phase**。

- [x] **Phase 1 — Skeleton**：窗口、菜单栏、工具栏、画布占位、图层/属性栏、
  状态栏。工具选择是真的（点击 / 快捷键），其余按钮点击在状态栏给出
  “该功能属于哪个 Phase”的提示。
- [x] **Phase 2 — Document / Layer / PixelBuffer / Color**：后端无关的文档数据
  模型（`src/document/`）+ `AppState` 持有真实的 800×600 `Document`；画布占位
  和状态栏显示文档尺寸。合成 / 渲染仍未接。
- [x] **Phase 3 — Canvas（缩放 / 平移 / 坐标转换 / 显示图像）**：
  `src/renderer/` 的 CPU 合成器把文档图层合成为一张 `PixelBuffer`；宿主用
  `register_texture` 上传；[`EditorView::paint`] 按相机把文档图像、像素网格和选区
  画在画布区域的裁剪里（在所有 UI 之前），所以文档不会盖住面板、也不会溢出画布。
  `src/canvas/` 提供相机与坐标转换；滚轮缩放（锚定指针）、中键平移、
  `+`/`-`/`0`/`F` 快捷键，状态栏显示指针下的像素坐标。画布默认带一层透明
  棋盘格背景（`src/canvas/checkerboard.rs`）：只在显示用的合成结果上生成，
  所以隐藏 / 擦除掉不透明的「背景」图层就会露出来，而导出的 PNG 仍保留 alpha；
  视图菜单的「显示棋盘格」可以开关它。
  默认文档是 128×128 的像素画画布，宿主用
  `WgpuBackend::set_texture_filter(.., TextureFilter::Nearest)` 放大时做最近邻
  采样，像素不会插值模糊。
- [x] **Phase 4 — 图层管理**：图层面板接真实 `Document`：虚拟化图层列表
  （眼睛 / 名字 / 不透明度、点选当前图层）+ 增删 / 显示隐藏 / 不透明度 ±10% /
  上移下移 / 重命名（`Overlays::popover` 里一个上游 `TextInput`，Enter 确认・
  Esc 取消）；任何改动后重新合成
  并重传纹理。属性面板实时显示当前图层：名字 / 不透明度 / 混合模式，以及图层
  的 `position` 偏移与缓冲区尺寸（移动画布外内容后缓冲会变大，这里能看到）。
- [x] **Phase 5 — Brush / Eraser**：`src/tools/` 的 `Tool` 接口 +
  `BrushTool`（`BrushMode::{Paint, Erase}` 共用一个引擎，§11）；在画布上
  左键拖动即在当前图层绘制 / 擦除，实时重合成。默认是**像素模式**
  （`hard = true` + `BrushShape::Square`）：任意尺寸圆心都吸附到像素网格、
  `coverage` 二值（边缘不抗锯齿），适合画像素图；工具选项栏可切换「像素」
  与「方形」两个开关。关掉像素模式后，圆形笔走抗锯齿、方形笔走切比雪夫距离的
  软边，**硬度**（`brush.hardness`，选项栏 −/+）决定实心核心到边缘的过渡带宽度：
  1 = 只有 1px 抗锯齿边（旧版观感），0 = 从中心一路渐变到边缘。
  **不透明度按「一笔」封顶**：落笔开一张覆盖率蒙版（`tools/brush/mask.rs`），
  密集重叠的 stamp 只按最大覆盖率从落笔前快照重算一次，所以 50% 就是 50%、
  沿笔迹不变深，两笔相交才叠到 75%。
  选项栏的「预设 ▾」下拉列出 6 个内置预设（像素硬笔 / 像素方块 / 硬圆笔 /
  柔边圆笔 / 喷枪 / 柔方块，`tools/brush/preset.rs`），当前项打 `✓`，参数不命中
  任何预设时显示「自定义」。
  **光标圈**跟着指针画当前笔刷的轮廓（2px 黑底 + 1px 白线 + 中心十字，圆头画圆、
  像素模式画方框），指针离开画布就消失。
  **对称绘画**（关 / 垂直 / 水平 / 四向，选项栏「切换」循环）把每个落笔像素再镜像
  到文档中线的另一侧 —— 镜像在**像素级**做（`x' = width - 1 - x`），所以像素画也
  严格对称。
  **平滑强度**（选项栏「平滑 −/+」）：100% = 二次中点平滑（快笔画不出折角，代价是
  笔迹末端滞后半个事件段）；0% = 纯折线，笔到手到，像素画用。
  笔刷参数（直径 / 不透明度 / 硬度 / 平滑 / 间距 / 像素·方形 / 对称）落在配置目录的
  `preferences` 里，重启后还在 —— 视图只导出快照，读写文件是宿主的活。
  1px 硬边笔走 Bresenham 连线（每个像素只压一次，斜线不会加粗）。
  落笔点由 `tools::stroke` 采样器给出：光标输入先被「二次中点平滑」成曲线
  （三点一组，控制点取中间那个点，端点是相邻两点的中点 —— 不过冲，折角被
  抹圆），再按**固定距离**（直径的 1/4，像素笔至少 1px）取点，所以快速拖动
  （系统给的事件很稀）画出来的也是圆滑曲线，落笔密度不随速度变化。
  代价是画的过程中笔迹末端最多落后光标半个事件段，抬笔时补完最后半段。
  像素模式 + `zoom >= 6` 时在画布上叠加屏幕空间的像素网格。`[`/`]` 调大小，
  `,`/`.` 调不透明度。
  “一笔 = 一次 Undo”：抬笔时提交成一条 [`PaintCommand`]，由 Phase 6 的历史栈接管。
- [x] **Phase 5.x — 钢笔（路径）**：`canvas/path.rs` 放贝塞尔几何
  （`Anchor` 两侧控制柄 + `Path`，自适应细分展平 —— 预览按 `0.25 / zoom` 展平、
  光栅化按 0.25 文档像素），`tools/pen.rs` 是多击状态机：单击落尖角锚点、
  按下拖动出对称控制柄、点回第一个锚点闭合、`Enter` 收尾（开放）、`Esc` 取消、
  `Backspace` 删最后一个锚点；收尾后再点空白处就开始新路径。
  收尾后的动作在选项栏（`ui/pen.rs`）：**建立选区**（`Selection::polygon` 偶奇
  光栅化，开放路径按首尾相连）、**填充**（复用 `fill_selection`，按当前选区裁剪）、
  **描边**（复用当前画笔：`BrushTool::stroke_path` 沿折线等距打点，段内是直线不
  做中点平滑，所以折角保持折角；硬度 / 每笔不透明度 / 预设 / 像素模式全都跟着画笔
  走）、**清除**。路径**本身不进撤销栈**（和选区一样），填充 / 描边各算一步 undo。
  预览是屏幕空间的双色虚线路径 + 空心锚点方块 + 控制柄小圆点 + 跟手的橡皮筋，
  点回起点可闭合时起点方块变实心。
- [x] **Phase 6 — History**：`Command` 模式 + 两条栈
  （`src/document/history/`），一笔画笔 / 橡皮 = 一步 undo。命令只记录**差异
  区域的前后像素**（`PixelRegion`，靠落笔快照与抬笔缓冲求 diff），所以撤销内存
  跟笔画大小成正比，不是整层快照；新命令清空 redo 栈，栈有上限（默认 32 步）。
  工具栏加了 `↶ 撤销` / `↷ 重做` 按钮，快捷键 `Ctrl/Cmd+Z` 撤销、
  `Shift+Ctrl/Cmd+Z`（或 `Ctrl+Y`）重做，结果写状态栏。
  除了画笔，图层的编辑也入历史：移动工具（`SetLayerPositionCommand`）、新建 / 删除
  （`AddLayerCommand` / `RemoveLayerCommand`）、重命名 / 可见性 / 不透明度 / 排序
  （`LayerMetaCommand`，只存元数据 + 顺序，不克隆像素）、裁到文档
  （`CropLayerCommand`）。
- [x] **Phase 7 — 导入导出**：`src/io/` 用 `png` crate 做编码与解码，拆成
  两个关注点：`codec`（`PixelBuffer <-> PNG 字节`）与 `file`
  （`路径 <-> PNG 字节` + 文件名规则）。右侧「文件」面板的「导入 PNG…」/
  「导出 PNG…」走**原生文件对话框**（Phase 23），把图片作为**新图层**放到最
  上面并选中；导入不进撤销栈（与「+ 图层」一致）。
- [x] **Phase 8 — Move / 框选 / 吸管**：补齐三个工具。
  `tools/move_tool.rs` 在画布上拖动改变**当前图层**的 `position`（图层像素
  缓冲区的原点，可以变负；不进撤销栈，与图层的增删 / 排序一致）。画笔 / 橡皮
  在**文档坐标**落笔，写入时减去 `position` 换算到缓冲区，所以笔迹始终对着
  光标；落笔前 `Document::ensure_layer_covers_document` 把缓冲区扩到
  “当前范围 ∪ 文档范围”，于是拖动画布之后：图层移空、重新露在文档里的区域是
  透明像素、可以继续画（“画布外／canvas 内也能画”），而**移出画布的像素不裁掉**，
  留在缓冲区里还能再移回来（补空间是左侧 / 上方，历史命令的区域会跟着平移）。
  要回收被移动撑大的缓冲区，用图层菜单的「裁到文档」（按 `position` 摆好后裁回
  文档尺寸、丢弃画布外像素）。
  框选拖出选区（`canvas::pixel_selection` 把两个角点
  裁剪成整数 `PixelRegion`，存在 `AppState.selection`），**按住 Shift 加选、Alt 减选**
  （框选 / 套索 / 魔棒都一样，`Selection::{union, subtract}` + `MergeMode`），
  **Ctrl/Cmd+X / C / V** 剪切 / 复制 / 粘贴选区内容、**Shift+Ctrl/Cmd+V** 粘贴为
  新图层、**Delete** 清空选区内容
  （`tools/clipboard.rs` 的 `PixelClipboard`：按选区包围盒抓一块像素、alpha 已乘
  覆盖率，贴回原位置并按当前选区裁剪；每次操作一步 undo，宿主跨工作区共享一块
  进程内剪贴板；macOS 上复制还会写一份 PNG 到**系统剪贴板**，进程内没有内容时粘贴会
  去系统剪贴板找一张图（先 PNG、再 TIFF —— TIFF 交给 AppKit 的
  `NSBitmapImageRep` 转成 PNG 再解码），放在选区左上角 / 文档左上角），
  画笔落笔按选区裁剪，
  选中时画一圈黑白相间的「马赛克线段」描边（`ui/selection.rs`，亮底 / 暗底都
  看得见），Esc 清空；「选择」菜单里「全选」/「反选」/「羽化」分别走 `Selection::all` /
  `Selection::invert` / `Selection::feathered`（掩码是 **u8 覆盖率**，羽化后画笔和
  填充都按覆盖率衰减，边缘是渐变而不是硬边）（快捷键 `Ctrl/Cmd+A`、`Shift+Ctrl/Cmd+I`，没有选区时反选等于
  全选、全选再反选等于取消选择）；吸管用 `renderer::sample_pixel` 取**合成后**的
  颜色写进前景色。三个工具的纯逻辑分别放在 `canvas` / `renderer` / `tools`，
  视图只做坐标换算与状态同步。
- [x] **Phase 9 — 菜单栏（真实下拉）**：标题点击改成一个共享请求格，
  `EditorView::update` 用它调 `igui_components::Overlays::menu` 弹出下拉；
  下拉内容是一个 `Menu`（`MenuItem` 行 = 左标签 + 右快捷键，动作不可用时
  `disabled`）。撤销 / 重做、导入 / 导出、缩放 / 适配、取消选区、关于已接真实
  动作，其余是带标注的占位项。菜单打开时点**另一个**标题会一次点击切换、点
  当前标题则关闭（标题的点击在覆盖层消费外部点击之前被 `EditorView` 拦下，
  再交给主树）。`Menu` / `MenuItem` / `Overlays::menu` 都在核心包
  `igui_components`，Phase 11 的右键菜单直接复用。
- [x] **Phase 9.x — 工具选项栏 + 可拖动右栏**：菜单栏下面多一行工具选项栏
  （`ui/options_bar.rs`）：画笔 / 橡皮显示笔刷大小、不透明度与 `−` / `+`
  按钮，其余工具显示一句操作提示；`−` / `+` 只写请求格，`update` 统一调整。
  右侧栏宽度改用 `ResizeHandle::vertical(..).invert()`（目标在把手右边）拖动，
  并在 `layout` 里 `clamp_sidebar_width`，保证画布不被挤没。右栏内部的
  「文件 / 图层 / 属性」三块之间也用 `ResizeHandle::horizontal(..)` 分隔，可以拖动
  调整各自高度（`文件` 驱动上面的面板、`属性` 用 `invert()` 驱动下面的面板，中间
  图层面板 `grow(1)` 吸收剩余）；`clamp_panel_heights` 预留图层最小高度。
  右栏还多了「历史」面板（`ui/history_panel.rs`）：一条虚拟化 `List` 显示
  撤销 / 重做栈（旧→新 → 「● 当前」→ 重做下一个），点某一步就撤 / 重做到那里。
  左侧工具栏右边是**独立的调色盘面板**（`ui/palette.rs`，宽度可拖）：上面一个
  **HSV 取色器**（饱和/明度方块 + 色相条，拖动即改前景色），下面是当前前景 /
  背景与 16 个预设色块。取色器靠 `Component::on_pointer`（press + move 给绝对
  位置）把指针映射到自己的矩形上；没有渐变图元，方块用一小片实心色块拼出。
  面板外观抽成了项目内的 `ui/card.rs`（`Card`：`surface` 底 + 内边距 + 行间距，
  无边框 / 圆角），右栏的文件 / 图层 / 属性 / 历史和它用同一套，侧栏看起来是
  一组卡片。

- [x] **Phase 12–16 — 选区与填充**：选区从「只能是矩形」升级成任意形状
  （`canvas::Selection`：与文档同尺寸的像素掩码 + 线段轮廓），因此有了**自由
  选区（套索）**（`L`：拖动折线，松手成选区，拖动中实时画 marching ants）和
  **魔棒**（`W`：点画布把容差连通区域变成选区，容差 / 「连续」与油漆桶共用）；
  填充工具共用 `tools::fill`：**油漆桶**（`G`：洪水填充，选项栏容差 `−/+`、
  「连续」开关）、**矩形填充**（`R`：拖矩形，松手用前景色填充 + 拖动预览）、
  **自由填充**（`U`：拖自由形状，松手填充内部）。都按当前选区裁剪，一次填充
  = 一条 undo（快照 + `PixelRegion::diff`）。
- [x] **Phase 17 — 渐变工具（多节点 ramp）**：`tools::gradient` 的五种类型
  （线性 / 对称 / 径向 / 角度 / 菱形）+ `document::Gradient` 多色标 ramp（采样 /
  增删移 / 端点跟随前景背景）；拖一条轴定义方向，实时改图层预览、松手一条
  undo；`ui/gradient_editor` 的色带 + 可拖动节点编辑器。

- [x] **Phase 18–22 — 多工作区 / 工作区文件 / 画廊**：`io/workspace.rs` 的
  `.igui` 二进制容器（多图层 + PNG 像素）、`io/recent.rs` 的最近列表、
  `app/workspaces.rs` 的多工作区 + `ui/workspace_tabs.rs` 顶部标签条，以及主页
  画廊列出打开中 / 最近工作区。
- [x] **Phase 23 — 原生文件对话框 / 保存流程 / 拖放**：`io/dialog.rs` 手写平台
  FFI（macOS `NSOpenPanel`/`NSSavePanel`、Windows `comdlg32`、其它 Unix
  `zenity`/`kdialog`）；文件面板按钮只发 `FileRequest` 意图，宿主解析路径后执行
  「保存 vs 另存为」；`ui/confirm.rs` 关闭脏工作区前确认；`app/drop_files.rs`
  把 `winit` 的 `DroppedFile` 转成新图层导入。

### 计划（Phase 10+）

本示例的待办已经整理到同目录的 [`todo.md`](todo.md)（含像素模式主线和
Phase 11），README 只留功能说明。要点：

- **Phase 11 — 图层右键菜单**：两个前置（增量）：`Overlays` 支持原始矩形 / 指针
  位置锚点；`igui_ui` 增加右键回调。

## 主题（紧凑）

编辑器用自己的主题 `theme::editor_theme(light)`：设计系统的调色板 +
`Density::COMPACT`（间距 ×0.75、控件更矮、默认 mini 按钮）。这是一次 token
替换，不是第二条代码路径 —— 组件库的间距 / 控件高度都从 `Theme` 读
（`theme.spacing(..)` / `theme.control_height(..)` / `theme.row_height()`），
所以改 `editor_theme` 一处就能整体调紧/调松。想回到常规尺寸用
`Density::COMFORTABLE`。

工具栏的图标尺寸是**显式固定**的（`icons::TOOLBAR_ICON`，20px，画在按钮中央），
不从按钮矩形推算 —— 所以紧凑主题把按钮变矮时图标不会跟着缩水；图标按钮用
`.min_size(32, 28)` 钉住高度，密度默认值不会覆盖它。

## 性能（大文档）

- `[profile.dev] opt-level = 2`：像素级循环在 opt-level 0 下慢一个数量级以上，
  4K 文档的每帧合成从 443 ms 降到 10 ms 量级；`debug_assertions` / 行号表都还在。
- **脏矩形合成**：画笔只把「这一笔写过的包围盒」标脏，`CpuRenderer::render_region`
  只重算那一块（棋盘格也只在脏区补），4K 每帧 ≈ 0.95 ms（1080p 0.27 ms）。
- **抬笔不扫整层**：笔画的包围盒由 `Writer` 增量记录，抬笔时 `PaintCommand::capture_in`
  只比那块区域 —— 4K 抬笔 0.08 ms，撤销内存 ≈ 笔迹大小（不再是整层 33 MB）。
- **选区不扫整张掩码**：`Selection` 的 `area` / `bounds` 构造时缓存，轮廓按段合并
  （整块矩形 4 条边），4K 全选 0.11 ms、蚂蚁线命令从 1.2 万条降到 800 条。
- **采样方式跟着缩放走**（`canvas::ImageSmoothing`）：倍率 ≥ 1 用最近邻（像素画硬边
  不被插值糊掉），**缩小时改线性** —— 4K 适配窗口只有 0.18 倍，最近邻会直接丢像素，
  细笔迹在屏幕上断成毛刺。最近邻时还会把文档矩形对齐到**设备像素网格**
  （`scale_factor` 由宿主每帧同步），否则 1:1 缩放也会出现 1px / 2px 交替的锯齿。

## 窗口布局（自上而下）

```
菜单栏（文件 / 编辑 / 图像 / 图层 / 选择 / 滤镜 / 视图 / 帮助）   ← 贴顶
工作区标签条（主页 · 各工作区标签 · + 新建）                      ← 宿主画，编辑器留出这一条
工具选项栏（随当前工具变化：预设 / 大小 / 硬度 / 平滑 / 对称 / …）
[工具栏] [画布] [调色盘] [右栏：文件·图层·属性 / 历史]  状态栏
```

- **菜单栏属于编辑器**（动作要读当前文档 / 历史 / 选区），工作区标签条属于**宿主**
  （跨工作区、能回主页）。两者靠「标签带」协作：编辑器在菜单栏和选项栏之间留出
  `TAB_STRIP_HEIGHT` 的空行，宿主把标签条钉在 `menu_bar_rect().bottom()` 上。
- **绘制顺序是「标签条 → 内容 → 浮层」**（`application::paint_screen`）：标签条是窗口
  chrome，必须先画，编辑器的 `Overlays`（菜单下拉、预设菜单、重命名浮层）才能盖在
  它上面 —— 否则菜单弹窗会被标签条切掉一条。
- 没有标签条的场景（`set_tab_band(0)`，例如单元测试）这一行会折叠，选项栏直接
  贴着菜单栏。
- 主页屏幕没有菜单栏也没有标签条（宿主整屏画首页）。

## 快捷键

| 键 | 作用 |
|---|---|
| `V` | 移动当前图层（画布上拖动） |
| `B` | 画笔 |
| `E` | 橡皮 |
| `M` | 矩形选择 |
| `L` | 套索 |
| `P` | 钢笔（点击落锚点 / 拖动出曲线 / 点回起点闭合） |
| `G` | 油漆桶 |
| `W` | 魔棒 |
| `N` | 渐变 |
| `R` | 矩形填充 |
| `U` | 自由填充 |
| `I` | 吸管 |
| `Enter` / `Esc` / `Backspace` | 钢笔：收尾 / 取消 / 删最后一个锚点（钢笔激活时） |
| `Shift` / `Alt` + 选区拖拽 | 加选 / 减选（框选、套索、魔棒） |
| `Ctrl/Cmd+X` / `C` / `V` | 剪切 / 复制 / 粘贴选区内容（进程内剪贴板） |
| `Delete`（macOS 上即 Backspace） | 清空选区内容的像素 |
| `Ctrl/Cmd+A` | 全选 |
| `Shift+Ctrl/Cmd+I` | 反选 |
| `Esc` | 清空选区 |
| `Ctrl/Cmd+Z` | 撤销 |
| `Shift+Ctrl/Cmd+Z` / `Ctrl+Y` | 重做 |
| `[` / `]` | 笔刷直径 ∓2px |
| `,` / `.` | 笔刷不透明度 ∓5% |
| `+` / `=` / `-` | 画布缩放 |
| `0` | 100% 并居中 |
| `F` | 适配画布区域 |

> 修饰键快捷键在**宿主层**处理：`InputEvent::KeyDown` 没有修饰键字段
> （`igui_core` 刻意保持最小），所以 `app/application.rs` 把 `Ctrl/Cmd+Z`
> 直接映射到 `EditorView::undo/redo`，不当作普通按键。

## 结构

```text
assets/icons/            # vendor 的 12 个 Lucide 图标（工具栏用）+ ISC LICENSE
src/
├── main.rs              # 参数解析
├── fonts.rs             # 随包字体定位（assets/fonts/，见 AGENTS.md）
├── app/
│   ├── state.rs         # ActiveTool / CanvasCamera / AppState（纯数据）
│   ├── tool_groups.rs   # 工具栏分组（组内工具 + 默认脸）
│   ├── workspaces.rs    # 多工作区（EditorView + 路径 + 脏标记）
│   └── application/     # winit + wgpu 宿主
│       ├── mod.rs       # Options / run / build_app + ImageEditorLogic 字段 + AppLogic
│       ├── home.rs      # 画廊条目（打开中 + 最近）+ 主页请求
│       ├── modal.rs     # 新建文档模态 + 关闭确认框的生命周期
│       ├── workspace.rs # 工作区新建 / 切标签 / 每帧同步（偏好、纹理采样）
│       ├── files.rs     # 保存 / 打开 / 导入导出 / 关闭确认 / 拖放
│       ├── shortcuts.rs # 确认解析 + ⌘ 快捷键
│       ├── plugins.rs   # 字体度量 / 帧数上限插件
│       └── tests.rs     # 宿主级测试（模态 / 标签条 / 保存流程）
├── icons.rs             # Lucide 图标包加载 + `Icon` 组件（igui_svg）
├── document/            # Phase 2：后端无关的数据模型
│   ├── mod.rs
│   ├── color.rs         # 8-bit RGBA
│   ├── id.rs            # DocumentId / LayerId
│   ├── point.rs         # 文档像素坐标
│   ├── region.rs        # Phase 6：整数像素矩形 + diff + contains（选裁剪）
│   ├── gradient.rs      # 多节点渐变 Gradient + GradientStop（ramp 采样 / 增删移）
│   ├── pixel_buffer.rs  # 行优先 RGBA
│   ├── layer.rs         # Layer + BlendMode
│   ├── history/         # Phase 6：撤销 / 重做
│   │   ├── mod.rs       #   Command trait + History（两条栈 / 上限 / 平移）
│   │   ├── paint.rs     #   PaintCommand（区域前后像素）
│   │   ├── layers.rs    #   图层增删 / 元数据 / 移动 / 裁到文档
│   │   └── tests.rs     #   栈语义与命令的测试
│   └── document.rs      # Document + 图层栈操作
├── canvas/              # Phase 3：画布相机与坐标转换
│   ├── mod.rs           # DOCUMENT_TEXTURE 约定
│   ├── camera.rs        # CanvasCamera（zoom / offset / fit）
│   ├── coordinate.rs    # screen -> document -> pixel + pixel_selection（框选）
│   ├── path.rs          # Phase 5.x：贝塞路径（Anchor / Path / 自适应展平）
│   └── selection.rs     # Selection：像素掩码 + 线段轮廓（矩形 / 套索 / 魔棒）
├── renderer/            # Phase 3：合成器
│   ├── mod.rs           # Renderer trait
│   ├── target.rs        # RenderTarget
│   └── cpu.rs           # CpuRenderer（Normal + opacity + position）+ sample_pixel
├── io/                  # Phase 7 / 18–19：导入导出 + 工作区文件
│   ├── mod.rs           # IoError
│   ├── codec.rs         # PixelBuffer <-> PNG 字节（png crate）
│   ├── file.rs          # 路径 <-> PNG 字节 + 默认命名 / 工作区路径
│   ├── workspace.rs     # .igui 二进制容器（多图层 + PNG 编码像素）
│   └── recent.rs        # 最近工作区列表（配置目录下的文本文件）
├── tools/               # Phase 5/8：工具系统
│   ├── mod.rs
│   ├── tool.rs          # Tool trait + PointerEvent + ToolContext（含 History）
│   ├── brush/           # 画笔：mod（BrushTool + 笔触 + 抬笔提交一笔）
│   │   ├── tip.rs       #   BrushShape + Tip：直径 / 硬度 -> 一个 stamp 的覆盖率
│   │   ├── mask.rs      #   一笔的覆盖率蒙版 + 按蒙版从快照合成（每笔不透明度）
│   │   └── preset.rs    #   BrushPreset + 内置预设（选项栏「预设」菜单）
│   ├── stroke.rs        # 笔触采样：二次中点平滑 + 按固定距离取点
│   ├── pen.rs           # PenTool：多击建路径 -> 选区 / 填充 / 描边的状态机
│   ├── fill.rs          # 洪水填充 flood_fill + PaintBucketTool + magic_wand_region
│   ├── gradient.rs      # GradientKind / gradient_fill / GradientTool（多节点渐变）
│   ├── shape_fill.rs    # RectangleFillTool（拖动矩形填充）
│   └── move_tool.rs     # Phase 8：MoveTool（拖动当前图层）
├── ui/
│   ├── mod.rs           # EditorView：状态字段 + new()（转交 build）+ update()
│   ├── build/           # EditorView 构造，按步骤拆开
│   │   ├── mod.rs       #   Wiring / Lists + editor()
│   │   ├── cells.rs     #   Wiring::new()：所有共享格的默认值
│   │   ├── panels.rs    #   选项栏 / 工具栏 | 调色盘 | 画布 | 右栏
│   │   ├── page.rs      #   页面纵列顺序（菜单 → 标签条 → 选项栏 → 主行 → 状态栏）
│   │   ├── tree.rs      #   串起来 + 挂到 SceneTree 根上
│   │   └── assemble.rs  #   NodeRef -> NodeId，填出 EditorView
│   ├── accessors.rs     # EditorView 的查询 / 点击目标访问器（宿主 / 测试 / 自检）
│   ├── paint.rs         # 文档 / 像素网格 / 选区 / 预览的绘制
│   ├── tools/           # 工具输入分发（指针事件 -> 当前工具）
│   │   ├── mod.rs       #   分发（begin/continue/end_tool）+ 共用的 with_ctx / drive
│   │   ├── paint.rs     #   画笔 / 橡皮 / 移动 / 油漆桶 / 矩形填充 / 渐变 / 吸管
│   │   ├── select.rs    #   框选 / 套索 / 魔棒 / 自由填充 + 选区命令
│   │   ├── pen.rs       #   钢笔指针 + 悬停 / 键位转发
│   │   └── adjust.rs    #   选项栏数值请求 -> 工具参数（画笔 / 预设 / 容差）
│   ├── pen.rs           # 钢笔动作：路径 -> 选区 / 填充 / 描边（+ 清除）
│   ├── sync.rs          # 状态 -> 控件同步（面板 / 选项栏）
│   ├── menus.rs         # 菜单下拉 / 长按浮层 / 渐变编辑器开关与动作
│   ├── input.rs         # 事件分发 + 画布缩放 / 平移 / 复位
│   ├── layout.rs        # 布局 / 分隔条夹取 / 画布区域几何
│   ├── io.rs            # PNG 导入 / 导出（路径由宿主给）
│   ├── tests/           # #[cfg(test)] 视图级测试（按功能分文件）
│   │   ├── mod.rs       #   夹具（viewport / click* / label_text / paint_commands …）
│   │   ├── brush.rs     #   画笔 / 橡皮 / 选项栏参数 / 像素模式 / 对称 / 光标圈
│   │   ├── canvas.rs    #   相机 / 缩放 / 平移 / 裁剪 / 棋盘格 / 像素网格
│   │   ├── chrome.rs    #   标签条 / 菜单栏 / 选项栏的顺序，状态栏文本
│   │   ├── selection.rs #   选区合并 / 羽化 / 魔棒 / 套索 / 马赛克描边
│   │   ├── pen.rs       #   钢笔：建路径 / 闭合 / 选区 / 填充 / 描边
│   │   ├── panels.rs    #   调色盘 / 分隔条 / 历史面板 / 图层列表
│   │   ├── layers.rs    #   属性面板 / 裁剪 / 重命名浮层
│   │   ├── menus.rs     #   菜单栏 + 编辑菜单的剪切复制粘贴
│   │   ├── toolbar.rs   #   工具栏分组 / 长按浮层 / 图标包
│   │   ├── history.rs   #   撤销 / 重做 / 脏区域
│   │   ├── io.rs        #   PNG 进出 + 文件按钮意图
│   │   ├── tools.rs     #   移动 / 吸管
│   │   ├── fill.rs      #   矩形填充 / 自由填充
│   │   └── gradient.rs  #   渐变填充 / 类型 / 端点 / 编辑器
│   ├── card.rs          # 项目内简单卡片（侧栏统一外观：surface + 内边距）
│   ├── menu.rs          # 菜单栏
│   ├── toolbar.rs       # 分组工具栏（长按展开切换）+ 撤销/重做
│   ├── workspace_tabs.rs # 工作区标签条（切换 / 关闭 / 新建 / 主页）
│   ├── palette.rs       # 左侧调色盘面板（HSV 取色器 + 前景/背景 + 预设色块）
│   ├── options_bar/     # 工具选项栏
│   │   ├── mod.rs       #   类型 / BarInputs / OptionSlots / options_bar + 提示
│   │   └── rows.rs      #   四行：画笔 / 钢笔 / 油漆桶 / 渐变 + 步进 / 开关按钮
│   ├── canvas.rs        # 透明画布区域（命中 / 定位用）
│   ├── selection.rs     # 选区描边（黑白相间马赛克线段）
│   ├── gradient_editor.rs # 多节点渐变编辑器（色带 + 节点手柄 + 配色行）
│   ├── file_panel.rs    # Phase 7：文件面板（路径 + 导入 / 导出按钮）
│   ├── layer_panel.rs   # 图层面板（List + 操作按钮）
│   ├── history_panel.rs # 历史面板（List：undo/redo 栈，点一步跳过去）
│   ├── properties_panel.rs  # 当前图层属性（只读）
│   ├── rename.rs        # 图层重命名浮层（`Overlays::popover` + `TextInput`）
│   ├── new_document/    # 「新建文档」浮动窗口
│   │   ├── mod.rs       #   共享格 + NewDocumentModal（对话框壳的转接）
│   │   ├── form.rs      #   表单：尺寸步进 / 预设 / 背景色 + 页脚按钮
│   │   ├── widgets.rs   #   步进行 / 预设按钮 / 背景色块（透明色棋盘格）
│   │   └── tests.rs     #   模态的行为测试
│   ├── window/          # 模拟桌面的浮动窗口
│   │   ├── mod.rs       #   窗口状态 + layout / event / paint
│   │   ├── chrome.rs    #   三段外观：标题栏 / 内容区 / 页脚 + 缩放手柄
│   │   ├── gesture.rs   #   拖动 / 缩放手势（命中顺序）
│   │   ├── geometry.rs  #   纯几何：命中带 / 改矩形 / 钳进视口
│   │   └── tests.rs     #   拖动 / 缩放 / 关闭 / 外观的测试
│   └── status_bar.rs
├── selfcheck/           # 无头自检（录制 DrawList + igui_profile 体检）
│   ├── mod.rs           #   录制 / 点击 / 取文字等夹具 + check() 主流程 + 报告
│   └── checks/          #   按功能分块的检查：pipeline / chrome / pen / brush / ...
└── architecture.rs      # #[cfg(test)] 分层约束：核心层不依赖 ui / app / 控件栈
```

## 编辑过程记录（延时动画的口子）

偏好 `preferences` 里 `record_timeline=1`（默认关）后，整段编辑过程会**全部**
留在内存里（每步：时间戳 + 命令名 + 一块「改后像素」补丁，独立于撤销栈），
保存 / 关闭工作区时顺手写到同名 `.igtl`（`x.igui` -> `x.igtl`）。格式是手写
二进制（magic `IGTL0001`，读写一对在 `src/io/timeline.rs`），给以后的「延时动画 /
过程回放」用。细节与取舍见 `todo.md` 的「编辑过程记录」一节。

## 画布操作

| 操作 | 效果 |
|---|---|
| 滚轮 | 缩放（锚定指针下的文档点） |
| 中键拖拽 | 平移 |
| `+` / `=` | 放大 |
| `-` | 缩小 |
| `0` | 100% 并居中 |
| `F` | 适配画布区域 |
| `[` / `]` | 笔刷直径 ∓2px |
| `,` / `.` | 笔刷不透明度 ∓5% |
| 左键拖动 | 画笔 / 橡皮画笔触（先选 `B`/`E`） |

## 图层操作

在右侧图层面板里先点选一个图层，再用按钮操作它：`+ 图层` / `− 删除`、
`显示/隐藏`、不透明度 `−` / `+`、`↑` / `↓`、`重命名`（弹一个 `TextInput`
浮层，Enter 确认 / Esc 取消）。列表第 0 行是最上面的图层。

## 文件与导入导出（Phase 7 / 23）

右侧「文件」面板（顶部一行是当前工作区文件的**只读**路径）：

- 「保存」：工作区有路径就直接写，没有就等同「另存为」（`Ctrl/Cmd+S`）。
- 「另存为…」：弹原生保存框，默认文件名是工作区名 + `.igui`（`Shift+Ctrl/Cmd+S`）。
- 「打开…」：弹原生打开框，过滤 `.igui`（`Ctrl/Cmd+O`）。
- 「导出 PNG…」：把当前文档**合成后**写盘（含透明背景；隐藏的图层不写），
  默认文件名是文档名 + `.png`（`Ctrl/Cmd+E`）。
- 「导入 PNG…」：把图片解码后作为**新图层**放在最上面并选中。图片尺寸可以与
  文档不同：合成器按 `Layer.position` 裁剪，超出画布的部分不显示。

原生对话框是**手写平台 FFI**（不引第三方对话框 crate）：macOS 走 AppKit 的
`NSOpenPanel` / `NSSavePanel`，Windows 走 `comdlg32`，其它 Unix 走
`zenity` / `kdialog` 子进程。`--selfcheck` 不注入真对话框，所以永不弹窗。
路径打不开 / 不是 PNG 只在状态栏报错，不会崩。

关闭有未保存改动的工作区会先弹「保存 / 不保存 / 取消」；把 PNG **拖进窗口**
则作为新图层导入（没有打开的工作区时，按图片尺寸新建一个文档）。

## 工具（Phase 8 / 12–17）

| 工具 | 键 | 操作 |
|---|---|---|
| 移动 | `V` | 在画布上拖动 → 当前图层按指针位移改 `position`（不进撤销栈） |
| 框选 | `M` | 拖动出矩形选区；画笔 / 填充 / 渐变只在选区内生效；选中有描边；`Esc` 清空 |
| 套索 | `L` | 按住拖出自由折线，松手成任意形状选区；`Esc` 清空 |
| 油漆桶 | `G` | 点画布洪水填充连通区域；选项栏调容差 / 「连续」 |
| 魔棒 | `W` | 点画布把容差连通区域变成选区（不落笔）；容差 / 「连续」与油漆桶共用 |
| 渐变 | `N` | 拖一条轴定义方向 / 长度，用**多节点 ramp** 填充选区；选项栏切类型（线性 / 对称 / 径向 / 角度 / 菱形）、「反向」、「编辑…」打开多节点编辑器 |
| 矩形填充 | `R` | 拖出矩形，松手用前景色填充；拖动中有预览 |
| 自由填充 | `U` | 按住拖出自由形状，松手填充内部 |
| 吸管 | `I` | 点画布 → 取**合成后**的颜色作为前景色（画笔颜色） |

选区是编辑器状态（`AppState.selection: Option<Rc<canvas::Selection>>`），不是
文档数据；`Selection` 是「与文档同尺寸的像素掩码 + 描边轮廓」，所以矩形框选和
自由套索是同一个类型，画笔 / 填充 / 渐变都直接查掩码（`contains` O(1)）。移动只改
`Layer.position`，合成器会把偏移算进摆放；和图层增删 / 排序一样**不**占用撤销
步数。

## 渐变（多节点 ramp）

渐变是一条 `document::Gradient`：一串色标（`position` 0..=1 + `Color`，≥2 个，按
位置排序），采样时定位区间并对两端做 RGBA 插值，两端 clamp——就是游戏引擎里的
color ramp。默认两端是当前前景 / 背景，**未自定义时**端点每帧跟随它们；一旦在
编辑器里加 / 移 / 改节点，就冻结成自定义。

选项栏（仅渐变工具显示）：类型 `−/+`、色带预览、「编辑…」、「反向」。点
「编辑…」用 `Overlays::popover` 打开 `ui/gradient_editor.rs`：一条色带预览 + 一排
节点手柄，`on_pointer` 把指针 x 映射成位置——按下靠近手柄选中并拖动，按下空白
新增节点；下面一行用「前景色 / 背景色」给选中节点上色（复用现有调色盘改前景的
流程）、「删除」「重置」。

拖动时渐变直接改当前图层做实时预览（落笔存快照，每次 move 从快照恢复再重画），
松手 `PixelRegion::diff` 提交一条 undo；用 `blend_pixel`（source-over）合成，所以
透明端会保留底下像素。

## 多工作区与工作区文件

**多工作区**：`app/workspaces.rs` 的 `Workspaces` 持有一串 `Workspace`（每个是一个
`EditorView` + 落盘路径 + 上次保存的 revision），宿主只把 `event/layout/paint` 发给
当前激活的那个。编辑器顶部一条 `ui/workspace_tabs.rs` 的标签条（`EditorView` 用
`set_top_inset` 预留 32px）：点标签切换、× 关闭、`+ 新建`、`主页` 回画廊；脏工作区
的标签带一个 `●`。

**自定义格式 `.igui`**（`io/workspace.rs`）：自包含二进制（小端，version 1）：
magic + 文档尺寸 / 背景 / 名字 / 激活图层 + 每个图层的名字 / 可见性 / 不透明度 /
混合模式 / 位置 / 像素尺寸，像素用 **PNG 编码**（复用现有编解码，零新依赖）。
解码对坏 magic / 截断 / 非法字段返回 `IoError::Invalid`。

**保存 / 打开**：右侧「文件」面板的路径格是共用的——「保存工作区」把当前文档写成
`.igui`（路径扩展名换成 `.igui`），「打开工作区」从路径读成一个新工作区。宿主
（`app/application.rs`）执行 IO、更新 `Workspace.path` / 脏标记、并把路径记入最近
列表。

**最近工作区**（`io/recent.rs`）：配置目录（macOS `Application Support` / Windows
`%APPDATA%` / 其余 `XDG_CONFIG_HOME`）下的 `recent` 文本文件，每行一个路径、最新
在前、上限 20；读取时滤掉已不存在的文件。

**主页画廊**（`ui/home.rs`）：列出打开中的工作区（点击切回去）和磁盘上的最近
工作区（点击打开），读文件头拿到名字 / 尺寸 / 图层数；没有就显示空状态。

## 验证（无截图）

遵守仓库规则：不截图。`--selfcheck` 把同一棵视图画进
`igui_backend_recording::RecordingBackend`，用 `igui_profile::inspect` 做结构
体检，并断言菜单 / 工具 / 画布 / 图层 / 图标 / 状态栏的文字都在命令流里。
Phase 6 起还做一次**真实的撤销 / 重做往返**：画笔一笔 → 点工具栏「撤销」→ 断言
合成结果回到白底且有 redo → 点「重做」→ 断言又变黑；最后断言历史栈是
`undo 1 · redo 0`。Phase 7 再加一段**真实的 PNG 往返**：把当前文档导出到临时
文件、解码回来核对白底与画笔像素；另写一张 4×3 红色 PNG 点「导入 PNG」，断言
图层数 +1 且合成后左上角变红。Phase 8 再验三个工具：吸管点在黑色笔画上 → 前景色
变黑；拖出框选 → 选区存在；移动拖动 → 当前图层位置改变。附加的图标包也会断言：
索引覆盖工具栏的工具与撤销 / 重做，且帧里确实有图标描出来的 `FillCircle`。同一套
断言在 `cargo test` 里跑。

这些步骤（`fmt` / `clippy -D warnings` / `test` / `--selfcheck`）也在
`.github/workflows/ci.yml` 里自动跑；本地按 AGENTS.md 的「per-change gate」
一次跑完即可。

## 图标（Lucide，附加）

工具栏的 9 个工具收成 **5 个组按钮**（见下）和撤销 / 重做各一枚 Lucide 图标。
图标是一个真正的 `Icon` **组件**（`src/icons.rs`）：`Icon::new(icons, name, color, size)`
占 `size × size`，在自己的矩形里居中描边；构建按钮时直接
`.child(Icon::new(..))`（见 `ui/toolbar.rs`），不用等树建好再回头挂装饰器。
按钮本身是空的点击 / 高亮区，`Icon` 的 `mouse_filter` 是 `Ignore`，点击落到按钮上。
当前工具用 `theme.palette().selection` 高亮（`Button` 的显式 `dynamic_background`
会覆盖 variant 的默认背景）。

### 分组工具栏（长按展开）

`ActiveTool` 的 10 个工具按 [`app::tool_groups`](src/app/tool_groups.rs) 收成 7 个
组按钮：移动、画笔、橡皮、选区（框选/套索/魔棒）、油漆桶（油漆桶/渐变）、填充形状
（矩形填充/自由填充）、吸管。组按钮画当前「脸」工具的图标 + 标签，右下角一个小
三角表示可展开：

- **短按**：激活脸工具（写一个请求格，`update` 统一应用）。
- **长按**（按住 ≥ 350ms 且没移动超过 6px）：`update` 轮询长按状态，到点用
  `Overlays::popover(.., Placement::Right, ..)` 弹出浮层，列出组内全部工具。
- 键盘快捷键 / 浮层选中的工具会把所属组的「脸」更新成它，所以激活组的图标永远
  等于当前工具。
- 等长按阈值期间 `EditorView::needs_frame` 返回 true，宿主持续出帧（winit 侧
  靠它 `request_redraw`）。

描边走核心的 `igui_svg`（backend-neutral：把 SVG 描边成 IR 的 `Line` /
`FillCircle`）——**不栅格化成纹理**，也不加额外依赖；小字标签在按钮下面。
默认加载仓库自带的 7 个图标（`assets/icons/`，含 Lucide 的 ISC `LICENSE`），
测试与 `--selfcheck` 因而可复现。

想看完整图标包（2112 个）：

```bash
curl -L -o lucide.tgz https://registry.npmjs.org/lucide-static/-/lucide-static-1.47.0.tgz
tar -xzf lucide.tgz package/icons
IMAGE_EDITOR_ICON_DIR=$PWD/package/icons cargo run
```

工具栏仍只用得到那 7 个名字，但 `IconPack` 会索引整包（`--selfcheck` 报告的图标数
会从 7 变成 2000+）。渲染细节见 `docs/svg.md`。
