# AGENTS.md — image_editor

A standalone, Photoshop-style 2D image editor whose entire UI is drawn by the
**quill** stack (`draw_core` / `draw_render` / `draw_scene` / `draw_theme` /
`draw_ui` / `draw_components` / `draw_svg` / `draw_backend_wgpu`). No `egui`, no
DOM.

This file is the short source of truth for rules. Details live in `README.md`
(features, phases) and `todo.md` (project backlog).

## Where things are

```
src/app/        state (AppState/document session) + window host (winit/wgpu, multi-window)
src/ui/         views + project-local components: home, new_document, editor (mod.rs),
                menu, toolbar, options_bar, canvas, layer/file/properties/tabs/palette panels
src/document/   Document / Layer / PixelBuffer / Color / History + commands
src/renderer/   CPU compositor (layers -> one PixelBuffer)
src/tools/      Brush/Eraser, Move, Rectangle-select, Eyedropper
src/canvas/     camera (zoom/pan), coordinate conversion, checkerboard
src/io/         PNG encode/decode + path helpers
src/fonts.rs    bundled-font discovery (assets/fonts/, `QUILL_FONT`)
src/icons.rs    vendored Lucide SVG subset, drawn via draw_svg (no textures)
src/theme.rs    editor theme (compact density)
src/selfcheck.rs headless `--selfcheck` / `--dump`
packaging/      macOS .app `Info.plist`
package-macos.sh  build --release and assemble `dist/*.app` (ad-hoc codesign)
```

Flow: `Input -> view -> SceneTree -> layout -> paint -> DrawList -> RenderBackend
-> pixels`.

## Dependency on quill

This project consumes the quill crates from the **sibling checkout** via relative
path deps (`../quill/crates/*` in `Cargo.toml`). Keep `image_editor/` and
`quill/` adjacent under the same parent directory. Do **not** vendor or fork the
quill crates here; shared UI-layer changes belong in the quill repo and must keep
its own gate green.

## Bundled font

The UI font is **京華老宋体 v3.0** (JingHua Lao Song), kept locally at:

```
assets/fonts/jinghua-laosong-v3.0.ttf
```

- **Not committed.** `assets/fonts/` is in `.gitignore` (large file, unclear
  redistribution license). Drop the `.ttf` there to build/run with it.
- `src/fonts.rs` locates that file and hands it to quill's
  `FontConfig::default_face` (`FaceRef`) as the default face, unless the user set
  `QUILL_FONT` (their value wins) or passed `--pixel-font`. The default face seeds
  the `FontServer` without scanning the system; that scan is deferred to the first
  missing glyph or font-picker.
- It is searched next to the crate root (dev), the executable (packaged), then
  the current directory. If missing, quill falls back to its system-font
  candidate list, then the built-in pixel font.
- To swap the font, replace that file (or change `fonts::BUNDLED_FONT`).

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
   out of the quill core crates.

## Build / run / verify

```bash
cargo run                       # desktop app (winit + wgpu)
cargo run -- --selfcheck        # headless: record DrawList, inspect, assert (exit 1 on failure)
cargo run -- --dump             # same, and print the draw commands
cargo run -- --frames 120       # render N frames then exit
cargo run -- --light            # light theme
cargo run -- --pixel-font       # built-in bitmap font (no CJK)
cargo test                      # unit tests
cargo fmt -- --check && cargo check && cargo test   # per-change gate
./package-macos.sh              # macOS .app bundle in dist/ (copies the font)
./package-macos.sh --open       # ... and launch it
```

The app has no screenshot tests; `--selfcheck` (and the unit tests) are the
verification. When you change a shared quill crate, also run quill's own gate in
`../quill` (`cargo fmt --all -- --check && cargo check --workspace &&
cargo test --workspace`).

## Doc division

- `README.md` — what the app does, phase by phase.
- `todo.md` — what is planned / in progress, per item ("change here / verify
  how").
- Module top-of-file docs — the contract of one module.
