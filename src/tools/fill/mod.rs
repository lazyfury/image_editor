//! 填充类工具：洪水填充（容差 / 连续 / 全局）+ 按选区纯色填充。
//!
//! - `flood.rs`：纯算法 —— [`flood_fill`]（只认 `PixelBuffer` + 文档坐标的
//!   [`Selection`]）与 [`magic_wand_region`]（同一套容差扫描，返回掩码）；
//! - `bucket.rs`：[`PaintBucketTool`]，把洪水填充包成 [`Tool`]（落笔前快照整层、
//!   填充后用 `PixelRegion::diff` 抓差异，一条 undo）；
//! - `selection.rs`：[`fill_selection`]，矩形 / 自由 / 钢笔填充共用的一条纯色填充。

mod bucket;
mod flood;
mod selection;

#[cfg(test)]
mod tests;

pub use bucket::PaintBucketTool;
pub use flood::magic_wand_region;
pub use selection::fill_selection;
