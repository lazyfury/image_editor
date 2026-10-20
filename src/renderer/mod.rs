//! 合成器：把 [`Document`](crate::document::Document) 的图层栈画成一张像素图。
//!
//! §3.2 / §13：先定一个后端无关的 [`Renderer`] trait，第一版只有 CPU 实现。
//! 业务（文档、图层）不认识 wgpu；以后加 `WgpuRenderer` 时替换的是这个
//! 接口，而不是把渲染塞进文档模型。
//!
//! 支持**按脏矩形局部合成**（[`Renderer::render_region`]）：画笔一笔只写几百
//! 到几千像素，4K 文档不必每帧重算 830 万像素。整图合成仍然保留（导出、尺寸
//! 变化、图层增删等场景）。

mod cpu;
mod target;

pub use cpu::{sample_pixel, CpuRenderer};
pub use target::RenderTarget;

use crate::document::{Document, PixelRegion};

/// 把文档合成到目标上。
pub trait Renderer {
    /// 整图合成（导出 / 尺寸变化 / 结构性改动）。
    fn render(&self, document: &Document, target: &mut RenderTarget);

    /// 只重算 `region`（目标坐标），其余像素保持上一帧的内容。
    ///
    /// 默认退回整图合成，所以只有真的实现了区域的合成器才需要覆盖它。
    fn render_region(&self, document: &Document, target: &mut RenderTarget, region: PixelRegion) {
        let _ = region;
        self.render(document, target);
    }
}
