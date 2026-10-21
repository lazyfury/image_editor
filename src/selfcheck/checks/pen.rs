//! 钢笔：多击建路径 -> 选区 / 填充 / 描边。

use crate::app::state::ActiveTool;
use crate::document::Color;
use crate::selfcheck::*;
use crate::ui::EditorView;
use igui_core::{InputEvent, Key, Vec2};

/// 钢笔：点三下成路径、Enter 收尾、点回起点闭合，再走「建立选区 / 填充 / 描边」。
pub(crate) fn check_pen(failures: &mut Vec<String>) {
    use crate::ui::PenAction;

    let mut view = EditorView::new(crate::theme::editor_theme(false), AppState::default());
    view.layout(viewport());
    view.event(&InputEvent::KeyDown {
        key: Key::Character('p'),
    });
    view.update();
    if view.active_tool() != ActiveTool::Pen {
        failures.push("P 应切到钢笔".to_string());
        return;
    }
    view.layout(viewport());

    // 三下点击 = 三个锚点（直角边 88 的三角形）。
    for (x, y) in [(20.0, 20.0), (108.0, 20.0), (108.0, 108.0)] {
        let screen = view.canvas_camera().document_to_screen(Vec2::new(x, y));
        click_at(&mut view, screen);
        view.update();
    }
    if view.pen_anchor_count() != 3 {
        failures.push(format!(
            "钢笔点三下应有 3 个锚点，实为 {}",
            view.pen_anchor_count()
        ));
    }

    // 画到一半要有预览：锚点方块（StrokeRect）+ 路径 / 橡皮筋（Line）。
    let hover = view
        .canvas_camera()
        .document_to_screen(Vec2::new(40.0, 90.0));
    view.event(&InputEvent::PointerMove { position: hover });
    view.layout(viewport());
    let frame = paint_frame(&view);
    let anchors = frame
        .commands()
        .iter()
        .filter(|command| matches!(command, DrawCommand::StrokeRect { .. }))
        .count();
    let lines = frame
        .commands()
        .iter()
        .filter(|command| matches!(command, DrawCommand::Line { .. }))
        .count();
    if anchors < 3 {
        failures.push(format!("钢笔预览只画了 {anchors} 个锚点方块"));
    }
    if lines < 3 {
        failures.push(format!("钢笔预览的路径 / 橡皮筋没画出来（{lines} 条线）"));
    }
    if !contains(&frame, "3 个锚点") {
        failures.push("选项栏没显示钢笔的锚点数".to_string());
    }

    // Enter 收尾是开放路径；点回起点才闭合。
    view.event(&InputEvent::KeyDown { key: Key::Enter });
    view.update();
    if view.pen_path_closed() {
        failures.push("Enter 收尾应保持开放路径".to_string());
    }
    let first = view
        .canvas_camera()
        .document_to_screen(Vec2::new(20.0, 20.0));
    click_at(&mut view, first + Vec2::new(3.0, 2.0));
    view.update();
    if !view.pen_path_closed() {
        failures.push("点回起点应闭合路径".to_string());
    }
    if view.pen_anchor_count() != 3 {
        failures.push("闭合不该再落一个锚点".to_string());
    }

    // 建立选区：直角边 88 的三角形 ≈ 88² / 2 = 3872 像素。
    match view.pen_action_center(PenAction::Select) {
        Some(center) => {
            click_at(&mut view, center);
            view.update();
            match view.selection() {
                Some(selection) if (selection.area() as i32 - 3872).abs() < 250 => {}
                Some(selection) => failures.push(format!(
                    "钢笔选区面积 {} 不对（期望 ≈3872）",
                    selection.area()
                )),
                None => failures.push("「建立选区」没有产生选区".to_string()),
            }
        }
        None => failures.push("找不到钢笔的「建立选区」按钮".to_string()),
    }

    // 描边：用当前画笔沿路径画一遍，只加一步 undo，路径上的像素变黑。
    let before = view.history_len().0;
    match view.pen_action_center(PenAction::Stroke) {
        Some(center) => {
            click_at(&mut view, center);
            view.update();
            if view.history_len().0 != before + 1 {
                failures.push("钢笔描边应只加一步 undo".to_string());
            }
            let on_edge = view
                .document_snapshot()
                .active_layer()
                .map(|layer| layer.pixels.get_pixel(60, 20))
                .unwrap_or(Color::TRANSPARENT);
            if on_edge != Color::BLACK {
                failures.push(format!("钢笔描边没画到路径上：{on_edge:?}"));
            }
        }
        None => failures.push("找不到钢笔的「描边」按钮".to_string()),
    }

    // 填充：同样只加一步 undo，三角形内部变成前景色（路径已经描过边，内部还是白的）。
    let before = view.history_len().0;
    match view.pen_action_center(PenAction::Fill) {
        Some(center) => {
            click_at(&mut view, center);
            view.update();
            if view.history_len().0 != before + 1 {
                failures.push("钢笔填充应只加一步 undo".to_string());
            }
            let inside = view
                .document_snapshot()
                .active_layer()
                .map(|layer| layer.pixels.get_pixel(90, 40))
                .unwrap_or(Color::TRANSPARENT);
            if inside != Color::BLACK {
                failures.push(format!("钢笔填充没填到路径里：{inside:?}"));
            }
        }
        None => failures.push("找不到钢笔的「填充」按钮".to_string()),
    }

    // 清除：路径没了，选项栏回到「没有路径」。
    match view.pen_action_center(PenAction::Clear) {
        Some(center) => {
            click_at(&mut view, center);
            view.update();
            if view.pen_anchor_count() != 0 {
                failures.push("「清除」没有丢掉路径".to_string());
            }
            view.layout(viewport());
            if !contains(&paint_frame(&view), "没有路径") {
                failures.push("清除后选项栏应显示「没有路径」".to_string());
            }
        }
        None => failures.push("找不到钢笔的「清除」按钮".to_string()),
    }
}
