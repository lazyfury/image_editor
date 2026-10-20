//! 笔刷预设：一组命名好的参数，选项栏的「预设」菜单用它。
//!
//! 预设只碰**外观参数**（直径 / 不透明度 / 硬度 / 像素模式 / 形状），不改颜色、
//! 选区、画笔 / 橡皮模式 —— 所以选预设不会悄悄把当前工具换成别的。

use super::tip::BrushShape;
use super::BrushTool;

/// 一套笔刷参数。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrushPreset {
    /// 菜单里显示的名字。
    pub name: &'static str,
    /// 直径，文档像素。
    pub size: f32,
    /// 每一笔的不透明度。
    pub opacity: f32,
    /// 0..=1（1 = 只有 1px 抗锯齿边）。
    pub hardness: f32,
    /// 像素模式（覆盖率二值、圆心吸附像素网格）。
    pub hard: bool,
    pub shape: BrushShape,
}

/// 内置预设。第一项是默认的像素硬笔（和 `BrushTool::paint()` 一致）。
pub const PRESETS: &[BrushPreset] = &[
    BrushPreset {
        name: "像素硬笔",
        size: 1.0,
        opacity: 1.0,
        hardness: 1.0,
        hard: true,
        shape: BrushShape::Square,
    },
    BrushPreset {
        name: "像素方块",
        size: 4.0,
        opacity: 1.0,
        hardness: 1.0,
        hard: true,
        shape: BrushShape::Square,
    },
    BrushPreset {
        name: "硬圆笔",
        size: 8.0,
        opacity: 1.0,
        hardness: 1.0,
        hard: false,
        shape: BrushShape::Round,
    },
    BrushPreset {
        name: "柔边圆笔",
        size: 24.0,
        opacity: 1.0,
        hardness: 0.35,
        hard: false,
        shape: BrushShape::Round,
    },
    BrushPreset {
        name: "喷枪",
        size: 48.0,
        opacity: 0.35,
        hardness: 0.0,
        hard: false,
        shape: BrushShape::Round,
    },
    BrushPreset {
        name: "柔方块",
        size: 16.0,
        opacity: 0.8,
        hardness: 0.6,
        hard: false,
        shape: BrushShape::Square,
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
    }
}

/// 当前参数命中的预设下标（逐项相等才算命中）。
pub fn matching(brush: &BrushTool) -> Option<usize> {
    PRESETS.iter().position(|preset| {
        preset.size == brush.size
            && preset.opacity == brush.opacity
            && preset.hardness == brush.hardness
            && preset.hard == brush.hard
            && preset.shape == brush.shape
    })
}

/// 选项栏显示的名字：命中预设就用它的名字，否则 [`CUSTOM`]。
pub fn label(brush: &BrushTool) -> &'static str {
    matching(brush).map_or(CUSTOM, |index| PRESETS[index].name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_brush_matches_the_first_preset() {
        // 默认画笔 = 像素硬笔，所以选项栏一开始不该显示「自定义」。
        assert_eq!(matching(&BrushTool::paint()), Some(0));
        assert_eq!(label(&BrushTool::paint()), "像素硬笔");
    }

    #[test]
    fn applying_a_preset_sets_every_appearance_field() {
        let mut brush = BrushTool::paint();
        PRESETS[4].apply(&mut brush);
        assert_eq!(
            (brush.size, brush.opacity, brush.hardness),
            (PRESETS[4].size, PRESETS[4].opacity, PRESETS[4].hardness)
        );
        assert_eq!(
            (brush.hard, brush.shape),
            (PRESETS[4].hard, PRESETS[4].shape)
        );
        assert_eq!(label(&brush), PRESETS[4].name);
    }

    #[test]
    fn applying_a_preset_keeps_the_colour_and_the_mode() {
        // 选预设不能把颜色 / 画笔-橡皮模式一起改掉。
        let mut brush = BrushTool::eraser().with_color(crate::document::Color::RED);
        PRESETS[3].apply(&mut brush);
        assert_eq!(brush.mode, super::super::BrushMode::Erase);
        assert_eq!(brush.color, crate::document::Color::RED);
    }

    #[test]
    fn tweaking_any_field_falls_back_to_custom() {
        let mut brush = BrushTool::paint();
        PRESETS[0].apply(&mut brush);
        brush.hardness = 0.5;
        assert_eq!(matching(&brush), None);
        assert_eq!(label(&brush), CUSTOM);
    }
}
