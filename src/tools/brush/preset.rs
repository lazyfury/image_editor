//! 笔刷预设：一组命名好的参数，选项栏的「预设」菜单用它。
//!
//! 预设只碰**外观参数**（直径 / 不透明度 / 硬度 / 像素模式 / 形状 / 颗粒 /
//! 个数 / 散布 / 间距），不改颜色、选区、画笔 / 橡皮模式 —— 所以选预设不会悄悄
//! 把当前工具换成别的。
//!
//! 三处来源，启动时合成一次（[`PresetLibrary::load`]）：
//! 1. 内置 [`PRESETS`]（第一项是默认的像素硬笔，和 `BrushTool::paint()` 一致）；
//! 2. `<素材目录>/presets.txt` 里手写的；
//! 3. 素材目录里**没被前两者引用**的 PNG，每个自动成一条（丢进去就能用）。

use std::borrow::Cow;
use std::rc::Rc;

use super::grain::{Grain, GrainMapping};
use super::texture::BrushTextures;
use super::tip::BrushShape;
use super::BrushTool;

/// 一套笔刷参数。
#[derive(Debug, Clone, PartialEq)]
pub struct BrushPreset {
    /// 菜单里显示的名字（内置的是 `&'static str`，用户预设读自文本文件）。
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
    /// 颗粒（纹理 + 映射 + 深度）；`texture = None` = 程序化笔尖。
    pub grain: GrainSpec,
    /// 每个采样点压几个图章（1..=16）。
    pub count: u32,
    /// 图章的随机偏移幅度（相对直径）。
    pub scatter: f32,
    /// 图章的随机旋转幅度（0..=1：1 = 0..360° 全随机）。
    pub scatter_rotation: f32,
    /// 落笔间距相对直径的比例。
    pub spacing_ratio: f32,
}

/// 预设里的颗粒参数：纹理名 + 怎么铺 + 深浅。
#[derive(Debug, Clone, PartialEq)]
pub struct GrainSpec {
    /// 纹理名（`<素材目录>/<name>.png`）；`None` = 程序化笔尖。
    pub texture: Option<Cow<'static, str>>,
    pub mapping: GrainMapping,
    /// `Stamp` 映射的固定角度（度）。
    pub angle: f32,
    /// 一块纹理覆盖几个直径。
    pub scale: f32,
    /// 0..=1：0 = 纯底色，1 = 纯颗粒。
    pub depth: f32,
    /// 0..=1：抬最暗处（稀疏毛束别把笔迹切成虚线）。
    pub min_depth: f32,
    /// 颗粒对比度（1 = 原样）。
    pub contrast: f32,
    /// 颗粒亮度偏移（-1..=1）。
    pub brightness: f32,
}

impl GrainSpec {
    /// 没有颗粒：程序化笔尖。
    pub const fn none() -> Self {
        Self {
            texture: None,
            mapping: GrainMapping::Stamp,
            angle: 0.0,
            scale: 1.0,
            depth: 1.0,
            min_depth: 0.0,
            contrast: 1.0,
            brightness: 0.0,
        }
    }

    /// 一张素材图 + 铺法。
    pub const fn textured(name: &'static str, mapping: GrainMapping) -> Self {
        Self {
            texture: Some(Cow::Borrowed(name)),
            mapping,
            angle: 0.0,
            scale: 1.0,
            depth: 1.0,
            min_depth: 0.0,
            contrast: 1.0,
            brightness: 0.0,
        }
    }

    pub const fn with_depth(mut self, depth: f32) -> Self {
        self.depth = depth;
        self
    }

    pub const fn with_min_depth(mut self, min_depth: f32) -> Self {
        self.min_depth = min_depth;
        self
    }

    pub const fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    pub const fn with_contrast(mut self, contrast: f32) -> Self {
        self.contrast = contrast;
        self
    }

    /// 按名字从素材库取图，做成画笔上的 [`Grain`]（取不到 = 程序化笔尖）。
    pub fn resolve(&self, textures: &BrushTextures) -> Option<Grain> {
        let texture = self
            .texture
            .as_deref()
            .and_then(|name| textures.get(name))?;
        Some(
            Grain::new(texture)
                .with_mapping(self.mapping)
                .with_angle(self.angle.to_radians())
                .with_scale(self.scale)
                .with_depth(self.depth)
                .with_min_depth(self.min_depth)
                .with_contrast(self.contrast)
                .with_brightness(self.brightness),
        )
    }
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
        grain: GrainSpec::none(),
        count: 1,
        scatter: 0.0,
        scatter_rotation: 0.0,
        spacing_ratio: 0.25,
    },
    BrushPreset {
        name: Cow::Borrowed("像素方块"),
        size: 4.0,
        opacity: 1.0,
        hardness: 1.0,
        hard: true,
        shape: BrushShape::Square,
        grain: GrainSpec::none(),
        count: 1,
        scatter: 0.0,
        scatter_rotation: 0.0,
        spacing_ratio: 0.25,
    },
    BrushPreset {
        name: Cow::Borrowed("硬圆笔"),
        size: 8.0,
        opacity: 1.0,
        hardness: 1.0,
        hard: false,
        shape: BrushShape::Round,
        grain: GrainSpec::none(),
        count: 1,
        scatter: 0.0,
        scatter_rotation: 0.0,
        spacing_ratio: 0.25,
    },
    BrushPreset {
        name: Cow::Borrowed("柔边圆笔"),
        size: 24.0,
        opacity: 1.0,
        hardness: 0.35,
        hard: false,
        shape: BrushShape::Round,
        grain: GrainSpec::none(),
        count: 1,
        scatter: 0.0,
        scatter_rotation: 0.0,
        spacing_ratio: 0.25,
    },
    BrushPreset {
        name: Cow::Borrowed("喷枪"),
        size: 48.0,
        opacity: 0.35,
        hardness: 0.0,
        hard: false,
        shape: BrushShape::Round,
        grain: GrainSpec::none(),
        count: 1,
        scatter: 0.0,
        scatter_rotation: 0.0,
        spacing_ratio: 0.25,
    },
    BrushPreset {
        name: Cow::Borrowed("柔方块"),
        size: 16.0,
        opacity: 0.8,
        hardness: 0.6,
        hard: false,
        shape: BrushShape::Square,
        grain: GrainSpec::none(),
        count: 1,
        scatter: 0.0,
        scatter_rotation: 0.0,
        spacing_ratio: 0.25,
    },
    BrushPreset {
        // 颗粒跟笔滚动：整条笔迹的毛是连续的。
        name: Cow::Borrowed("毛笔"),
        size: 48.0,
        opacity: 1.0,
        hardness: 1.0,
        hard: false,
        shape: BrushShape::Round,
        grain: GrainSpec::textured("ink", GrainMapping::Dragged),
        count: 1,
        scatter: 0.0,
        scatter_rotation: 0.0,
        spacing_ratio: 0.12,
    },
    BrushPreset {
        name: Cow::Borrowed("鬃刷"),
        size: 64.0,
        opacity: 1.0,
        hardness: 1.0,
        hard: false,
        shape: BrushShape::Round,
        grain: GrainSpec::textured("bristle", GrainMapping::Dragged),
        count: 1,
        scatter: 0.0,
        scatter_rotation: 0.0,
        spacing_ratio: 0.1,
    },
    BrushPreset {
        // 颗粒钉在画布上：一整条笔迹共享同一片纹理，像"一片区域被描过"。
        name: Cow::Borrowed("纹理化刷"),
        size: 56.0,
        opacity: 1.0,
        hardness: 1.0,
        hard: false,
        shape: BrushShape::Round,
        grain: GrainSpec::textured("ink", GrainMapping::CanvasLocked)
            .with_depth(0.8)
            .with_min_depth(0.15)
            // 一块纹理 = 4 个直径：短笔迹里根本看不出重复（"一片区域"的观感）。
            .with_scale(4.0),
        count: 1,
        scatter: 0.0,
        scatter_rotation: 0.0,
        spacing_ratio: 0.08,
    },
    BrushPreset {
        // 每个采样点压 3 个图章、各自随机旋转 / 偏移：干刷的飞白。
        name: Cow::Borrowed("干刷"),
        size: 52.0,
        opacity: 0.9,
        hardness: 1.0,
        hard: false,
        shape: BrushShape::Round,
        grain: GrainSpec::textured("bristle", GrainMapping::Dragged).with_depth(0.9),
        count: 3,
        scatter: 0.08,
        scatter_rotation: 1.0,
        spacing_ratio: 0.08,
    },
    BrushPreset {
        // 草：数量多 + 大散布，就是 Procreate 的「散点草」。
        name: Cow::Borrowed("草"),
        size: 40.0,
        opacity: 1.0,
        hardness: 1.0,
        hard: false,
        shape: BrushShape::Round,
        grain: GrainSpec::textured("grass", GrainMapping::Dragged),
        count: 4,
        scatter: 0.45,
        scatter_rotation: 1.0,
        spacing_ratio: 0.12,
    },
];

/// 参数不命中任何预设时选项栏显示的名字。
pub const CUSTOM: &str = "自定义";

impl BrushPreset {
    /// 把预设写进画笔（只改外观参数）。纹理按名字从素材库取，取不到就是程序化笔尖。
    pub fn apply(&self, brush: &mut BrushTool, textures: &BrushTextures) {
        brush.size = self.size;
        brush.opacity = self.opacity;
        brush.hardness = self.hardness;
        brush.hard = self.hard;
        brush.shape = self.shape;
        brush.grain = self.grain.resolve(textures);
        brush.count = self.count;
        brush.scatter = self.scatter;
        brush.scatter_rotation = self.scatter_rotation;
        brush.spacing_ratio = self.spacing_ratio;
    }
}

/// 当前参数在 `presets` 里命中的下标（逐项相等才算命中）。
///
/// 纹理按 `Rc` 指针比（素材是缓存共享的），避免每帧比较几万字节的蒙版。
pub fn matching(
    presets: &[BrushPreset],
    brush: &BrushTool,
    textures: &BrushTextures,
) -> Option<usize> {
    presets.iter().position(|preset| {
        let expected = preset.grain.resolve(textures);
        let same_grain = match (expected, brush.grain.as_ref()) {
            (None, None) => true,
            (Some(expected), Some(current)) => {
                Rc::ptr_eq(current.texture(), expected.texture())
                    && current.mapping == expected.mapping
                    && (current.angle - expected.angle).abs() < 1e-6
                    && (current.scale - expected.scale).abs() < 1e-6
                    && (current.depth - expected.depth).abs() < 1e-6
                    && (current.min_depth - expected.min_depth).abs() < 1e-6
            }
            _ => false,
        };
        preset.size == brush.size
            && preset.opacity == brush.opacity
            && preset.hardness == brush.hardness
            && preset.hard == brush.hard
            && preset.shape == brush.shape
            && preset.count == brush.count
            && (preset.scatter - brush.scatter).abs() < 1e-6
            && (preset.scatter_rotation - brush.scatter_rotation).abs() < 1e-6
            && (preset.spacing_ratio - brush.spacing_ratio).abs() < 1e-6
            && same_grain
    })
}

/// 选项栏显示的名字：命中预设就用它的名字，否则 [`CUSTOM`]。
pub fn label(presets: &[BrushPreset], brush: &BrushTool, textures: &BrushTextures) -> String {
    matching(presets, brush, textures).map_or_else(
        || CUSTOM.to_string(),
        |index| presets[index].name.to_string(),
    )
}

/// 预设库：内置预设 + 用户目录里的 `presets.txt` + 目录里没被引用的 PNG。
#[derive(Debug, Clone, Default)]
pub struct PresetLibrary {
    presets: Vec<BrushPreset>,
}

/// 用户文件格式（每行一条，`#` 开头是注释）：
///
/// ```text
/// 名字 | 纹理名 | 直径 | 不透明度 | 硬度 | 间距比例 | 铺法 | 深度 | 颗粒比例 | 个数 | 散布 | 对比
/// 我的草 | grass | 36 | 1 | 1 | 0.08 | along | 1 | 1 | 4 | 0.4
/// 干刷   | my-brush | 64 | 0.8 | 0.6 | 0.15 | 30
/// ```
///
/// 第 7 段：`along` = 跟笔滚动、`canvas` = 钉在画布、数字 = 固定角度（度）。
/// 后面几段可省略（默认 深度 1 / 比例 1 / 个数 1 / 散布 0 / 对比 1）。
/// 坏行跳过，不会让编辑器起不来。
impl PresetLibrary {
    /// 内置 + `presets.txt` 里写的 + 目录里没被引用的 PNG（每个自动成一条）。
    pub fn load(textures: &BrushTextures) -> Self {
        let mut presets = PRESETS.to_vec();
        let user = user_presets(textures);
        // 已经被内置 / presets.txt 引用的纹理不再自动生成。
        let mut referenced: Vec<String> = presets
            .iter()
            .filter_map(|preset| preset.grain.texture.as_deref().map(str::to_string))
            .chain(
                user.iter()
                    .filter_map(|preset| preset.grain.texture.as_deref().map(str::to_string)),
            )
            .collect();
        presets.extend(user);
        presets.extend(auto_presets(textures, &mut referenced));
        Self { presets }
    }

    pub fn all(&self) -> &[BrushPreset] {
        &self.presets
    }

    pub fn get(&self, index: usize) -> Option<&BrushPreset> {
        self.presets.get(index)
    }

    pub fn matching(&self, brush: &BrushTool, textures: &BrushTextures) -> Option<usize> {
        matching(&self.presets, brush, textures)
    }

    pub fn label(&self, brush: &BrushTool, textures: &BrushTextures) -> String {
        label(&self.presets, brush, textures)
    }
}

/// 自动预设最多收这么多（素材目录指向一个大文件夹时别把菜单塞爆）。
const MAX_AUTO_PRESETS: usize = 24;

/// 素材目录里那些没被引用的 PNG，每个都自动变成一条预设。
fn auto_presets(textures: &BrushTextures, referenced: &mut Vec<String>) -> Vec<BrushPreset> {
    let mut presets = Vec::new();
    for name in textures.names() {
        if presets.len() >= MAX_AUTO_PRESETS {
            break;
        }
        if referenced.iter().any(|used| used == &name) {
            continue;
        }
        // 只收真的能加载的（坏 PNG / 空图跳过）。
        if textures.get(&name).is_none() {
            continue;
        }
        referenced.push(name.clone());
        presets.push(BrushPreset {
            name: Cow::Owned(name.clone()),
            size: 64.0,
            opacity: 1.0,
            hardness: 1.0,
            hard: false,
            shape: BrushShape::Round,
            grain: GrainSpec {
                texture: Some(Cow::Owned(name)),
                mapping: GrainMapping::Dragged,
                angle: 0.0,
                scale: 1.0,
                // 用户素材多半是"一根笔触"（大片透明），当颗粒直接铺会断成虚线：
                // 留 30% 底线 + 只取 80% 深度，笔迹永远是连续的，纹理浮在上面。
                depth: 0.8,
                min_depth: 0.3,
                contrast: 1.0,
                brightness: 0.0,
            }
            // 素材自带明暗就够，稍微拉开一点点（偏白的图才看得出纹理）。
            .with_contrast(1.5),
            count: 1,
            scatter: 0.0,
            scatter_rotation: 0.0,
            spacing_ratio: 0.1,
        });
    }
    presets
}

/// 读用户预设（`<素材目录>/presets.txt`）；没有文件就是空列表。
fn user_presets(textures: &BrushTextures) -> Vec<BrushPreset> {
    let Some(dir) = textures.dir() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(dir.join("presets.txt")) else {
        return Vec::new();
    };
    parse_user_presets(&text)
}

/// 解析用户预设文本（坏行跳过）。
fn parse_user_presets(text: &str) -> Vec<BrushPreset> {
    let mut presets = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('|').map(str::trim).collect();
        if fields.len() < 7 {
            continue;
        }
        let number = |value: &str| value.parse::<f32>().ok();
        let (Some(size), Some(opacity), Some(hardness), Some(spacing)) = (
            number(fields[2]),
            number(fields[3]),
            number(fields[4]),
            number(fields[5]),
        ) else {
            continue;
        };
        // 第 7 段：`along` / `canvas` = 跟笔 / 钉画布，数字 = 固定角度（度）。
        let (mapping, angle) = match fields[6].to_ascii_lowercase().as_str() {
            "along" | "dragged" | "跟笔" => (GrainMapping::Dragged, 0.0),
            "canvas" | "locked" | "画布" => (GrainMapping::CanvasLocked, 0.0),
            _ => {
                let Some(degrees) = number(fields[6]) else {
                    continue;
                };
                (GrainMapping::Stamp, degrees)
            }
        };
        // 可选后缀：| 深度 | 颗粒比例 | 个数 | 散布
        let depth = fields.get(7).and_then(|value| number(value)).unwrap_or(1.0);
        let scale = fields.get(8).and_then(|value| number(value)).unwrap_or(1.0);
        let count = fields
            .get(9)
            .and_then(|value| number(value))
            .unwrap_or(1.0)
            .round()
            .clamp(1.0, super::MAX_STAMPS as f32) as u32;
        let scatter = fields
            .get(10)
            .and_then(|value| number(value))
            .unwrap_or(0.0);
        let contrast = fields
            .get(11)
            .and_then(|value| number(value))
            .unwrap_or(1.0);
        presets.push(BrushPreset {
            name: Cow::Owned(fields[0].to_string()),
            size: size.clamp(BrushTool::MIN_SIZE, BrushTool::MAX_SIZE),
            opacity: opacity.clamp(0.05, 1.0),
            hardness: hardness.clamp(0.05, 1.0),
            hard: false,
            shape: BrushShape::Round,
            grain: GrainSpec {
                texture: Some(Cow::Owned(fields[1].to_string())),
                mapping,
                angle,
                scale: scale.clamp(0.05, 8.0),
                depth: depth.clamp(0.0, 1.0),
                min_depth: 0.0,
                contrast: contrast.clamp(0.0, 8.0),
                brightness: 0.0,
            },
            count,
            scatter: scatter.clamp(0.0, 4.0),
            scatter_rotation: if scatter > 0.0 { 1.0 } else { 0.0 },
            spacing_ratio: spacing.clamp(0.02, 1.0),
        });
    }
    presets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn textures() -> BrushTextures {
        BrushTextures::default()
    }

    #[test]
    fn a_fresh_brush_matches_the_first_preset() {
        // 默认画笔 = 像素硬笔，所以选项栏一开始不该显示「自定义」。
        let textures = textures();
        assert_eq!(matching(PRESETS, &BrushTool::paint(), &textures), Some(0));
        assert_eq!(label(PRESETS, &BrushTool::paint(), &textures), "像素硬笔");
    }

    #[test]
    fn applying_a_preset_sets_every_appearance_field() {
        let textures = textures();
        let mut brush = BrushTool::paint();
        PRESETS[4].apply(&mut brush, &textures);
        assert_eq!(
            (brush.size, brush.opacity, brush.hardness),
            (PRESETS[4].size, PRESETS[4].opacity, PRESETS[4].hardness)
        );
        assert_eq!(
            (brush.hard, brush.shape),
            (PRESETS[4].hard, PRESETS[4].shape)
        );
        assert_eq!(label(PRESETS, &brush, &textures), PRESETS[4].name);
    }

    #[test]
    fn the_textured_presets_load_their_brush_tips() {
        // 带素材的预设应用后画笔上真的挂了一张纹理。
        let textures = textures();
        let mut brush = BrushTool::paint();
        let textured: Vec<&BrushPreset> = PRESETS
            .iter()
            .filter(|preset| preset.grain.texture.is_some())
            .collect();
        assert!(textured.len() >= 5, "毛笔 / 鬃刷 / 纹理化刷 / 干刷 / 草");
        for preset in textured {
            preset.apply(&mut brush, &textures);
            let grain = brush
                .grain
                .clone()
                .unwrap_or_else(|| panic!("{} 应该挂上颗粒", preset.name));
            assert_eq!(grain.texture().size(), (128, 128), "{} 的颗粒", preset.name);
            assert!(
                preset.spacing_ratio < 0.25,
                "纹理笔的间距应该更小（连续）：{}",
                preset.spacing_ratio
            );
            assert_eq!(label(PRESETS, &brush, &textures), preset.name);
        }
    }

    #[test]
    fn the_grain_presets_cover_all_three_mappings() {
        // 三态各有代表：纯笔尖、跟笔滚动、钉在画布。
        let mappings: Vec<GrainMapping> = PRESETS
            .iter()
            .filter(|preset| preset.grain.texture.is_some())
            .map(|preset| preset.grain.mapping)
            .collect();
        assert!(mappings.contains(&GrainMapping::Dragged));
        assert!(mappings.contains(&GrainMapping::CanvasLocked));
        // 有散布的预设（干刷 / 草）数量都 > 1。
        assert!(PRESETS.iter().any(|preset| preset.count > 1));
        assert!(PRESETS.iter().any(|preset| preset.scatter > 0.0));
    }

    #[test]
    fn applying_a_preset_keeps_the_colour_and_the_mode() {
        // 选预设不能把颜色 / 画笔-橡皮模式一起改掉。
        let textures = textures();
        let mut brush = BrushTool::eraser().with_color(crate::document::Color::RED);
        PRESETS[3].apply(&mut brush, &textures);
        assert_eq!(brush.mode, super::super::BrushMode::Erase);
        assert_eq!(brush.color, crate::document::Color::RED);
    }

    #[test]
    fn tweaking_any_field_falls_back_to_custom() {
        let textures = textures();
        let mut brush = BrushTool::paint();
        PRESETS[0].apply(&mut brush, &textures);
        brush.hardness = 0.5;
        assert_eq!(matching(PRESETS, &brush, &textures), None);
        assert_eq!(label(PRESETS, &brush, &textures), CUSTOM);
    }

    #[test]
    fn a_user_preset_file_parses_into_extra_presets() {
        let text = "\
# 注释行
我的草 | grass | 36 | 1 | 1 | 0.08 | along | 1 | 1 | 4 | 0.4
干刷 | bristle | 64 | 0.8 | 0.6 | 0.15 | 30
坏行没有竖线
缺字段 | grass | 36
";
        let presets = parse_user_presets(text);
        assert_eq!(presets.len(), 2, "坏行要跳过：{presets:?}");
        assert_eq!(presets[0].name, "我的草");
        assert_eq!(presets[0].grain.texture.as_deref(), Some("grass"), "纹理名");
        assert_eq!(presets[0].grain.mapping, GrainMapping::Dragged);
        assert_eq!(presets[0].count, 4, "可选后缀里的个数");
        assert!((presets[0].scatter - 0.4).abs() < 1e-6, "可选后缀里的散布");
        assert!((presets[0].spacing_ratio - 0.08).abs() < 1e-6);
        // 数字 = 固定角度（度）。
        assert_eq!(presets[1].grain.mapping, GrainMapping::Stamp);
        assert!((presets[1].grain.angle - 30.0).abs() < 1e-6);
        assert_eq!(presets[1].count, 1, "没写后缀就是默认值");
    }

    #[test]
    fn the_library_puts_builtin_presets_first_and_user_ones_after() {
        // 内置那套顺序不能乱（默认预设 = 下标 0）；仓库素材已经被内置引用，
        // 所以自动那条不会再重复出现。
        let textures = textures();
        let library = PresetLibrary::load(&textures);
        assert_eq!(library.all()[0].name, "像素硬笔");
        let names: Vec<&str> = library.all().iter().map(|p| p.name.as_ref()).collect();
        assert!(!names.contains(&"ink"), "ink 已经给毛笔用了：{names:?}");
        assert!(!names.contains(&"grass"), "grass 已经给草用了：{names:?}");
    }

    #[test]
    fn a_png_dropped_into_the_directory_becomes_a_preset() {
        // 把一张 PNG 丢进素材目录（这里用临时目录模拟）就能直接在预设里选到它。
        let dir = std::env::temp_dir().join(format!(
            "image_editor_brushes_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("建临时素材目录");
        let mut pixels = crate::document::PixelBuffer::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                let alpha = if (x + y) % 2 == 0 { 255 } else { 40 };
                pixels.set_pixel(x, y, crate::document::Color::new(255, 255, 255, alpha));
            }
        }
        let png = crate::io::encode_png(&pixels).expect("编码");
        std::fs::write(dir.join("my-tip.png"), png).expect("写 PNG");

        let textures = BrushTextures::new(Some(dir.clone()));
        let library = PresetLibrary::load(&textures);
        let preset = library
            .all()
            .iter()
            .find(|preset| preset.name == "my-tip")
            .expect("目录里的 PNG 应该自动成一条预设");
        assert_eq!(preset.grain.texture.as_deref(), Some("my-tip"));
        assert!(!preset.hard, "自动预设要关掉像素模式，纹理才连得上");
        assert!(preset.spacing_ratio <= 0.1, "间距要密，不然一笔像撒点");

        // 应用它能真的挂上颗粒。
        let mut brush = BrushTool::paint();
        preset.clone().apply(&mut brush, &textures);
        let grain = brush.grain.expect("应该挂上颗粒").clone();
        // 用户素材多半大片透明：留底色 + 最小深度，笔迹才不会断成虚线。
        assert_eq!(grain.depth, 0.8, "别把素材当纯形状（会断）");
        assert!(grain.min_depth >= 0.3, "要有底色下限：{}", grain.min_depth);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
