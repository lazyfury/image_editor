//! 撤销栈与命令的测试（栈语义、区域、平移、图层命令）。

use super::*;
use crate::document::{Color, Document, Layer, PixelBuffer};

fn document() -> Document {
    Document::new("d", 8, 8)
}

/// 直接写一个像素，构造一条命令（不经过画笔）。
fn paint_command(document: &mut Document) -> PaintCommand {
    let id = document.active_layer.unwrap();
    let before = document.layer(id).unwrap().pixels.clone();
    // 来回切换颜色，好让同一个 helper 能在一次历史里反复造命令。
    let next = if before.get_pixel(4, 4) == Color::RED {
        Color::BLACK
    } else {
        Color::RED
    };
    document.layer_mut(id).unwrap().pixels.set_pixel(4, 4, next);
    let after = document.layer(id).unwrap().pixels.clone();
    PaintCommand::capture(id, &before, &after, "画笔").expect("有变化")
}

fn pixel(document: &Document, x: u32, y: u32) -> Color {
    document.active_layer().unwrap().pixels.get_pixel(x, y)
}

#[test]
fn execute_records_and_clears_redo() {
    let mut document = document();
    let mut history = History::new();
    let command = paint_command(&mut document);
    history.execute(Box::new(command), &mut document);

    assert!(history.can_undo());
    assert!(!history.can_redo());
    assert_eq!(history.undo_len(), 1);
}

#[test]
fn undo_restores_and_redo_reapplies() {
    let mut document = document();
    let mut history = History::new();
    let command = paint_command(&mut document);
    history.execute(Box::new(command), &mut document);
    assert_eq!(pixel(&document, 4, 4), Color::RED);

    assert_eq!(history.undo(&mut document), Some("画笔"));
    assert_eq!(pixel(&document, 4, 4), Color::WHITE);
    assert!(history.can_redo());

    assert_eq!(history.redo(&mut document), Some("画笔"));
    assert_eq!(pixel(&document, 4, 4), Color::RED);
    assert!(!history.can_redo());
}

#[test]
fn undo_and_redo_return_none_on_empty_stacks() {
    let mut document = document();
    let mut history = History::new();
    assert_eq!(history.undo(&mut document), None);
    assert_eq!(history.redo(&mut document), None);
    assert!(!history.can_undo());
}

#[test]
fn a_new_command_clears_the_redo_stack() {
    let mut document = document();
    let mut history = History::new();
    let first = paint_command(&mut document);
    history.execute(Box::new(first), &mut document);
    history.undo(&mut document);
    assert!(history.can_redo());

    let second = paint_command(&mut document);
    history.execute(Box::new(second), &mut document);
    assert!(!history.can_redo(), "新命令之后 redo 栈应清空");
    assert_eq!(history.undo_len(), 1);
}

#[test]
fn the_stack_is_bounded_by_the_limit() {
    let mut document = document();
    let mut history = History::with_limit(2);
    for _ in 0..5 {
        let command = paint_command(&mut document);
        history.execute(Box::new(command), &mut document);
    }
    assert_eq!(history.undo_len(), 2);
}

#[test]
fn undo_touches_the_document_revision() {
    let mut document = document();
    let mut history = History::new();
    let command = paint_command(&mut document);
    history.execute(Box::new(command), &mut document);
    let revision = document.revision();
    history.undo(&mut document);
    assert_eq!(document.revision(), revision + 1, "撤销要让视图重合成");
}

#[test]
fn labels_track_the_undo_and_redo_stacks() {
    let mut document = document();
    let mut history = History::new();
    let command = paint_command(&mut document);
    history.execute(Box::new(command), &mut document);
    assert_eq!(history.undo_labels(), vec!["画笔"]);
    assert!(history.redo_labels().is_empty());

    history.undo(&mut document);
    assert!(history.undo_labels().is_empty());
    assert_eq!(history.redo_labels(), vec!["画笔"]);
}

#[test]
fn layer_meta_command_restores_metadata_and_order() {
    let mut document = document();
    let bottom = document.layers[0].id;
    let top = document.add_layer("top");

    let before = LayerMetaCommand::capture(&document);
    document.set_layer_visible(bottom, false);
    document.rename_layer(top, "重命名");
    document.set_layer_opacity(top, 0.5);
    document.move_layer(bottom, usize::MAX);
    let after = LayerMetaCommand::capture(&document);

    let mut command = LayerMetaCommand::new(before, after, "图层元数据");
    command.undo(&mut document);
    assert!(document.layer(bottom).unwrap().visible);
    assert_eq!(document.layer(top).unwrap().name, "top");
    assert_eq!(document.layer(top).unwrap().opacity, 1.0);
    assert_eq!(document.layers[0].id, bottom, "顺序复原");

    command.execute(&mut document);
    assert!(!document.layer(bottom).unwrap().visible);
    assert_eq!(document.layer(top).unwrap().name, "重命名");
    assert_eq!(document.layers.last().unwrap().id, bottom);
}

#[test]
fn add_and_remove_layer_commands_round_trip() {
    let mut document = document();
    let before_active = document.active_layer;
    let layer = Layer::new("L", PixelBuffer::filled(2, 2, Color::RED));
    let id = layer.id;

    let mut add = AddLayerCommand::new(layer, document.layers.len(), before_active);
    add.execute(&mut document);
    assert!(document.layer(id).is_some());
    assert_eq!(document.active_layer, Some(id));
    add.undo(&mut document);
    assert!(document.layer(id).is_none());
    add.execute(&mut document);
    assert!(document.layer(id).is_some());

    let mut remove = RemoveLayerCommand::new(id, Some(id));
    remove.execute(&mut document);
    assert!(document.layer(id).is_none());
    remove.undo(&mut document);
    assert!(document.layer(id).is_some(), "撤销删除把图层插回");
}
