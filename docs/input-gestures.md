# macOS 触控板 / 鼠标：设备区分与画布导航设计

状态：**已实现**（igui `v0.4.0` + 本仓接线）。拍板结果：滚轮保持缩放、走路线 A
（上游事件带设备 + 相位）、`Cmd`/`Ctrl`+滚动不额外做（捏合已覆盖）、面板滚轮
交给 `igui_ui`。触控板两指旋转同批接上（见 `docs/canvas-rotation.md` §5）。

## 0. 结论先行

**能区分，而且不用写 FFI。** macOS 通过 winit 0.30 已经给到三样东西：

| 信号 | 从哪来 | 说明什么 |
|---|---|---|
| `MouseScrollDelta::PixelDelta` / `LineDelta` | `NSEvent.hasPreciseScrollingDeltas` | 精确像素 = 触控板 / Magic Mouse 这类「滑面」设备；行增量 = 带刻度的滚轮 |
| `TouchPhase`（`Started` / `Moved` / `Ended` / `Cancelled`） | `NSEvent.phase` + `momentumPhase` | 触控板一次手势有起止（惯性滚动也算同一手势的延续）；老式滚轮永远只有 `Moved` |
| `WindowEvent::PinchGesture { delta, phase }` | `magnifyWithEvent:` | 两指捏合；macOS 独有 |

问题不在「能不能拿到」，而在**我们的管线把它们全丢了**：

- `igui_winit` 的 `PointerPlugin` 把 `MouseWheel` 压成 `InputEvent::Wheel { position, delta: Vec2::new(0.0, y) }`（`crates/window/igui_winit/src/pointer.rs:66`）——**丢掉 x、phase、设备**，且 `LineDelta` 直接乘 24 当像素。
- `PinchGesture` 没有被任何插件转发，今天完全不存在（`grep Pinch crates/` 为空）。
- `EditorView::event` 对 `Wheel` 的处理是「在画布区域里就缩放」（`src/ui/input.rs:73`），不看设备、不看方向、不看幅度。

于是今天的真实体验是：**触控板两指一划 = 每个事件放大/缩小 1.25 倍（含惯性阶段），而捏合什么都不做，横向滚动丢失。**

设计目标（按优先级）：

1. 触控板两指滚动 = **平移**画布（内容跟手），横向也能用；
2. 触控板捏合 = **缩放**，锚定指针（指针不在画布内时锚定画布中心）；
3. 滚轮保持现有语义（缩放），不回归 README 里写过的行为；
4. 惯性滚动（momentum）继续平移，不中途「换设备」变成缩放；
5. 语义只在**一处**定义：设备判定 + 相位，视图按事件分流，不做平台猜测。

落地后的实际分工（先看这里再看设计）：

- `igui_core`：`InputEvent::Wheel` 带 `device: ScrollDevice` / `phase: TouchPhase`；
  新增 `InputEvent::Pinch { position, factor, phase }`（factor = 1 + 放大率）与
  `InputEvent::Rotate { position, delta(度, 顺时针为正), phase }`。
- `igui_winit`：`input::wheel` 返回 `(Vec2, ScrollDevice)`（像素级 = `Trackpad`，
  行增量 = `Wheel`，x 不再被丢掉），`input::touch_phase` 映射相位；`PointerPlugin`
  顺手转发 `PinchGesture` / `RotationGesture`（非有限 / 0 增量丢弃）。
- 本仓 `src/ui/input.rs`：`Wheel` 按 `device` 分流（触控板平移 / 滚轮缩放，
  `Shift` 换轴），`Pinch` 描指针或画布中心，`Rotate` 绕画布中心 + 15° 吸附。
  设计稿里的「应用内 observer + 意图队列」（路线 B）没有落地 —— 不需要。

---

## 1. 现状（证据链）

```text
NSEvent(scrollWheel/magnify)
  -> winit WindowEvent::{MouseWheel, PinchGesture}     # 信息完整
  -> igui_winit::PointerPlugin                          # 只剩 y 像素
  -> igui_core::InputEvent::Wheel { position, delta }    # 无设备 / 无相位 / 无 x
  -> ImageEditorLogic::route_event
  -> EditorView::event(Wheel) => canvas.zoom_at          # 无脑缩放
```

附带事实（会决定方案选择）：

- `igui_ui::handle_input` 的 `Wheel` 只交给「指针下最近一个注册了 scroll 回调的祖先」，没人认领就返回 `Ignored`（`crates/core/igui_ui/src/input.rs:240`）——所以画布之外的滚轮能正常给面板列表用，我们的改动**只该影响画布**。
- `igui_app::PlatformObserver::on_platform` **只能追加**输入事件，不能吞掉；想「过滤掉 `igui_winit` 生成的那个 Wheel」，只能靠输入层（`InputLayer`）或让视图自己忽略。
- `App::platform_event` 是「先跑完所有 observer 收集事件，再逐条 dispatch」，所以 app 侧 observer 写的分类结果，在 dispatch 时一定已经就绪。
- `igui_winit` 暴露 `SharedWindowState`（`scale_factor`、`cursor` 都是 pub），PixelDelta 是**物理像素**，app 侧要除 `scale_factor`。
- `--selfcheck` / `ui/tests` 目前只用裸 `InputEvent::Wheel` 验缩放语义（`src/ui/tests/canvas.rs:133`），不涉及设备。

---

## 2. 平台能力对照

| 事件源 | winit 变体 | macOS 独有 | 现有转发 |
|---|---|---|---|
| 滚轮（带刻度） | `MouseWheel { LineDelta }` + `Moved` | 否 | 有（丢相位/x） |
| 触控板两指滚动 | `MouseWheel { PixelDelta }` + `Started/Moved/Ended` | 否 | 有（丢相位/x） |
| 惯性滚动 | `MouseWheel { PixelDelta }` + 由 `momentumPhase` 推出的相位 | 否 | 有（丢相位） |
| 捏合缩放 | `PinchGesture` | 是 | **无** |
| 智能缩放（双击） | `DoubleTapGesture` | 是 | 无 |
| 两指旋转 | `RotationGesture { delta(度), phase }` | 是 | 无 |
| 压感 | `TouchpadPressure` | 是 | 无 |

本设计用前两行 + 捏合；旋转/智能缩放记在 §11 的后续项（旋转是「转画布视图」不是转文档，和 AGENTS「2D 栅格编辑」不冲突但也不急）。

**已知不可判定的两处**，写清楚免得以后当 bug 修：

1. **Magic Mouse 与触控板无法区分**：Magic Mouse 也 `hasPreciseScrollingDeltas = true`、也发手势相位。两者都归入「gesture 设备」→ 平移。这是可接受的（Magic Mouse 本来就没有滚轮刻度），真要区分得读 `NSEvent` 的 `deviceID` / `CGEventSource`，且没有公开稳定依据。
2. **高分辨率滚轮**（部分游戏鼠标发像素级 delta）也会被判成 gesture 设备 → 平移。降级策略是「宁可变平移，也不要缩放风暴」，并且 §9 留了一个偏好开关兜底。

---

## 3. 分类规则

### 3.1 单事件 → 语义

```rust
// igui_core::input（路线 A 的新类型）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDevice { Wheel, Trackpad }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchPhase { Started, Moved, Ended, Cancelled }
```

判定顺序（纯函数，无状态部分）：

| 原生事件 | 结果 |
|---|---|
| `PinchGesture { phase: Started..Ended, delta }` | `Pinch { factor: 1.0 + delta, phase }` |
| `MouseWheel { LineDelta(..) }` | `Scroll { device: Wheel, phase }` |
| `MouseWheel { PixelDelta(p) }` | `Scroll { device: Trackpad, phase }`（保守，见 §2 注 2） |
| `MouseWheel { PixelDelta, phase: Moved }` 且没有打开中的手势 | `Scroll { device: Trackpad }`（像素级 + 无相位 → 也只有滑面设备会这样） |

`TouchPhase::Cancelled` 只用于**关闭**手势，不产生位移。

### 3.2 手势锁存（防惯性阶段换设备）

状态机很小，但它保证「一次手势只有一种语义」：

```text
Idle --Started--> Active(device)     # 记住 device
Active --Moved----> Active(device)   # 沿用锁存的 device，不看本事件的 delta 类型
Active --Ended---> Idle              # 惯性滚动随后会再来一个 Started，从而重新打开（同 device）
Active --Cancelled--> Idle           # 丢弃，不产生位移
```

没有锁存就会出现这个 bug：手指抬起后 momentum 事件在某些机器上只带 `Moved`，若按「无相位 + PixelDelta」重新判定还好，但一旦某台机器把惯性报成 `LineDelta`，画面会在惯性阶段突然从平移变成缩放。

### 3.3 与修饰键组合

修饰键不进分类器（分类器只认设备/相位），组合在视图/宿主层做：

| 手势 | 无修饰 | `Shift` | `Cmd` / `Ctrl` |
|---|---|---|---|
| 触控板滚动 | 平移（双向） | 只取主方向 → 水平平移 | 以指针为锚缩放 |
| 滚轮 | 缩放（±25%/格） | 水平平移 | 缩放（同左） |
| 捏合 | 缩放 | 缩放 | 缩放 |
| 中键拖拽 | 平移 | — | — |

理由：`Cmd`+滚动在 macOS 上是事实标准（浏览器 / Figma / Sketch 都是缩放），`Shift` 在滚动里是事实标准的「换轴」。捏合本身已经是缩放手势，修饰键不改语义。

---

## 4. 视图 API（两条路线共用）

把「平台语义」挡在视图外面，`EditorView` 只暴露三个意图，全部无设备概念：

```rust
// src/ui/input.rs（新增 pub(crate) 方法，`EditorView`）
/// 以屏幕逻辑像素平移画布（正数 = 内容向右 / 向下移动，即相机反向）。
pub fn pan_canvas(&mut self, delta: Vec2) -> EventResult;

/// 以 `position` 为锚缩放；`None` = 画布中心（`zoom_by` 现有规则）。
pub fn zoom_canvas_at(&mut self, position: Option<Vec2>, factor: f32) -> EventResult;

/// 捏合：`factor = 1 + magnification`，锚点规则同上。
pub fn pinch_canvas(&mut self, factor: f32) -> EventResult;
```

三条都做同一件事：**命中测试 `canvas_area_contains` / 指针是否在画布内**，不在画布内就返回 `Ignored`（让滚轮继续落到面板）；然后 `state.borrow_mut().canvas.pan_by / zoom_at`。

`CanvasCamera` 不用改：`pan_by` / `zoom_at` 已经够用。

现状里 `EditorView::event` 的 `Wheel` 分支（`src/ui/input.rs:73-85`）**保留但只作降级**：没有宿主分类器时（自检、单元测试、将来的其它宿主）仍然是「滚轮 = 缩放」。装了分类器的宿主不会把裸 `Wheel` 再交给视图当导航用（见 §5 两路线各自的接法）。

---

## 5. 架构：两条路线，推荐 A

### 5.1 路线 A（推荐）：把「设备 + 相位 + 双轴」补进 igui

`igui_core`：

```rust
pub enum InputEvent {
    Wheel { position: Vec2, delta: Vec2, phase: TouchPhase, device: ScrollDevice },
    Pinch { position: Vec2, factor: f32, phase: TouchPhase },
    // ...
}
```

`igui_winit`（`crates/window/igui_winit/src/pointer.rs`）：

- `input::wheel(delta, scale) -> (Vec2 /*x, y 都留*/, ScrollDevice)`；符号约定**不变**（`y > 0` = 内容向上滚 / 滚动偏移增加），x 按同样规则转换；
- 把 winit 的 `TouchPhase` 直接传下去；
- 新增 `PinchObserver` 分支：`PinchGesture` → `InputEvent::Pinch { position: state.cursor, factor: 1.0 + delta as f32, phase }`；
- `docs/components.md:458` 那段「host 的职责」说明同步更新。

app 侧改动（很小）：

- `src/ui/input.rs`：`Wheel` 分支按 `device` + 修饰键分流到 §4 的三个意图；新增 `Pinch` 分支 → `pinch_canvas`；
- `src/app/application/mod.rs`：不用加插件、不用加共享队列；
- 因为走的是同一条输入管线，**捏合和滚动都在同一个原生事件里立即生效**，没有一帧延迟，面板里的 `List` / `ScrollView` 也顺带拿到了正确的 x 与相位。

代价：要动 igui 仓、过它自己的 gate、把 `Cargo.toml` 的 tag 从 `v0.3.0` 升到新 tag。按 `AGENTS.md`，共享 UI 层改动本来就该在上游做，这是「正规路径」。

### 5.2 路线 B（过渡/不想发版时）：应用内观察者 + 意图队列

新增 `src/app/gestures.rs`（和 `drop_files.rs` 同款，`PlatformObserver` 直接 downcast `winit::event::WindowEvent`）：

```rust
pub enum CanvasGesture {
    Scroll { position: Vec2, delta: Vec2, device: ScrollDevice },
    Pinch { factor: f32, phase: TouchPhase },
}

pub struct CanvasGestures(Rc<RefCell<VecDeque<CanvasGesture>>>);   // 注册成 service

pub struct GesturePlugin;   // 在 `.plugin(PointerPlugin)` 之后注册
```

- observer 负责 §3 的分类 + 锁存 + 物理→逻辑像素（`SharedWindowState::scale_factor`），把结果压进队列；
- 宿主在 `ImageEditorLogic::route_event` 的**入口**（模态检查通过之后、转发给视图之前）把队列排空 → `editor.pan_canvas / zoom_canvas_at / pinch_canvas`；
- 捏合没有对应的核心事件，所以 `AppLogic::update` 里也要排空一次（最多一帧延迟，16ms，肉眼不可见）；
- 视图的 `Wheel` 分支在宿主接管后**不再参与导航**（回到 `Ignored`），否则同一个原生事件会「既平移又缩放」。

代价：多一层队列、分类逻辑长在 app 里（其它宿主拿不到），还要处理「谁来负责吞掉那个已经生成的 `InputEvent::Wheel`」。好处：**不动 igui，今天就能上**，而且分类器是纯函数、单测很好写，将来搬到 `igui_winit` 是逐行搬。

### 5.3 为什么不直接 FFI `NSEvent`

- winit 已经把我们需要的三个信号（`hasPreciseScrollingDeltas`、`phase/momentumPhase`、`magnification`）翻译出来了，规格写在 `src/platform_impl/macos/view.rs:660-750`；
- 自己挂 `NSEvent` 监听要处理响应链、`acceptsFirstResponder`、以及与 winit 视图的事件竞争，还得 `unsafe` + 版本耦合；
- 只有「旋转 / 智能缩放 / `isDirectionInvertedFromDevice`」这类 winit 没给的细节才值得 FFI，本设计不需要。

---

## 6. 数值与手感

| 参数 | 值 | 理由 |
|---|---|---|
| 触控板平移增益 | `1.0`（逻辑像素 1:1） | macOS 已经做过滚动加速，再乘系数会「飘」 |
| 惯性 | 不过滤、不衰减 | 系统给的 momentum delta 本身就是衰减曲线；过滤会让惯性「断掉」 |
| 平移方向 | `camera.pan_by(-delta)`（igui 约定 `y>0` = 内容上移） | 两指上推 → 画面内容跟着上走；符号只在这一处写，配一条单测 |
| 捏合系数 | `factor = 1 + magnification`，单事件钳到 `[0.5, 2.0]` | 防 `NaN` / 极端事件把 `zoom` 一步顶到 64×；相机本身还会 clamp 到 `[0.05, 64]` |
| 捏合锚点 | 指针在画布内 → 指针；否则画布中心 | 触控板上「指针」只是个残影，画面外捏合还锚在指针会把内容甩出去 |
| 滚轮缩放步进 | 保持 `ZOOM_STEP = 1.25` / 格（现有） | 不回归测试与 README |
| `Cmd`+滚动缩放 | 同上 1.25/格（滚轮）、触控板按 delta 连续 | 与 Figma/Sketch 手感一致 |
| 最小事件 delta | 不设死区 | 死区会让慢速微调（大画布对位）丢帧 |

可选（不在本期）：缩放平滑（对 `zoom` 做指数趋近，让捏合更「顺」）；触控板平移做 60Hz 合并。两项都要先有真机手感数据再定。

---

## 7. 边界情况

1. **模态/浮层**：`route_event` 里模态、确认框、弹出窗口都在手势排空**之前**返回 `Handled`，所以它们开着时画布不会被动 —— 保持现状即可。
2. **指针不在画布**：滚动交给面板（`Ignored`），捏合缩放到画布中心（不吞面板的滚轮）。若觉得「指针在面板上捏合也缩放」奇怪，改成 `Ignored` 只需一行。
3. **手势中途离开画布区域**：以 `Started` 时的命中为准（锁存），否则会把画布停在半路；也就是分类器顺带锁存「这次手势归谁」。
4. **全屏/切换工作区**：手势队列在切换时清空，避免旧 delta 落到新工作区。
5. **device 热切换**（同时接鼠标和触控板）：按事件逐个判定 + `Started` 重开手势，天然支持。
6. **非 macOS**：`PixelDelta` 在 X11/Wayland 上通常也是触控板，规则同样成立；`PinchGesture` 不会出现，只是死分支。用 `#[cfg]` 封得越少越好（单测能在 Linux CI 上跑分类器）。
7. **`--selfcheck` / `ui/tests`**：不装分类器 → 裸 `Wheel` 仍是缩放，现有测试语义不变。
8. **`NaN` / 零 delta**：分类器丢弃 `!delta.is_finite()`；零 delta 直接不产生意图（避免空命令进历史/重绘）。

---

## 8. 决策点（已定）

1. **滚轮语义**：保持「滚轮 = 缩放」（现状、README、测试都不回归）；触控板滚动才是平移。
2. **路线**：A —— igui 升 `v0.4.0`，事件自带设备 / 相位 / 手势；B 的 app 内 observer 没有落地。
3. **`Cmd`/`Ctrl`+滚动缩放**：没做。触控板用捏合缩放，滚轮本身就是缩放，再叠一套修饰键只是多一条路径。
4. **面板滚轮**：保持交给 `igui_ui`（指针不在画布上就落回原路由），图层 / 历史列表的滚动行为不变。

---

## 9. 测试与验收

只做程序化验证（`AGENTS.md` 硬规则 1）。**已落地的测试**：

- `igui_winit::input`：`a_wheel_up_scrolls_up_on_a_detented_wheel`、
  `pixel_deltas_are_logical_and_mean_a_trackpad`、`gesture_phases_map_one_to_one`；
  `igui_winit::pointer`：`a_precise_wheel_means_trackpad_and_keeps_the_phase`、
  `pinch_and_rotate_forward_as_gestures`（含旋转符号翻转）。
- 本仓视图级：`a_trackpad_scroll_pans_while_a_wheel_still_zooms`、
  `shift_scroll_swaps_the_axis`、`a_scroll_outside_the_canvas_leaves_the_camera_alone`、
  `a_pinch_zooms_around_the_pointer_and_clamps_one_event`、
  `a_pinch_outside_the_canvas_anchors_on_the_canvas_center`、
  `the_rotation_gesture_snaps_but_the_key_steps_exactly`。
- 宿主级：`gesture_events_reach_the_active_workspace_camera`（只盯接线）。

下面是设计时列的验收清单，保留作参考。

**纯函数（分类器）**，路线 A 落在 `igui_winit::input`（和 `wheel_pixels` 同处），路线 B 落在 `src/app/gestures.rs`：

- `line_delta_is_a_wheel`：`LineDelta` → `Wheel`；
- `precise_delta_is_a_trackpad`：`PixelDelta` → `Trackpad`；
- `started_latches_the_device_until_ended`：`Trackpad Started` → 中间来一条 `LineDelta Moved` → 仍是 `Trackpad`；`Ended` 之后 `LineDelta` 才回到 `Wheel`；
- `momentum_reopens_the_same_gesture`：`Ended` 之后再来 `Started` 的像素 delta → 仍归 `Trackpad`；
- `cancelled_scroll_produces_no_motion`；
- `pinch_factor_is_one_plus_magnification`（含 `NaN` 丢弃、单事件钳制）。

**相机 / 视图**（`src/canvas/camera.rs`、`src/ui/tests/canvas.rs`）：

- `two_finger_scroll_pans_the_canvas`：`pan_canvas(Vec2::new(0.0, -20.0))` 后 `offset.y` 减 20、`zoom` 不变；
- `scroll_outside_the_canvas_is_ignored`：指针在面板上 → `Ignored`、相机不变（这是「面板滚动不被抢」的规则）；
- `pinch_zooms_around_the_pointer`：锚点下的文档点在捏合前后不变；
- `pinch_outside_the_canvas_anchors_on_the_center`；
- `pinch_factor_is_clamped`：给 1e9 → `MAX_ZOOM`，给 0 → `MIN_ZOOM`（复用现有 clamp 测试风格）。

**集成**：

- `--selfcheck` 全绿（现状无 `Wheel` 检查，所以只验证不回归）；
- 宿主单测：造一条 `winit::event::WindowEvent::MouseWheel` 塞给 observer，断言队列里出现正确的 `CanvasGesture`（路线 B），或 `PointerObserver` 产出带 `device` 的 `InputEvent`（路线 A）；
- **真机手测清单**（无法程序化，写进 PR 说明）：两指平移（横/竖）→ 惯性延续 → 捏合缩放锚点 → 滚轮仍是缩放 → 在图层列表上两指滚动 → 面板上捏合。

调试手段：分类结果打一条 `tracing::debug!(target: "image_editor", device = ?d, phase = ?p, "scroll")`，真机上用 `RUST_LOG` 打开即可核对，不依赖截图。

---

## 10. 分步落地（全部完成）

1. [x] igui `v0.4.0`：`ScrollDevice` / `TouchPhase` / `Wheel{device,phase}` /
   `Pinch` / `Rotate` + `igui_winit` 设备与相位映射、x 保留、手势转发。
2. [x] 本仓分流：`src/ui/input.rs`（`Wheel` 按 device、`Pinch`、`Rotate`）。
3. [x] 旋转手势的 15° 吸附 / 0.2° 死区（`gesture_rotate`）。
4. [x] 测试：igui 单测 + 视图单测 + 宿主接线测试。
5. [x] 文档：README 操作表、`todo.md`、igui `release.md` / `docs/guide/components.md`。

### 10.1 工具入口（抓手 / 缩放 / 空格）

同一套相机动作，除了手势之外再给三个「工具入口」（不改上面任何手势语义）：

| 入口 | 行为 | 说明 |
|---|---|---|
| 抓手工具 `H` | 左键拖动平移 | 和中键 / 触控板共用 `ViewportNav.pan` |
| 缩放工具 `Z` | 点击放大一档；`Option/Alt` + 点击缩小 | 以指针为锚，复用 `CanvasCamera::zoom_at` |
| 按住 `Space` | 临时抓手 | **不切换** `active_tool`，松开空格 / 鼠标即恢复 |

路由顺序：`input.rs` 里视图导航先于 `begin_tool`，所以蒙版 / 图层组不会拦它，
也不会产生提示；两个工具都不写文档、不进撤销。实现集中在 `src/ui/viewport.rs`，
`pan_last` 已重整为 `ViewportNav`（中键 / 抓手 / 空格三个来源）。

### 10.2 系统光标（已实现）

画布上的光标由 `src/app/cursor.rs` 映射、`application/cursor.rs` 每帧通过
`SharedWindow` 设给 winit（变了才 `set_cursor`）：

| 位置 / 状态 | 光标 |
|---|---|
| 平移中（中键 / 抓手 / 空格） | `Grabbing` |
| 按住空格 / 抓手待命 | `Grab` |
| 缩放工具（画布内） | `ZoomIn`；`Option` → `ZoomOut` |
| 画笔 / 橡皮 / 选区 / 钢笔 / 填充 / 吸管 | `Crosshair` |
| 移动 | `Move` |
| 面板 / 标签条 / 对话框 | igui `hovered_cursor` → `Pointer` / `Text` / Resize |

不在本期：隐藏系统光标 + 自绘笔刷轮廓、任意位图 `CustomCursor`（需要上游
igui 暴露 event loop / cursor 服务）。

---

## 11. 后续（不在本期）

- `RotationGesture` → 视图旋转（只转相机，不动文档像素）：可行性与做法已单独评估，
  见 **`docs/canvas-rotation.md`**（不改 igui，用 `SetTransform` 画旋转的文档四边形）；
- `DoubleTapGesture`（智能缩放）→ 「适配 / 100%」二态切换；
- 偏好里的 `pointer.scroll_pan=0/1`、`pointer.zoom_with_cmd=0/1`，给「高分辨率滚轮」和「我就想滚轮平移」的人一个出口（`src/io/preferences.rs` 已有 `key=value` 机制，加两个 bool 很便宜）；
- 触控板平移的 60Hz 合并 / 缩放平滑（等真机手感数据）。
