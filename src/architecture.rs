//! 分层约束的机械检查（只在 `cargo test` 里跑）。
//!
//! `AGENTS.md` 的分层是 `document <- canvas / renderer / tools / io <- app / ui`：
//! 核心层不认识窗口、控件、平台。约定很容易在某一次改动里悄悄破掉（加一行
//! `use crate::ui::...` 就够了，编译还会过），所以这里读一遍源码再断言。
//!
//! 检查只看**代码**：`//` 之后的内容（含文档链接）不算依赖。

use std::path::{Path, PathBuf};

/// 核心层目录 + 它不允许依赖的路径前缀（分层的箭头方向）。
const LAYERS: &[(&str, &[&str])] = &[
    (
        "document",
        &[
            "crate::app",
            "crate::ui",
            "crate::io",
            "crate::tools",
            "crate::renderer",
            "crate::canvas",
        ],
    ),
    (
        "canvas",
        &[
            "crate::app",
            "crate::ui",
            "crate::io",
            "crate::tools",
            "crate::renderer",
        ],
    ),
    (
        "renderer",
        &[
            "crate::app",
            "crate::ui",
            "crate::io",
            "crate::tools",
            "crate::canvas",
        ],
    ),
    (
        "tools",
        &["crate::app", "crate::ui", "crate::io", "crate::renderer"],
    ),
    (
        "io",
        &["crate::app", "crate::ui", "crate::renderer", "crate::tools"],
    ),
];

/// 控件 / 平台栈：只有 `app` 和 `ui` 可以碰。
const WIDGET_CRATES: &[&str] = &[
    "igui_ui",
    "igui_components",
    "igui_scene",
    "igui_theme",
    "igui_app",
    "igui_winit",
    "igui_backend_wgpu",
    "igui_backend_recording",
    "igui_svg",
];

fn src_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// 一个目录下的所有 `.rs`（含子目录），按路径排序好让失败信息稳定。
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// 行里的代码部分（去掉 `//` 之后的注释）。
fn code_of(line: &str) -> &str {
    line.split("//").next().unwrap_or("")
}

/// 在 `dir` 里找 `needles`（路径前缀）的代码引用，返回 `相对路径:行号: 命中` 列表。
fn references(dir: &Path, needles: &[&str]) -> Vec<String> {
    let root = src_root();
    let mut hits = Vec::new();
    for file in rust_files(dir) {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let name = file
            .strip_prefix(&root)
            .unwrap_or(&file)
            .display()
            .to_string();
        for (index, line) in text.lines().enumerate() {
            let code = code_of(line);
            if let Some(needle) = needles
                .iter()
                .find(|needle| code.contains(&format!("{needle}::")))
            {
                hits.push(format!("{name}:{}: {needle}", index + 1));
            }
        }
    }
    hits
}

#[test]
fn core_layers_only_point_down_the_dependency_arrow() {
    let root = src_root();
    let mut scanned = 0;
    let mut violations = Vec::new();
    for (layer, forbidden) in LAYERS {
        let dir = root.join(layer);
        scanned += rust_files(&dir).len();
        violations.extend(references(&dir, forbidden));
    }
    // 目录改名 / 挪走时别让这条规则静默通过。
    assert!(scanned > 30, "只扫到 {scanned} 个源文件，分层检查没覆盖到");
    assert!(
        violations.is_empty(),
        "核心层只能向下依赖（document <- canvas/renderer/tools/io）：\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_widget_stack_stays_in_app_and_ui() {
    let root = src_root();
    let mut violations = Vec::new();
    for (layer, _) in LAYERS {
        violations.extend(references(&root.join(layer), WIDGET_CRATES));
    }
    assert!(
        violations.is_empty(),
        "核心层不该认识控件 / 平台栈（只有 app / ui 可以）：\n{}",
        violations.join("\n")
    );
}
