# AGENTS.md — image_editor

A standalone, Photoshop-style 2D image editor whose entire UI is drawn by the
**igui** stack (formerly quill) (`igui_core` / `igui_render` / `igui_scene` /
`igui_theme` / `igui_ui` / `igui_components` / `igui_svg` /
`igui_backend_wgpu`). No `egui`, no DOM.

This file is the short source of truth for rules. Details live in `README.md`
(features, phases) and `todo.md` (project backlog).

## Where things are

```
src/app/        state (AppState/document session) + workspaces (multi-workspace) +
                single-window host on the igui_app runtime + igui_winit plugins +
                drop_files (drag-and-drop -> new layer).
                `application/` holds the host: mod.rs (Options / run / build_app
                + ImageEditorLogic + AppLogic), files.rs (save/open/import/
                export/close/drop), shortcuts.rs, plugins.rs (text measure /
                frame limit).
src/ui/         views + project-local components: home (recent gallery),
                new_document, workspace_tabs, menu, toolbar, options_bar, canvas,
                layer/file/properties/tabs/palette panels, rename (TextInput
                popover), window (floating window), confirm (unsaved-changes
                dialog).
                `ui/mod.rs` is `EditorView` (fields + `new` + `update`); the rest
                of it is split by concern: accessors / paint / tools / sync /
                menus / input / layout / io / tests (each a sibling `ui/*.rs`).
                `ui/build/` is the constructor: mod.rs (Wiring + `editor`),
                tree.rs (panels + scene tree), assemble.rs (NodeRef -> EditorView).
src/document/   Document / Layer / PixelBuffer / Color / Gradient / History + commands
src/renderer/   CPU compositor (layers -> one PixelBuffer)
src/tools/      Brush/Eraser (in `brush/`: mod = tool + stroke, tip = brush-tip
                geometry/hardness, mask = per-stroke coverage mask + compositing,
                preset = named parameter sets), Pen (`pen.rs`: multi-click bezier
                path -> selection / fill / stroke), Move, Paint-bucket flood fill,
                Magic wand (tolerance region -> selection), Gradient (multi-stop
                ramp), Rectangle/Free fill; `stroke.rs` = smoothing + fixed-distance
                sampling
src/canvas/     camera (zoom/pan), coordinate conversion, checkerboard,
                Path (bezier anchors + handles, adaptive flatten), Selection
                (pixel mask + outline: rect / lasso / wand)
src/io/         PNG encode/decode + `.igui` workspace files + recent list +
                dialog.rs (hand-written native file dialogs: macOS NSOpenPanel /
                NSSavePanel, Windows comdlg32, Unix zenity/kdialog; no `rfd`)
src/fonts.rs    bundled-font discovery (assets/fonts/)
src/icons.rs    vendored Lucide SVG subset, drawn via igui_svg (no textures)
src/theme.rs    editor theme (compact density)
src/selfcheck.rs headless `--selfcheck` / `--dump`
packaging/      macOS .app `Info.plist`
package-macos.sh  build --release and assemble `dist/*.app` (ad-hoc codesign)
```

Flow: `Input -> AppLogic -> view -> SceneTree -> layout -> paint -> DrawList ->
Presenter -> pixels`. The window/GPU/input plumbing is `igui_app` + `igui_winit`
plugins; the app implements one `AppLogic`.

## Dependency on igui (formerly quill)

This project consumes the **igui** UI stack from the upstream GitHub repo in
**git dependency mode** — every `igui_*` crate in `Cargo.toml` points at
`https://github.com/lazyfury/igui`, pinned to the release tag documented in that
repo's `release.md` (currently `v0.3.0`). No sibling path checkout is required;
Cargo fetches the repo once for all crates. Do **not** vendor or fork the igui
crates here; shared UI-layer changes belong in the igui repo and must keep its
own gate green. To upgrade, bump the tag here and follow `release.md`'s
rename / API mapping.

The app also depends directly on `winit` (for `WindowEvent::DroppedFile`, which
`igui_winit` does not forward) and, on macOS, on `objc2` / `objc2-app-kit` /
`objc2-foundation` for the hand-written `NSOpenPanel` / `NSSavePanel` file
dialogs — **no `rfd`**. Those crates are already in the lockfile via winit/wgpu.

## Bundled font

The UI font is **京華老宋体 v3.0** (JingHua Lao Song), kept locally at:

```
assets/fonts/jinghua-laosong-v3.0.ttf
```

- **Not committed.** `assets/fonts/` is in `.gitignore` (large file, unclear
  redistribution license). Drop the `.ttf` there to build/run with it.
- `src/fonts.rs` locates that file and hands it to igui's
  `FontConfig::default_face` (`FaceRef`) as the default face, unless `--pixel-font`
  was passed. The default face seeds
  the `FontServer` without scanning the system; that scan is deferred to the first
  missing glyph or font-picker.
- It is searched next to the crate root (dev), the executable (packaged), then
  the current directory. If missing, igui falls back to its system-font
  candidate list, then the built-in pixel font.
- To swap the font, replace that file (or change `fonts::BUNDLED_FONT`).
- **Vertical text centering** lives in `EditorTextMeasurer`
  (`src/app/application.rs`), not in igui: layout places the baseline at
  `line_box_top + ascent`, so the app overrides `TextMeasurer::ascent`.
  The bundled 老宋体 needs `line_height/2 + cap/2` (its glyphs sit low in
  the em box); system fonts (PingFang SC / SF Pro) are already centred by
  their native `ascent`, so the cap formula is applied **only when the
  bundled face is actually used** (`bundled_face().is_some()`). Tune
  `CAP_HEIGHT_RATIO` there.

## Hard rules

1. **No screenshot / screen-recording visual testing.** Never use
   `screencapture`, browser screenshots, screen recording, or any OS-level
   capture to verify rendering. Verify programmatically instead: read the
   backend's own pixel buffer, assert `DrawList` command sequences, check layout
   rectangles, or read callback / shared state. If a claim cannot be verified
   without a screenshot, say so rather than capturing one.
2. **No meaningless UI tests.** Every test asserts a behaviour or a rule and is
   named as that rule. Do not:
   - re-test the shared pipeline through every entry point;
   - write a test whose only job is to call a function and assert it ran;
   - snapshot the whole draw list or assert on incidental text.
   Prefer component-level unit tests (state transitions, geometry, parsing,
   clamping) over end-to-end UI tests. `--selfcheck` already frame-checks the
   layout — don't duplicate it by hand. One behaviour per test; delete before
   adding when the count outgrows the behaviour.
3. **Keep agent / LLM context small.** `rg` for a symbol before opening a file;
   read one module (or the window around a change), not a whole crate; never read
   `target/` or `Cargo.lock`; capture noisy command output to a file or pipe it
   through `rg`; batch verification into one command; don't re-read a file right
   after editing it.
4. One concern per module. A file that passes ~500 lines gets split along its
   seams; a domain concern (time, colour, path, …) gets its own small module
   rather than being embedded where it is used.
5. Order of work: **API -> test -> implementation -> integration.**
6. Scope: 2D raster editing only. No ECS, shaders, render graph, particles, or an
   in-app editor editor. `png` / `tracing` are fine here; keep new dependencies
   out of the igui core crates.

## Build / run / verify

```bash
cargo run                       # desktop app (winit + wgpu)
cargo run -- --selfcheck        # headless: record DrawList, inspect, assert (exit 1 on failure)
cargo run -- --dump             # same, and print the draw commands
cargo run -- --frames 120       # render N frames then exit
cargo run -- --light            # light theme
cargo run -- --pixel-font       # built-in bitmap font (no CJK)
cargo test                      # unit tests
cargo clippy --all-targets -- -D warnings           # lints (must stay clean)
# per-change gate (the same steps CI runs, see .github/workflows/ci.yml):
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
./package-macos.sh              # macOS .app bundle in dist/ (copies the font)
./package-macos.sh --open       # ... and launch it
```

The app has no screenshot tests; `--selfcheck` (and the unit tests) are the
verification. When you change a shared igui crate, do it in the igui repo, run
its own gate there (`cargo fmt --all -- --check && cargo check --workspace &&
cargo test --workspace`), then bump the pinned tag here.

## Doc division

- `README.md` — what the app does, phase by phase.
- `todo.md` — what is planned / in progress, per item ("change here / verify
  how").
- Module top-of-file docs — the contract of one module.
