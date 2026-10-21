//! 主流程的顺序检查：首帧合成 / 撤销重做 / PNG 进出 / 指针工具。
//!
//! 这些检查跑在 `check()` 建好的同一个视图上，所以按顺序调用：每一步都接着
//! 上一步的文档状态（一笔 -> 撤销 -> 重做 -> 导出 / 导入 -> 取色 / 框选 / 移动）。

use crate::app::state::{ActiveTool, HistoryAction};
use crate::document::{Color, PixelBuffer};
use crate::selfcheck::*;
use crate::ui::{EditorView, FileRequest};
use igui_core::{InputEvent, PointerButton, Vec2};

/// CPU 合成器：一个新文档 + 一笔黑色，应当能合成出对应的像素；
/// 撤销区域要贴着笔迹、一笔就是一步 undo。
pub(crate) fn first_stroke(view: &mut EditorView, failures: &mut Vec<String>) {
    view.mark_texture_dirty();
    let composite = view.take_texture_upload().expect("脏文档应产出合成结果");
    if composite.get_pixel(0, 0) != Color::WHITE {
        failures.push("CPU 合成结果不是白底".to_string());
    }
    if composite.get_pixel(64, 64) != Color::BLACK {
        failures.push("画笔没有在 (64, 64) 留下像素".to_string());
    }
    if !view.can_undo() {
        failures.push("一笔画笔之后应该能撤销".to_string());
    }
    // 撤销区域贴着笔迹（不是整层 diff）：30 屏幕像素的一笔远小于整张文档。
    match view.undo_region() {
        Some(region) if region.area() * 8 < 128 * 128 => {}
        Some(region) => failures.push(format!("撤销区域应是笔迹包围盒而不是整层：{region:?}")),
        None => failures.push("撤销命令没有记录像素区域".to_string()),
    }
    // 一笔 = 一步 undo：此刻栈里应该只有这一笔。
    if view.history_len() != (1, 0) {
        failures.push(format!(
            "一笔之后应为 undo=1/redo=0，实际 {:?}",
            view.history_len()
        ));
    }
}

/// 点工具栏的「撤销」回到白底，点「重做」再画回来。
pub(crate) fn undo_redo(view: &mut EditorView, failures: &mut Vec<String>) {
    // Phase 6：一笔 = 一步 undo。点工具栏的“撤销”回到白底，点“重做”再画回来。
    let undo_button = view.history_center(HistoryAction::Undo).expect("撤销按钮");
    click_at(view, undo_button);
    view.update();
    view.mark_texture_dirty();
    let undone = view.take_texture_upload().expect("撤销后应重合成");
    if undone.get_pixel(64, 64) != Color::WHITE {
        failures.push("撤销没有还原画笔笔触".to_string());
    }
    if !view.can_redo() {
        failures.push("撤销之后应该能重做".to_string());
    }

    let redo_button = view.history_center(HistoryAction::Redo).expect("重做按钮");
    click_at(view, redo_button);
    view.update();
    view.mark_texture_dirty();
    let redone = view.take_texture_upload().expect("重做后应重合成");
    if redone.get_pixel(64, 64) != Color::BLACK {
        failures.push("重做没有恢复画笔笔触".to_string());
    }
}

/// 导出当前文档为 PNG 再从磁盘解码回来核对像素；导入一张 PNG 当新图层。
/// 两个按钮都只留意图（路径由宿主的原生对话框选）。
pub(crate) fn png_io(view: &mut EditorView, failures: &mut Vec<String>) {
    // Phase 7 / 23：导出当前文档为 PNG，再从磁盘解码回来核对像素。
    let export_path = temp_png("export");
    if let Err(error) = view.export_png_to(&export_path.to_string_lossy()) {
        failures.push(format!("导出 PNG 失败：{error}"));
    }
    match crate::io::read_png(&export_path) {
        Ok(exported) => {
            if exported.get_pixel(0, 0) != Color::WHITE {
                failures.push("导出的 PNG 背景不是白色".to_string());
            }
            if exported.get_pixel(64, 64) != Color::BLACK {
                failures.push("导出的 PNG 缺少画笔像素".to_string());
            }
        }
        Err(error) => failures.push(format!("导出的 PNG 无法解码：{error}")),
    }

    // 点「导出 PNG…」按钮只留下意图（路径由宿主原生对话框选）。
    let export_button = view.file_center(FileRequest::ExportPng).expect("导出按钮");
    click_at(view, export_button);
    view.update();
    if view.take_file_request() != Some(FileRequest::ExportPng) {
        failures.push("导出按钮没有留下导出意图".to_string());
    }

    // 导入：独立写一张 4×3 红色 PNG，导入为新图层，
    // 合成后左上角变红（红色图层在最上面）。
    let import_path = temp_png("import");
    let source = PixelBuffer::filled(4, 3, Color::RED);
    if let Err(error) = crate::io::write_png(&import_path, &source) {
        failures.push(format!("测试图写入失败：{error}"));
    } else {
        let before = view.layer_count();
        if let Err(error) = view.import_png_from(&import_path.to_string_lossy()) {
            failures.push(format!("导入 PNG 失败：{error}"));
        }
        view.update();
        if view.layer_count() != before + 1 {
            failures.push("导入没有新增图层".to_string());
        }
        view.mark_texture_dirty();
        let composite = view.take_texture_upload().expect("导入后应重合成");
        if composite.get_pixel(0, 0) != Color::RED {
            failures.push("导入的图层没有进入合成结果".to_string());
        }
    }

    // 点「导入 PNG…」按钮同样只留下意图。
    let import_button = view.file_center(FileRequest::ImportPng).expect("导入按钮");
    click_at(view, import_button);
    view.update();
    if view.take_file_request() != Some(FileRequest::ImportPng) {
        failures.push("导入按钮没有留下导入意图".to_string());
    }
    let _ = std::fs::remove_file(&export_path);
    let _ = std::fs::remove_file(&import_path);
}

/// 吸管取色 -> 框选 -> 移动当前图层。
pub(crate) fn pointer_tools(view: &mut EditorView, failures: &mut Vec<String>) {
    // Phase 8：吸管取色 -> 框选 -> 移动当前图层。
    let eyedropper = view.tool_center(ActiveTool::Eyedropper).expect("吸管按钮");
    click_at(view, eyedropper);
    view.update();
    let black = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));
    click_at(view, black);
    view.update();
    if view.foreground() != Color::BLACK {
        failures.push("吸管没有取到画笔的黑色像素".to_string());
    }

    let select = view
        .tool_center(ActiveTool::RectangleSelect)
        .expect("框选按钮");
    click_at(view, select);
    view.update();
    let select_start = view
        .canvas_camera()
        .document_to_screen(Vec2::new(54.0, 54.0));
    let select_end = view
        .canvas_camera()
        .document_to_screen(Vec2::new(74.0, 74.0));
    view.event(&InputEvent::PointerDown {
        position: select_start,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove {
        position: select_end,
    });
    view.event(&InputEvent::PointerUp {
        position: select_end,
        button: PointerButton::Left,
    });
    view.update();
    let selection = view.selection();
    if selection.is_none() {
        failures.push("框选没有产生选区".to_string());
    }

    let move_button = view.tool_center(ActiveTool::Move).expect("移动按钮");
    click_at(view, move_button);
    view.update();
    let before = view.active_layer_position().unwrap_or_default();
    let move_start = view
        .canvas_camera()
        .document_to_screen(Vec2::new(64.5, 64.5));
    let move_end = move_start + Vec2::new(10.0, 10.0);
    view.event(&InputEvent::PointerDown {
        position: move_start,
        button: PointerButton::Left,
    });
    view.event(&InputEvent::PointerMove { position: move_end });
    view.event(&InputEvent::PointerUp {
        position: move_end,
        button: PointerButton::Left,
    });
    view.update();
    if view.active_layer_position().unwrap_or_default() == before {
        failures.push("移动工具没有改变图层位置".to_string());
    }
}
