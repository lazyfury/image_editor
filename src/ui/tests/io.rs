//! PNG 进出 + 文件按钮只发意图。

use super::*;

/// 测试用唯一临时 PNG 路径。
fn temp_png(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "image_editor_ui_{}_{n}_{tag}.png",
        std::process::id()
    ))
}

#[test]
fn exporting_writes_the_composite_to_the_path() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    paint_a_stroke(&mut view);

    let path = temp_png("export");
    view.export_png_to(&path.to_string_lossy())
        .expect("导出 PNG");

    let exported = crate::io::read_png(&path).expect("导出的 PNG 应能解码");
    assert_eq!(exported.get_pixel(0, 0), Color::WHITE);
    assert_eq!(exported.get_pixel(64, 64), Color::BLACK);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn importing_a_png_adds_a_layer() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    let path = temp_png("import");
    crate::io::write_png(&path, &PixelBuffer::filled(2, 2, Color::RED)).unwrap();

    let before = view.state.borrow().document.layers.len();
    view.import_png_from(&path.to_string_lossy())
        .expect("导入 PNG");
    view.update();

    assert_eq!(view.state.borrow().document.layers.len(), before + 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_failed_export_is_an_error() {
    let view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    let missing = std::env::temp_dir()
        .join("image_editor_definitely_missing_dir")
        .join("x.png");
    assert!(view.export_png_to(&missing.to_string_lossy()).is_err());
}

/// 每个文件按钮只留下对应的意图（路径由宿主原生对话框选）。
#[test]
fn file_buttons_emit_their_intent() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    for action in [
        FileRequest::Save,
        FileRequest::SaveAs,
        FileRequest::Open,
        FileRequest::ImportPng,
        FileRequest::ExportPng,
    ] {
        let center = view
            .file_center(action)
            .unwrap_or_else(|| panic!("{action:?} 按钮"));
        click(&mut view, center);
        view.update();
        assert_eq!(
            view.take_file_request(),
            Some(action),
            "{action:?} 应留下意图"
        );
    }
}
