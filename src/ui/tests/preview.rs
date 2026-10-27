//! 预览面板：默认标签、纹理上传与「主画布可见范围」框。

use super::*;

/// 打开工作区默认停在「预览」，历史页不占布局。
#[test]
fn the_preview_tab_is_the_default() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.update();
    view.layout(viewport());
    assert_eq!(view.active_tab(), SidebarTab::Preview);
    assert!(view.tab_content_visible(SidebarTab::Preview));
    assert!(!view.tab_content_visible(SidebarTab::History));
}

/// 预览画的是显示合成结果缩成的小图，并在上面画主画布可见范围的四条边。
#[test]
fn the_preview_draws_the_texture_and_the_visible_box() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.update();
    view.layout(viewport());

    view.mark_texture_dirty();
    let _ = view.take_texture_upload().expect("显示纹理");
    let preview = view.take_preview_upload().expect("预览图");
    assert!(preview.width <= crate::ui::preview_panel::PREVIEW_MAX_EDGE);
    assert!(preview.height <= crate::ui::preview_panel::PREVIEW_MAX_EDGE);

    let rect = view.preview_rect().expect("预览区域");
    let commands = paint_commands(&view);
    let destination = commands
        .iter()
        .find_map(|command| match command {
            DrawCommand::DrawImage {
                texture,
                destination,
                ..
            } if *texture == crate::ui::PREVIEW_TEXTURE => Some(*destination),
            _ => None,
        })
        .expect("预览应画出文档纹理");
    assert!(
        rect.contains(destination.center()),
        "预览图应画在预览区域里：{destination:?} / {rect:?}"
    );

    let accent = view.theme.palette().accent;
    let edges = commands
        .iter()
        .filter(|command| match command {
            DrawCommand::Line {
                from, to, paint, ..
            } => paint.color == accent && rect.contains(*from) && rect.contains(*to),
            _ => false,
        })
        .count();
    assert_eq!(edges, 4, "可见范围是四条 accent 边（旋转时也是四边形）");
}

/// 预览图 200ms 节流：刚缩过就不再缩，时间到了才重新上传。
#[test]
fn the_preview_upload_is_throttled() {
    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.update();
    view.layout(viewport());

    view.mark_texture_dirty();
    let _ = view.take_texture_upload().expect("显示纹理");
    let _ = view.take_preview_upload().expect("第一张预览");

    // 画笔那种**局部**更新走节流；整图重合成（结构改动 / 切工作区）会立即刷新。
    view.mark_region_dirty(crate::document::PixelRegion::new(0, 0, 1, 1));
    let _ = view.take_texture_upload().expect("局部合成");
    assert!(
        view.take_preview_upload().is_none(),
        "节流窗口内不重复缩预览"
    );

    view.preview_at = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_millis(
            crate::ui::preview_panel::PREVIEW_INTERVAL_MS,
        ))
        .unwrap();
    view.mark_region_dirty(crate::document::PixelRegion::new(0, 0, 1, 1));
    let _ = view.take_texture_upload().expect("局部合成");
    assert!(view.take_preview_upload().is_some(), "到点后重新上传");
}
