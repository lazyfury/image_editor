# 旋转画布（视图旋转）可行性评估

状态：**已实现**（视图旋转核心 + 键盘 / 菜单 / 状态栏）。
拍板结果：只做视图态（文档像素不动）、先上键盘 / 菜单、非直角强制线性采样。
触控板两指旋转也已接上（igui `v0.4.0` 的 `InputEvent::Rotate`，与捏合同一批），
见 §5。

## 0. 结论

| 问题 | 结论 |
|---|---|
| 能画旋转后的文档吗？ | **能，且零 CPU 重采样**：`DrawList` 早就有 `SetTransform(Transform2D)`，wgpu 后端对每个顶点都乘这个矩阵（`tessellate.rs:158`），旋转只是「4 个顶点 + 一个矩阵」。 |
| 要改 igui 吗？ | **不用**（和 `docs/input-gestures.md` 的输入改动不同）。`PaintContext::set_transform` 是公开 API。 |
| 相机要动多少？ | `CanvasCamera` 加一个 `angle: f32`，5 个方法加旋转、2 个新方法，约 40 行 + 测试。 |
| 工具 / 选区 / 命中测试要改多少？ | **几乎不用改**：所有工具入口已经是「屏幕点 → 文档点」（`ui/tools/mod.rs:76`），逐点经过 `document_to_screen` 的叠加层自动正确。 |
| 真正要改的绘制 | 2 处：文档图像（`paint_document`）、像素网格（`paint_pixel_grid`）；1 处小改：方形笔光标。 |
| 工作量 | 约 **2–2.5 人日**（不含输入手势那批的上游改动）。 |
| 手感风险 | 非 90° 时最近邻会抖 → 规则：非 90° 强制线性采样，90° 倍数走现在的轴对齐快路径（像素画不退化）。 |

范围界定（重要）：**只旋转视图**（Photoshop 的 Rotate View Tool），文档像素、
图层数据、撤销历史都不动。文档像素的「旋转 90°」是另一件事，`ui/menu.rs:366`
现在还是占位。

## 1. 渲染：为什么不用 CPU 重采样

证据链（igui 仓）：

- `Transform2D { x_axis, y_axis, origin }`（`crates/core/igui_core/src/transform.rs:17`），
  带 `from_rotation` / `from_scale_rotation_origin`（:59 / :69）和 `transform_point`（:84）——
  旋转 + 缩放 + 平移是一个仿射矩阵，**不需要**特殊类型。
- `PaintContext::set_transform` / `transform`（`crates/core/igui_render/src/list.rs:171-180`）
  产出 `DrawCommand::SetTransform`，并且 `save` / `restore` 会连变换一起压栈。
- wgpu 后端：`push_vertex` 对所有顶点做 `self.state.transform.transform_point(local)`
  （`crates/platform/wgpu/igui_backend_wgpu/src/backend/tessellate.rs:158`），
  `DrawImage` 就是一个四边形（同文件 `quad`）。→ 旋转的文档 = 旋转的四边形，
  采样在 GPU 上做，每帧成本与不旋转时**完全一样**。
- `ClipRect` 是屏幕空间的 scissor（`clip_result`，同文件 :112-130），**不参与变换**。
  正好是我们要的：画布区域是屏幕矩形，旋转的是里面的内容。
- 记录后端 / `--selfcheck` 只收 `DrawList`，所以能直接断言 `SetTransform` 的命令与矩阵。

所以渲染方案是：

```rust
// paint_document：局部坐标 = 文档像素矩形，变换一次带上 zoom + angle + offset
ctx.save();
ctx.set_transform(Transform2D::from_scale_rotation_origin(
    Vec2::splat(camera.zoom),
    camera.angle,
    camera.offset,
));
ctx.draw_image(DOCUMENT_TEXTURE, Rect::from_min_size(Vec2::ZERO, doc_size), None, paint);
ctx.restore();
```

**0° / 90° / 180° / 270° 走旧路径**（轴对齐 `Rect` + `snap_to_device` + `Nearest`），
这样现有渲染逐像素不变，像素画的「1:1 对齐设备像素」也保住了。只有非直角旋转
才走上面的矩阵路径。

## 2. 相机

```rust
pub struct CanvasCamera {
    pub zoom: f32,
    pub angle: f32,        // 弧度，归一化到 (-π, π]
    pub offset: Vec2,      // 文档原点在屏幕上的位置
}

screen = offset + R(angle) · (document · zoom)
document = R(-angle) · (screen - offset) / zoom
```

| 方法 | 变化 |
|---|---|
| `document_to_screen` / `screen_to_document` | 各加一次旋转 / 逆旋转（4 行） |
| `zoom_at(screen, factor)` | 先用逆变换取锚点，再 `offset = screen - R·(anchor·zoom')`（公式同现在，多一个 R） |
| `pan_by` | 不变（`offset` 是屏幕量） |
| `centered(area, doc, zoom)` | 改成「文档中心落在 area 中心」：`offset = area.center() - R·(doc/2·zoom)`，与角度无关，比现在更简单 |
| `fit(area, doc, padding)` | 用**旋转后的 AABB** 求 zoom（`w' = |cos|w + |sin|h`，`h' = |sin|w + |cos|h`），再 `centered` |
| `rotate_to(angle)` / `rotate_by(delta)` | 新方法：绕画布中心转。用户转手势时报告不了「两指中心」，绕中心最稳，不会漂 |
| `is_upright()` | 角度 ≈ 0（±1e-4）→ 只有它走轴对齐快路径（设备像素对齐只在正着看时有定义） |
| `is_right_angle(angle)` | 角度 ≈ 90° 的整数倍 → 决定采样（直角最近邻仍然精确） |
| `to_transform()` | 绘制用的仿射矩阵（和 `document_to_screen` 同一定义，测试对着 `transform_point` 核对） |

`Default` 的 `angle = 0` → 现有所有相机测试、`--selfcheck` 的场景全部不用改。

## 3. 绘制：谁要改，谁自动就对

| 绘制 / 逻辑 | 结论 | 依据 |
|---|---|---|
| 文档图像 `paint_document`（`ui/paint.rs:57`） | **改**：矩阵路径 + 轴对齐快路径 | 现在用 `document_to_screen(ZERO)` 和 `(w,h)` 取 AABB，旋转后会画成轴对齐 |
| 像素网格 `paint_pixel_grid`（`ui/paint.rs:84`） | **改**：网格线两端都过相机；把画布区域四角逆变换成文档 AABB 做裁剪 | 现在是 `origin + x*zoom` 的轴对齐假设，且用 AABB 裁 |
| 笔刷光标圆（`ui/paint.rs:163`） | 不用改 | 圆在文档空间旋转不变 |
| 笔刷光标的**方形**笔尖 | **小改**：方框按 `angle` 转（`Transform2D::from_rotation_origin`） | 现在画 `Rect::from_center_size`（屏幕轴对齐） |
| 选区描边 `paint_selection`（`ui/paint.rs:197`） | 自动正确 | 轮廓在文档像素坐标生成，逐点 `document_to_screen` |
| 套索 / 钢笔 / 矩形填充预览 / 渐变轴（`ui/paint.rs:207/272/286`） | 自动正确 | 同上，逐点换算 |
| 棋盘格 | 跟着图转 | 已烘进显示纹理（`canvas/checkerboard.rs`）；现在本来就跟着缩放走，属于已知偏差 |
| 所有工具 / 命中测试 | 自动正确 | `begin_tool` / `continue_tool` / `end_tool` 都先 `document_position(screen)`（`ui/tools/mod.rs:76-130`）→ `screen_to_document` |
| 移动工具 | 自动正确 | 记的是**文档坐标**增量（`tools/move_tool.rs:38-55`） |
| 状态栏像素坐标、魔棒、油漆桶、渐变 | 自动正确 | 全部走 `screen_to_document` |
| `snap_to_device`（`ui/paint.rs:20`） | 只在轴对齐快路径用 | 旋转后「对齐设备像素网格」没有定义 |
| `ImageSmoothing::for_zoom`（`ui/sync.rs:35`） | **改**：加角度 | 见 §4 |

## 4. 采样与像素画

- 非直角旋转 + `Nearest` = 边缘抖动 / 摩尔纹（GPU 只做 4 点采样，没有 mipmap）。
  规则：**`camera.is_axis_aligned()` 才允许 `Nearest`，否则强制 `Linear`**。
- 直角旋转（0/90/180/270）走轴对齐快路径，`Nearest` + 设备像素对齐全部保留 →
  像素画用户转 90° 看正，画面跟不旋转一样锐。
- 显示纹理是单级的，缩小 + 旋转本来就有摩尔纹；这次不引入 mipmap（那是另一个工作量）。
- 触控板捏合在旋转视图里依旧绕指针锚定，`zoom_at` 已经和角度解耦。

## 5. 交互：怎么转

| 操作 | 效果 |
|---|---|
| 触控板两指旋转（macOS `RotationGesture`） | 连续旋转，绕画布中心；单事件 0.2° 死区，接近 15° 倍数 ±2° 时吸附 |
| `T` / `Shift+T` | 顺时针 / 逆时针 15°（鼠标用户的路；`R` 已被矩形填充工具占用） |
| 视图菜单「旋转视图 +15° / −15° / 重置旋转」 | 同键盘；`menu_spec` 一张表，原生菜单与自绘菜单同时生效 |
| `0`（100% 并居中） | **同时**把角度归零（和 Photoshop 的 Reset View 一致） |
| `F`（适配） | 保留当前角度，用旋转后的 AABB 适配 |

实现路径：

- `RotationGesture` 在 winit 0.30 macOS 上可用（`platform_impl/macos/view.rs:736-751`，
  角度单位是度），但 `igui_winit` 没有转发 —— 和捏合**同一批**补
  （`docs/input-gestures.md` 的路线 A 顺手加 `InputEvent::Rotate`，路线 B 就在
  `src/app/gestures.rs` 里多一个分支）。**没有手势也能先做核心**：
  camera + 绘制 + 键盘/菜单，触控板旋转最后接。
- 状态栏右半边加一个「角度」文案（`ui/sync.rs` 的 `StatusSnapshot` 已有这套机制），
  非 0 时显示 `↻ 15°`，让人知道当前是转的。

## 6. 与现有系统的关系

- **撤销 / 历史**：视图旋转不进撤销栈（和缩放 / 平移一样，`AppState.canvas` 不在
  文档里）。
- **保存**：`.igui` 不存相机状态（grep `io/` 无 camera），所以不用加字段；打开时
  照旧 `fit` 且 `angle = 0`。
- **适配 / 居中**：`ui/input.rs:164/181` 两个函数跟着 `CanvasCamera` 新签名走。
- **窗口缩放 / 侧栏拖拽**：相机不动，只是裁剪区域变，天然正确。
- **性能**：4 个顶点乘一个矩阵，零 CPU 重采样；像素网格的命令数在旋转后最多
  ×√2（用文档空间 AABB 裁剪，仍有界）。

## 7. 测试与验收（只做程序化验证）

已落地（名字就是代码里的测试名）：

- 相机（`src/canvas/camera.rs`）：`rotation_round_trips_and_matches_the_draw_transform`、
  `zooming_keeps_the_anchor_fixed_at_an_angle`、`rotating_keeps_the_anchor_fixed`、
  `fit_accounts_for_the_rotated_bounds`（45° 的 zoom = 直角的 1/√2）、
  `right_angles_are_axis_aligned_but_free_angles_are_not`、`angle_is_normalized`、
  `rotated_centering_puts_the_document_center_on_the_area_center`；
  采样规则并进 `smoothing_is_nearest_when_magnifying_and_linear_when_minifying`。
- 视图 / 绘制（`src/ui/tests/canvas.rs`）：`a_rotated_view_draws_the_document_through_one_transform`
  （30° 发出一条等于相机矩阵的 `SetTransform`，`DrawImage` 用局部坐标）、
  `an_upright_view_keeps_the_axis_aligned_path`（0° 没有非单位变换）、
  `the_pixel_grid_follows_the_rotation`（网格端点逆变换回文档空间后落在整数坐标、
  线数有界）、`painting_through_a_rotated_camera_hits_the_expected_pixel`（30° 点击命中
  (64,64) 且相邻像素不变 — 命中测试与绘制同源）、
  `the_rotation_shortcuts_step_and_reset_the_view`（`T`/`Shift+T`/`0`）。
- `--selfcheck`：新增 `checks/rotation.rs`（0° 无变换 → 30° 有相机矩阵 → 点击命中
  正确像素 → `0` 归零）。

不做截图（`AGENTS.md` 硬规则 1）。人手验收只在真机上确认「转起来手感如何」，
这一点没法程序化，写进 PR 说明。

## 8. 工作量与风险

| 项 | 估时 | 说明 |
|---|---|---|
| 相机 + 相机测试 | 0.5d | 纯数学，测试最容易写 |
| 文档图像矩阵路径 + 采样规则 | 0.25d | 一次 `set_transform` |
| 像素网格 | 0.5d | 需要重写裁剪 + 线段端点 |
| 方笔光标 + `snap_to_device` 守卫 | 0.25d | 小 |
| 键盘 / 菜单 / 状态栏 | 0.25d | 菜单表加三项 |
| `RotationGesture` 接线 | 0.5d | 依赖 `input-gestures` 的路线选择 |
| 测试 + `--selfcheck` 场景 | 0.5d | |

风险（按严重度）：

1. **像素画观感**：非直角旋转 + 线性采样会把硬边糊掉。这是物理限制（没有 mipmap /
   supersampling）。缓解：直角快路径 + 角度吸附，让像素画用户实际只会停在 90° 倍数。
2. **网格命令数**：极端角度下文档可见 AABB 变大，线数 ×√2；已经有「只画交集」的
   上限，可接受。
3. **旋转手势误触**：两指滚动时轻微旋转会被 macOS 判成 rotate。缓解：死区 5°、
   吸附、只在画布区域响应。
4. **棋盘格跟着转**：视觉上和 Photoshop 不一致（它是屏幕空间），但现状已经「跟着
   缩放走」，属于同一笔历史债；要么以后改成屏幕空间叠加，要么接受。
5. **和输入手势那批的上游改动耦合**：`Pinch` / `Rotate` 一起进 igui 最省事，否则
   旋转要等第二批。

## 9. 明确不做

- **不旋转文档像素**（破坏性）：`ui/menu.rs:366` 的「旋转 90°」是另一件事（连同
  `水平翻转` / `垂直翻转` 占位）；做完视图旋转后它们互不干扰。
- **不做任意角度的图层变换**：那是 `todo.md` 第 4 批「自由变换」的范围。
- **不做透视 / 斜切视图**：矩阵支持，但没有交互需求，也没有 UI 表达。
- **可选一行彩蛋**：视图水平 / 垂直翻转 = `scale ±1`，命中测试与叠加层会自动正确；
  但翻转会让「画笔画的方向」和鼠标方向相反，容易吓到人，本期不做。

## 10. 落地顺序（前 4 步已完成）

1. [x] **相机**（含测试）——纯函数，零风险；
2. [x] **绘制**：`paint_document` 矩阵路径 + 0° 轴对齐快路径 + 采样规则；
3. [x] **像素网格 + 方笔光标**；
4. [x] **键盘 / 菜单 / 状态栏**（`T`/`Shift+T`、视图菜单三项、状态栏 `↻ 15°`）；
5. [x] **触控板旋转手势**：`InputEvent::Rotate`（度、顺时针为正）→ `gesture_rotate`：
   绕画布中心、0.2° 死区、15° 吸附（±2°）；测试
   `the_rotation_gesture_snaps_but_the_key_steps_exactly`；
6. [x] `--selfcheck` 场景 + README 补表格 + `todo.md`。

实现与评估的两处偏差（都是有意的）：

- 评估里写「90° 倍数也走轴对齐快路径」，实际只有 **0°** 走：90°/180°/270° 的 AABB
  没法直接 `DrawImage`（图像会正向画进同一个矩形，等于没转），所以它们走
  `SetTransform`。90° 的采样仍然逐像素精确，只是不走「设备像素对齐」那一步。
- 评估里的 `is_axis_aligned()` 拆成了 `is_upright()`（绘制快路径）和
  `is_right_angle(angle)`（采样规则），两个判断的用途不同，混在一起会让
  「90° 要不要对齐设备像素」变得没有答案。
