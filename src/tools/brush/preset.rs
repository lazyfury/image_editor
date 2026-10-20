//! 笔刷预设：选项栏「预设 ▾」菜单用的命名参数组合。
//!
//! 预设只是**一组外观参数**（直径 / 不透明度 / 硬度 / 像素模式 / 形状 / 间距），
//! 不涉及任何素材文件。当前画笔的参数正好等于某条预设时，选项栏显示它的名字，
//! 否则显示 [`CUSTOM`]。

use std::borrow::Cow;

use super::tip::BrushShape;
use super::BrushTool;

/// 一套笔刷参数。
#[derive(Debug, Clone, PartialEq)]
pub struct BrushPreset {
    /// 菜单里显示的名字。
    pub name: Cow<'static, str>,
    /// 直径，文档像素。
    pub size: f32,
    /// 每一笔的不透明度。
    pub opacity: f32,
    /// 0..=1（1 = 只有 1px 抗锯齿边）。
    pub hardness: f32,
    /// 像素模式（覆盖率二值、圆心吸附像素网格）。
    pub hard: bool,
    pub shape: BrushShape,
    /// 落笔间距相对直径的比例。
    pub spacing_ratio: f32,
}

/// 内置预设。第一项是默认的像素硬笔（和 `BrushTool::paint()` 一致）。
pub const PRESETS: &[BrushPreset] = &[
    BrushPreset {
        name: Cow::Borrowed("像素硬笔"),
        size: 1.0,
        opacity: 1.0,
        hardness: 1.0,
        hard: true,
        shape: BrushShape::Square,
        spacing_ratio: 0.25,
    },
    BrushPreset {
        name: Cow::Borrowed("像素方块"),
        size: 4.0,
        opacity: 1.0,
        hardness: 1.0,
        hard: true,
        shape: BrushShape::Square,
        spacing_ratio: 0.25,
    },
    BrushPreset {
        name: Cow::Borrowed("硬圆笔"),
        size: 8.0,
        opacity: 1.0,
        hardness: 1.0,
        hard: false,
        shape: BrushShape::Round,
        spacing_ratio: 0.25,
    },
    BrushPreset {
        name: Cow::Borrowed("柔边圆笔"),
        size: 24.0,
        opacity: 1.0,
        hardness: 0.35,
        hard: false,
        shape: BrushShape::Round,
        spacing_ratio: 0.25,
    },
    BrushPreset {
        name: Cow::Borrowed("喷枪"),
        size: 48.0,
        opacity: 0.35,
        hardness: 0.0,
        hard: false,
        shape: BrushShape::Round,
        spacing_ratio: 0.25,
    },
    BrushPreset {
        name: Cow::Borrowed("柔方块"),
        size: 16.0,
        opacity: 0.8,
        hardness: 0.6,
        hard: false,
        shape: BrushShape::Square,
        spacing_ratio: 0.25,
    },
];

/// 参数不命中任何预设时选项栏显示的名字。
pub const CUSTOM: &str = "自定义";

impl BrushPreset {
    /// 把预设写进画笔（只改外观参数）。
    pub fn apply(&self, brush: &mut BrushTool) {
        brush.size = self.size;
        brush.opacity = self.opacity;
        brush.hardness = self.hardness;
        brush.hard = self.hard;
        brush.shape = self.shape;
        brush.spacing_ratio = self.spacing_ratio;
    }
}

/// 当前参数在 `presets` 里命中的下标（逐项相等才算命中）。
pub fn matching(presets: &[BrushPreset], brush: &BrushTool) -> Option<usize> {
    presets.iter().position(|preset| {
        preset.size == brush.size
            && preset.opacity == brush.opacity
            && preset.hardness == brush.hardness
            && preset.hard == brush.hard
            && preset.shape == brush.shape
            && (preset.spacing_ratio - brush.spacing_ratio).abs() < 1e-6
    })
}

/// 选项栏显示的名字：命中预设就用它的名字，否则 [`CUSTOM`]。
pub fn label(presets: &[BrushPreset], brush: &BrushTool) -> String {
    matching(presets, brush).map_or_else(
        || CUSTOM.to_string(),
        |index| presets[index].name.to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_brush_matches_the_first_preset() {
        // 默认画笔 = 像素硬笔，所以选项栏一开始不该显示「自定义」。
        assert_eq!(matching(PRESETS, &BrushTool::paint()), Some(0));
        assert_eq!(label(PRESETS, &BrushTool::paint()), "像素硬笔");
    }

    #[test]
    fn applying_a_preset_sets_every_appearance_field() {
        let mut brush = BrushTool::paint();
        PRESETS[3].apply(&mut brush);
        assert_eq!(
            (brush.size, brush.opacity, brush.hardness),
            (PRESETS[3].size, PRESETS[3].opacity, PRESETS[3].hardness)
        );
        assert_eq!(
            (brush.hard, brush.shape, brush.spacing_ratio),
            (PRESETS[3].hard, PRESETS[3].shape, PRESETS[3].spacing_ratio)
        );
        assert_eq!(matching(PRESETS, &brush), Some(3), "应用后应命中该预设");
    }

    #[test]
    fn a_hand_tuned_brush_falls_back_to_custom() {
        let mut brush = BrushTool::paint();
        brush.size = 7.0;
        assert_eq!(matching(PRESETS, &brush), None);
        assert_eq!(label(PRESETS, &brush), CUSTOM);
    }

    #[test]
    fn the_first_preset_is_the_default_brush() {
        // 列表第一项必须与 `BrushTool::paint()` 完全一致，否则启动就显示「自定义」。
        let brush = BrushTool::paint();
        let first = &PRESETS[0];
        assert_eq!(
            (first.size, first.opacity, first.hardness, first.hard),
            (brush.size, brush.opacity, brush.hardness, brush.hard)
        );
        assert_eq!(
            (first.shape, first.spacing_ratio),
            (brush.shape, brush.spacing_ratio)
        );
    }
}
