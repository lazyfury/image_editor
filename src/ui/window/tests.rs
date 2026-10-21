//! 浮动窗口的行为测试：拖动 / 缩放 / 关闭 / 外观（手柄、标题栏、页脚）。

use igui_components::{Button, Component, NodeRef};
use std::cell::RefCell;

use super::*;

fn viewport() -> ViewportSize {
    ViewportSize::new(Size::new(800.0, 600.0))
}

/// 一个带「点我」按钮的窗口，按钮回调累计点击次数。
fn window_with_button() -> (Window, Rc<RefCell<u32>>, NodeRef) {
    let clicks: Rc<RefCell<u32>> = Rc::new(RefCell::new(0));
    let counter = clicks.clone();
    let button_ref = NodeRef::new();
    let slot = button_ref.clone();
    let window = Window::new(
        crate::theme::editor_theme(false),
        "标题",
        Vec2::new(420.0, 320.0),
        Vec2::new(320.0, 240.0),
        move |tree, content, _footer| {
            let counter = counter.clone();
            tree.add_child(
                content,
                Flex::column().padding(Edges::ZERO).child(
                    Button::secondary("点我", crate::theme::editor_theme(false))
                        .on_click(move |_tree, _id| *counter.borrow_mut() += 1)
                        .ref_(&slot),
                ),
            );
        },
    );
    (window, clicks, button_ref)
}

fn press(window: &mut Window, position: Vec2) {
    window.event(&InputEvent::PointerDown {
        position,
        button: PointerButton::Left,
    });
}

fn release(window: &mut Window, position: Vec2) {
    window.event(&InputEvent::PointerUp {
        position,
        button: PointerButton::Left,
    });
}

fn click(window: &mut Window, position: Vec2) {
    press(window, position);
    release(window, position);
    window.layout(viewport());
}

#[test]
fn the_window_starts_centered() {
    let (mut window, _, _) = window_with_button();
    window.layout(viewport());
    let rect = window.rect();
    let offset = rect.center() - viewport().logical_rect().center();
    assert!(
        offset.x.abs() < 2.0 && offset.y.abs() < 2.0,
        "窗口应大致居中：{offset:?}"
    );
    assert!(rect.size.width >= 420.0, "size = {:?}", rect.size);
    assert!(rect.size.height >= 320.0, "size = {:?}", rect.size);
}

#[test]
fn dragging_the_title_bar_moves_the_window() {
    let (mut window, _, _) = window_with_button();
    window.layout(viewport());
    let before = window.rect();
    let grab = Vec2::new(before.center().x, before.top() + 10.0);

    press(&mut window, grab);
    window.event(&InputEvent::PointerMove {
        position: grab + Vec2::new(-40.0, 30.0),
    });
    window.layout(viewport());
    let after = window.rect();

    assert_eq!(after.origin, before.origin + Vec2::new(-40.0, 30.0));
    assert_eq!(after.size, before.size, "拖动不该改尺寸");
    release(&mut window, grab + Vec2::new(-40.0, 30.0));
}

#[test]
fn dragging_the_bottom_right_corner_resizes_the_window() {
    let (mut window, _, _) = window_with_button();
    window.layout(viewport());
    let before = window.rect();
    let grab = Vec2::new(before.right() - 2.0, before.bottom() - 2.0);

    press(&mut window, grab);
    window.event(&InputEvent::PointerMove {
        position: grab + Vec2::new(50.0, 40.0),
    });
    window.layout(viewport());
    let after = window.rect();

    assert_eq!(after.origin, before.origin, "右下角缩放不动左上角");
    assert_eq!(
        after.size,
        Size::new(before.size.width + 50.0, before.size.height + 40.0)
    );
    release(&mut window, grab + Vec2::new(50.0, 40.0));
}

#[test]
fn resizing_from_the_left_edge_moves_the_left_edge_only() {
    let (mut window, _, _) = window_with_button();
    window.layout(viewport());
    let before = window.rect();
    let grab = Vec2::new(before.left() + 2.0, before.center().y);

    press(&mut window, grab);
    window.event(&InputEvent::PointerMove {
        position: grab + Vec2::new(20.0, 0.0),
    });
    window.layout(viewport());
    let after = window.rect();

    assert_eq!(after.left(), before.left() + 20.0);
    assert_eq!(after.right(), before.right());
}

#[test]
fn resizing_cannot_go_below_the_minimum() {
    let (mut window, _, _) = window_with_button();
    window.layout(viewport());
    let before = window.rect();
    let grab = Vec2::new(before.right() - 2.0, before.bottom() - 2.0);

    press(&mut window, grab);
    window.event(&InputEvent::PointerMove {
        position: grab - Vec2::new(10_000.0, 10_000.0),
    });
    window.layout(viewport());
    let after = window.rect();

    assert!(after.size.width >= 320.0 - 0.5, "size = {:?}", after.size);
    assert!(after.size.height >= 240.0 - 0.5, "size = {:?}", after.size);
}

#[test]
fn a_press_on_window_content_reaches_the_buttons() {
    let (mut window, clicks, button) = window_with_button();
    window.layout(viewport());
    let rect = window.rect();
    let button = igui_ui::control(&window.tree, button.get().expect("按钮已挂载"))
        .map(|control| control.rect)
        .expect("按钮 rect");
    let position = button.center();
    assert!(rect.contains(position));
    click(&mut window, position);

    assert_eq!(*clicks.borrow(), 1, "按钮应收到点击");
    assert_eq!(window.rect().origin, rect.origin, "点按钮不该拖动窗口");
}

#[test]
fn a_press_outside_the_window_requests_close() {
    let (mut window, _, _) = window_with_button();
    window.layout(viewport());
    let outside = Vec2::new(5.0, 5.0);
    assert!(!window.rect().contains(outside));
    press(&mut window, outside);
    assert!(window.take_close_request());
    assert!(!window.take_close_request(), "请求只能取走一次");
}

#[test]
fn escape_requests_close() {
    let (mut window, _, _) = window_with_button();
    window.layout(viewport());
    window.event(&InputEvent::KeyDown { key: Key::Escape });
    assert!(window.take_close_request());
}

#[test]
fn the_close_button_requests_close() {
    let (mut window, _, _) = window_with_button();
    window.layout(viewport());
    let rect = window.rect();
    let close = Vec2::new(
        rect.right() - CLOSE_BUTTON / 2.0,
        rect.top() + TITLE_BAR_HEIGHT / 2.0,
    );
    click(&mut window, close);
    assert!(window.take_close_request());
}

/// 窗口底色必须与 secondary / ghost 按钮的填充不同，否则按钮只剩描边，
/// 看起来比实心按钮小一圈。
#[test]
fn the_window_surface_differs_from_the_button_fill() {
    let theme = crate::theme::editor_theme(false);
    assert_ne!(
        theme.surface(WINDOW_SURFACE),
        theme.palette().surface_raised,
        "对话框底色不能和按钮填充一样"
    );
}

/// 内容区紧贴标题栏下沿与外框两边（外框不能自带 padding）。
#[test]
fn the_content_is_flush_with_the_header_and_the_frame() {
    let (mut window, _, _) = window_with_button();
    window.layout(viewport());
    let rect = window.rect();
    let content = igui_ui::control(&window.tree, window.content)
        .map(|control| control.rect)
        .expect("内容区 rect");
    let footer = igui_ui::control(&window.tree, window.footer)
        .map(|control| control.rect)
        .expect("页脚 rect");
    // 外框不能自带 padding（`Flex` 默认 16），否则标题栏 / 页脚会被缩进。
    assert_eq!(content.left(), rect.left(), "内容区应贴左边");
    assert_eq!(content.right(), rect.right(), "内容区应贴右边");
    assert_eq!(
        content.top(),
        rect.top() + TITLE_BAR_HEIGHT,
        "内容区应紧贴标题栏下沿"
    );
    assert_eq!(footer.left(), rect.left(), "页脚应贴左边");
    assert_eq!(footer.right(), rect.right(), "页脚应贴右边");
    assert_eq!(footer.bottom(), rect.bottom(), "页脚应贴底边");
}

/// 页脚是标题栏与内容区之外的独立底部区域。
#[test]
fn the_footer_is_a_separate_bottom_region() {
    let button_ref = NodeRef::new();
    let slot = button_ref.clone();
    let mut window = Window::new(
        crate::theme::editor_theme(false),
        "标题",
        Vec2::new(420.0, 320.0),
        Vec2::new(320.0, 240.0),
        move |tree, _content, footer| {
            tree.add_child(
                footer,
                Button::primary("确定", crate::theme::editor_theme(false)).ref_(&slot),
            );
        },
    );
    window.layout(viewport());
    let rect = window.rect();
    let button = igui_ui::control(&window.tree, button_ref.get().expect("按钮已挂载"))
        .map(|control| control.rect)
        .expect("按钮 rect");
    assert!(rect.contains(button.center()));
    assert!(
        button.top() > rect.top() + TITLE_BAR_HEIGHT,
        "页脚按钮应在标题栏下方"
    );
    assert!(
        button.bottom() > rect.bottom() - 60.0,
        "页脚按钮应贴近底边：button={button:?} rect={rect:?}"
    );
}

/// 标题栏比窗口正文浅一档（深色主题下更亮），把标题区从内容里分出来。
#[test]
fn the_header_band_is_lighter_than_the_window_body() {
    fn luminance(color: Color) -> f32 {
        let [r, g, b, _] = color.to_rgba8();
        0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
    }

    let dark = crate::theme::editor_theme(false);
    assert!(
        luminance(dark.surface(HEADER_SURFACE)) > luminance(dark.surface(WINDOW_SURFACE)),
        "深色主题下标题栏应更浅"
    );
    let light = crate::theme::editor_theme(true);
    assert_ne!(
        light.surface(HEADER_SURFACE),
        light.surface(WINDOW_SURFACE),
        "浅色主题下标题栏也要和正文区分开"
    );
}

/// 页脚有固定最小高度：内容再挤，底部操作区也不会缩没。
#[test]
fn the_footer_keeps_a_minimum_height() {
    let (mut window, _, _) = window_with_button();
    window.layout(viewport());
    let footer = |window: &Window| {
        igui_ui::control(&window.tree, window.footer)
            .map(|control| control.rect)
            .expect("页脚 rect")
    };
    let height = footer(&window).size.height;
    assert!(
        height >= FOOTER_MIN_HEIGHT - 0.5,
        "页脚应至少 {FOOTER_MIN_HEIGHT} 高，实际 {height}"
    );

    // 视口缩到放不下初始尺寸时也不该被压扁。
    window.layout(ViewportSize::new(Size::new(360.0, 280.0)));
    let height = footer(&window).size.height;
    assert!(
        height >= FOOTER_MIN_HEIGHT - 0.5,
        "小视口下页脚应保持最小高度，实际 {height}"
    );
}

/// 右下角画了缩放提示：三道 45° 斜线，位置在窗口右下角附近。
#[test]
fn the_resize_grip_is_painted_in_the_bottom_right_corner() {
    use igui_render::DrawCommand;

    let (mut window, _, _) = window_with_button();
    window.layout(viewport());
    let rect = window.rect();
    let corner = Rect::from_min_size(
        Vec2::new(rect.right() - 24.0, rect.bottom() - 24.0),
        Size::splat(24.0),
    );

    let mut ctx = PaintContext::new();
    window.paint(&mut ctx);
    let lines: Vec<(Vec2, Vec2)> = ctx
        .into_draw_list()
        .into_commands()
        .into_iter()
        .filter_map(|command| match command {
            DrawCommand::Line { from, to, .. } if corner.contains(from) && corner.contains(to) => {
                Some((from, to))
            }
            _ => None,
        })
        .collect();

    assert_eq!(
        lines.len(),
        GRIP_LINES,
        "右下角应有 {GRIP_LINES} 道缩放手柄线：{lines:?}"
    );
    for (from, to) in &lines {
        assert!(
            (to.x - from.x - (from.y - to.y)).abs() < 0.01,
            "手柄斜线应为 45°：{from:?} → {to:?}"
        );
    }
}

#[test]
fn the_window_swallows_input_underneath() {
    let (mut window, _, _) = window_with_button();
    window.layout(viewport());
    let result = window.event(&InputEvent::KeyDown {
        key: Key::Character('x'),
    });
    assert!(result.is_handled());
}
