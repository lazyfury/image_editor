//! 图层缩略图：每层一张小纹理，列表行里画出来。
//!
//! 缩略图是**显示用的缓存**，不是文档数据：`scaled_to_fit` 把图层缓冲缩到
//! 最长边 [`THUMB_MAX`]，透明区域补棋盘底，再排队等宿主上传成纹理。
//!
//! 成本用两道闸门控制：
//!
//! - 只重建**当前视口里**的行（`ListState::visible_range`）；
//! - 时间节流 [`THUMB_INTERVAL`]：画笔每帧都推进 `revision`，但缩略图 250ms
//!   更新一次就够了，4K 图层不会被每次落笔都整张平均一遍。
//!
//! 纹理 id 进程内全局递增（多个工作区共用同一个后端），按 `LayerId` 缓存，
//! 同一层永远复用同一张纹理。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use igui_render::TextureId;

use crate::document::{LayerId, PixelBuffer};

use super::*;

/// 缩略图最长边（像素）。行高 28，留出边距。
pub const THUMB_MAX: u32 = 22;
/// 两次重建之间的最小间隔。
pub const THUMB_INTERVAL: Duration = Duration::from_millis(250);

/// 分配一个新的图层缩略图纹理 id（进程内唯一）。
fn next_texture() -> TextureId {
    use std::sync::atomic::{AtomicU32, Ordering};
    // 避让文档纹理（1）与主页画廊用的低位区间。
    static NEXT: AtomicU32 = AtomicU32::new(100_000);
    TextureId::new(NEXT.fetch_add(1, Ordering::Relaxed))
}

impl EditorView {
    /// 重建视口内的栅格层缩略图并排队上传（组没有像素，跳过）。
    pub(super) fn rebuild_layer_thumbnails(&mut self) {
        let range = self.layer_state.visible_range();
        let rows: Vec<LayerId> = self
            .layer_rows
            .borrow()
            .iter()
            .enumerate()
            .filter(|(index, _)| range.contains(index))
            .map(|(_, row)| row.id)
            .collect();
        if rows.is_empty() {
            return;
        }
        let state = self.state.borrow();
        let mut textures = self.layer_textures.borrow_mut();
        for id in rows {
            let Some(layer) = state.document.layer(id) else {
                continue;
            };
            if layer.is_group() {
                continue;
            }
            let mut pixels = layer.pixels.scaled_to_fit(THUMB_MAX);
            // 透明区域给个棋盘底，否则缩略图跟面板底色糊在一起。
            crate::canvas::paint_backdrop(&mut pixels);
            let texture = *textures.entry(id).or_insert_with(next_texture);
            self.layer_thumb_uploads.push((texture, pixels));
        }
    }

    /// 取走待上传的图层缩略图（宿主把它灌进后端）。
    pub fn take_layer_thumbnail_uploads(&mut self) -> Vec<(TextureId, PixelBuffer)> {
        std::mem::take(&mut self.layer_thumb_uploads)
    }

    /// 缩略图节流是否到点了（`update` 用）。
    pub(super) fn thumbnails_due(&self) -> bool {
        self.layer_thumb_at.elapsed() >= THUMB_INTERVAL
    }

    /// 标记「刚刚重建过」。
    pub(super) fn mark_thumbnails_rebuilt(&mut self) {
        self.layer_thumb_at = Instant::now();
    }
}

/// 缩略图纹理表（面板行绘制与 [`EditorView`] 共用同一份）。
pub type LayerTextures = HashMap<LayerId, TextureId>;
