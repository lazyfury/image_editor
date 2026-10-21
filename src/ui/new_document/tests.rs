//! 新建文档模态的测试：步进 / 预设 / 背景 / 创建与取消。

use super::widgets::to_ui;
use super::*;

use igui_core::{Color as UiColor, PointerButton, Size};
use igui_render::DrawCommand;

use super::super::window::FOOTER_BUTTON_MIN_WIDTH;
use crate::canvas::{CHECKER_DARK, CHECKER_LIGHT};
use crate::document::Color;

fn viewport() -> ViewportSize {
    ViewportSize::new(Size::new(700.0, 640.0))
}

fn modal() -> NewDocumentModal {
    let mut modal = NewDocumentModal::new(crate::theme::editor_theme(false));
    modal.layout(viewport());
    modal
}

/// 页脚按钮要有统一的宽度（创建 / 取消一样大）。
#[test]
fn the_footer_buttons_are_the_same_size() {
    let modal = modal();
    let tree = modal.window.tree();
    let size_of = |slot: &Cell<Option<NodeId>>| {
        let id = slot.get().expect("页脚按钮已挂载");
        igui_ui::control(tree, id).map(|control| control.rect.size)
    };
    let create = size_of(&modal.nodes.create).expect("创建按钮 rect");
    let cancel = size_of(&modal.nodes.cancel).expect("取消按钮 rect");
    assert_eq!(create, cancel, "创建 / 取消按钮应一样大");
    assert!(create.width >= FOOTER_BUTTON_MIN_WIDTH, "size = {create:?}");
}

fn click(modal: &mut NewDocumentModal, position: Vec2) {
    modal.event(&InputEvent::PointerDown {
        position,
        button: PointerButton::Left,
    });
    modal.event(&InputEvent::PointerUp {
        position,
        button: PointerButton::Left,
    });
    modal.update();
    modal.layout(viewport());
}

fn center(modal: &NewDocumentModal, id: NodeId) -> Option<Vec2> {
    igui_ui::control(modal.tree(), id).map(|control| control.rect.center())
}

fn rects(modal: &NewDocumentModal, ids: &RefCell<Vec<NodeId>>) -> Vec<Rect> {
    ids.borrow()
        .iter()
        .filter_map(|id| igui_ui::control(modal.tree(), *id).map(|control| control.rect))
        .collect()
}

/// 拖动右边，把窗口缩到最小宽度，用来逼出预设换行。
fn resize_to_min(modal: &mut NewDocumentModal) {
    let rect = modal.rect();
    let grab = Vec2::new(rect.right() - 2.0, rect.center().y);
    modal.event(&InputEvent::PointerDown {
        position: grab,
        button: PointerButton::Left,
    });
    modal.event(&InputEvent::PointerMove {
        position: grab - Vec2::new(10_000.0, 0.0),
    });
    modal.event(&InputEvent::PointerUp {
        position: grab - Vec2::new(10_000.0, 0.0),
        button: PointerButton::Left,
    });
    modal.layout(viewport());
}

#[test]
fn the_default_spec_is_32_transparent() {
    assert_eq!(
        NewDocumentSpec::default(),
        NewDocumentSpec {
            width: 32,
            height: 32,
            background: Color::TRANSPARENT,
        }
    );
}

#[test]
fn the_window_is_centered_and_keeps_a_minimum_size() {
    let modal = modal();
    let rect = modal.rect();
    let offset = rect.center() - viewport().logical_rect().center();
    assert!(
        offset.x.abs() < 2.0 && offset.y.abs() < 2.0,
        "窗口应大致居中：{offset:?}"
    );
    assert!(rect.size.width >= WINDOW_MIN.x, "rect = {rect:?}");
    assert!(rect.size.height >= WINDOW_MIN.y, "rect = {rect:?}");
}

/// 拖动标题栏能移动窗口，且内容不会被挪出窗口。
#[test]
fn dragging_the_title_bar_moves_the_modal() {
    let mut modal = modal();
    let before = modal.rect();
    let grab = Vec2::new(before.center().x, before.top() + 10.0);
    modal.event(&InputEvent::PointerDown {
        position: grab,
        button: PointerButton::Left,
    });
    modal.event(&InputEvent::PointerMove {
        position: grab + Vec2::new(-60.0, 40.0),
    });
    modal.event(&InputEvent::PointerUp {
        position: grab + Vec2::new(-60.0, 40.0),
        button: PointerButton::Left,
    });
    modal.layout(viewport());

    let after = modal.rect();
    assert_eq!(after.origin, before.origin + Vec2::new(-60.0, 40.0));
    assert_eq!(after.size, before.size);
}

/// 缩到最小后，预设换行且不压住背景色那一行。
#[test]
fn a_wrapped_preset_row_does_not_overlap_the_background_row() {
    let mut modal = modal();
    resize_to_min(&mut modal);

    let presets = rects(&modal, &modal.nodes.presets);
    let first_row = presets.first().map(|rect| rect.top()).unwrap_or(0.0);
    let rows = presets
        .iter()
        .filter(|rect| (rect.top() - first_row).abs() > 0.5)
        .count();
    assert!(rows >= 1, "最小宽度下预设应换行：{presets:?}");
    for (i, a) in presets.iter().enumerate() {
        for b in &presets[i + 1..] {
            assert!(a.intersection(*b).is_none(), "预设不应重叠：{a:?} vs {b:?}");
        }
    }

    let preset_bottom = presets
        .iter()
        .map(|rect| rect.bottom())
        .fold(0.0f32, f32::max);
    let background_top = rects(&modal, &modal.nodes.backgrounds)
        .iter()
        .map(|rect| rect.top())
        .fold(f32::INFINITY, f32::min);
    assert!(
        preset_bottom <= background_top + 0.5,
        "换行后的预设压住了下一行：preset_bottom={preset_bottom} background_top={background_top}"
    );
}

/// 高分辨率预设：一次点到位，宽高都按真实分辨率设置。
#[test]
fn a_high_resolution_preset_sets_both_sides() {
    let mut modal = modal();
    // 4K：3840×2160。
    let index = PRESETS
        .iter()
        .position(|(width, height)| (*width, *height) == (3840, 2160))
        .expect("4K 预设");
    let id = modal.nodes.presets.borrow()[index];
    let position = center(&modal, id).expect("预设按钮");
    click(&mut modal, position);
    assert_eq!((modal.spec().width, modal.spec().height), (3840, 2160));

    // A4 300dpi 是竖的：宽 < 高。
    let index = PRESETS
        .iter()
        .position(|(width, height)| (*width, *height) == (2480, 3508))
        .expect("A4 300dpi 预设");
    let id = modal.nodes.presets.borrow()[index];
    let position = center(&modal, id).expect("预设按钮");
    click(&mut modal, position);
    assert_eq!((modal.spec().width, modal.spec().height), (2480, 3508));
}

/// 每个预设都要在 stepper 的合法范围内，否则点完就不再用 ± 调得回去。
#[test]
fn every_preset_fits_the_side_limits() {
    for (width, height) in PRESETS {
        assert!(
            (MIN_SIDE..=MAX_SIDE).contains(&width) && (MIN_SIDE..=MAX_SIDE).contains(&height),
            "{width}×{height} 超出 {MIN_SIDE}..={MAX_SIDE}"
        );
    }
    // 4K 与 A4 300dpi 必须放得下（上限就是为了它们定的）。
    let largest = PRESETS.iter().map(|(w, h)| (*w).max(*h)).max().unwrap_or(0);
    assert!(MAX_SIDE >= largest, "MAX_SIDE = {MAX_SIDE} < {largest}");
}

/// 最小窗口下，换行后的预设和背景色仍然落在页脚上面（不溢出内容区）。
#[test]
fn the_wrapped_presets_stay_above_the_footer_at_the_minimum_size() {
    let mut modal = modal();
    resize_to_min(&mut modal);
    let presets = rects(&modal, &modal.nodes.presets);
    let backgrounds = rects(&modal, &modal.nodes.backgrounds);
    let footer = center(&modal, modal.nodes.create.get().expect("创建按钮"))
        .map(|center| center.y - 12.0)
        .expect("创建按钮位置");
    let content_bottom = presets
        .iter()
        .chain(backgrounds.iter())
        .map(|rect| rect.bottom())
        .fold(0.0f32, f32::max);
    assert!(
        content_bottom <= footer + 0.5,
        "内容溢出到页脚：content_bottom={content_bottom} footer_top={footer}"
    );
}

/// 透明背景在色块里用棋盘格预览，而不是一片空白。
#[test]
fn the_transparent_swatch_previews_a_checkerboard() {
    let modal = modal();
    let mut ctx = PaintContext::new();
    modal.paint(&mut ctx);
    let commands = ctx.into_draw_list().commands().to_vec();
    let swatch = rects(&modal, &modal.nodes.backgrounds)[0];

    let fills = |color: UiColor| {
        commands.iter().any(|command| {
            matches!(command, DrawCommand::FillRect { rect, paint }
                if paint.color == color && rect.intersects(swatch))
        })
    };
    assert!(fills(to_ui(CHECKER_LIGHT)), "缺少亮格");
    assert!(fills(to_ui(CHECKER_DARK)), "缺少暗格");
}

#[test]
fn a_preset_then_create_returns_that_spec() {
    let mut modal = modal();
    let index = PRESETS.len() - 1;
    let (width, height) = PRESETS[index];
    let id = modal.nodes.presets.borrow()[index];
    let position = center(&modal, id).expect("预设按钮");
    click(&mut modal, position);
    assert_eq!(modal.spec().width, width);
    assert_eq!(modal.spec().height, height);

    modal.accept();
    assert_eq!(
        modal.take_result(),
        Some(NewDocumentResult::Create(NewDocumentSpec {
            width,
            height,
            background: Color::TRANSPARENT,
        }))
    );
    assert_eq!(modal.take_result(), None, "结果只能取走一次");
}

#[test]
fn choosing_a_background_then_create_returns_it() {
    let mut modal = modal();
    let id = modal.nodes.backgrounds.borrow()[0];
    let position = center(&modal, id).expect("透明色块");
    click(&mut modal, position);
    assert_eq!(modal.spec().background, Color::TRANSPARENT);

    modal.accept();
    assert_eq!(
        modal.take_result(),
        Some(NewDocumentResult::Create(NewDocumentSpec {
            width: 32,
            height: 32,
            background: Color::TRANSPARENT,
        }))
    );
}

/// 点「创建」按钮本身（不是测试后门）也产出结果。
#[test]
fn clicking_the_create_button_returns_the_spec() {
    let mut modal = modal();
    let id = modal.nodes.create.get().expect("创建按钮");
    let position = center(&modal, id).expect("创建按钮 rect");
    click(&mut modal, position);
    assert_eq!(
        modal.take_result(),
        Some(NewDocumentResult::Create(NewDocumentSpec::default()))
    );
}

#[test]
fn cancel_reports_cancel() {
    let modal = modal();
    modal.cancel();
    assert_eq!(modal.take_result(), Some(NewDocumentResult::Cancel));
}

/// Esc 关掉窗口，取结果时折算成取消。
#[test]
fn escape_marks_the_modal_cancelled() {
    let mut modal = modal();
    modal.event(&InputEvent::KeyDown {
        key: igui_core::Key::Escape,
    });
    assert_eq!(modal.take_result(), Some(NewDocumentResult::Cancel));
}

/// 点窗口外（遮罩）也折算成取消。
#[test]
fn a_click_outside_marks_the_modal_cancelled() {
    let mut modal = modal();
    let outside = Vec2::new(2.0, 2.0);
    assert!(!modal.rect().contains(outside));
    modal.event(&InputEvent::PointerDown {
        position: outside,
        button: PointerButton::Left,
    });
    assert_eq!(modal.take_result(), Some(NewDocumentResult::Cancel));
}
