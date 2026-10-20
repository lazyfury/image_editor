//! 编辑器偏好：配置目录里的 `preferences`（`key=value` 纯文本，跨会话保留）。
//!
//! 只存**笔刷外观**（直径 / 不透明度 / 硬度 / 平滑 / 像素模式 / 方形笔 / 对称 /
//! 颗粒 / 个数 / 散布 / 间距）——
//! 颜色、选区、路径属于文档或会话，不在这里。读的时候坏行直接跳过、越界值夹到
//! 合法范围，所以手改坏了最多是回到默认值，不会画出 0.001px 的笔。

use std::path::{Path, PathBuf};

use super::recent::config_dir;
use super::IoError;

/// 笔刷外观参数（跨会话保留）。
#[derive(Debug, Clone, PartialEq)]
pub struct BrushPreferences {
    /// 直径，文档像素。
    pub size: f32,
    /// 每一笔的不透明度。
    pub opacity: f32,
    /// 笔尖硬度（1 = 只有 1px 抗锯齿边）。
    pub hardness: f32,
    /// 平滑强度（0 = 折线、1 = 二次中点平滑）。
    pub smooth: f32,
    /// 像素模式（硬边）。
    pub solid: bool,
    /// 方形笔（否则圆头）。
    pub square: bool,
    /// 对称：0 关 / 1 垂直 / 2 水平 / 3 四向。
    pub symmetry: u8,
    /// 颗粒素材名（空 = 程序化笔尖）。
    pub texture: String,
    /// `Stamp` 映射的固定角度（度）。
    pub angle: f32,
    /// 颗粒映射：0 = 每站重贴 / 1 = 跟笔滚动 / 2 = 钉在画布。
    pub angle_mode: u8,
    /// 一块纹理覆盖几个直径。
    pub grain_scale: f32,
    /// 0..=1：颗粒相对底色的深度。
    pub grain_depth: f32,
    /// 0..=1：颗粒最暗处的下限。
    pub grain_min_depth: f32,
    /// 颗粒对比度（1 = 原样）。
    pub grain_contrast: f32,
    /// 颗粒亮度偏移（-1..=1）。
    pub grain_brightness: f32,
    /// 每个采样点的图章数（1..=16）。
    pub count: u32,
    /// 图章随机偏移幅度（相对直径）。
    pub scatter: f32,
    /// 图章随机旋转幅度（0..=1）。
    pub scatter_rotation: f32,
    /// 落笔间距相对直径的比例。
    pub spacing: f32,
}

impl Default for BrushPreferences {
    /// 和 [`crate::tools::BrushTool::paint`] 的默认值一致（像素硬笔）。
    fn default() -> Self {
        Self {
            size: 1.0,
            opacity: 1.0,
            hardness: 1.0,
            smooth: 1.0,
            solid: true,
            square: true,
            symmetry: 0,
            texture: String::new(),
            angle: 0.0,
            angle_mode: 0,
            grain_scale: 1.0,
            grain_depth: 1.0,
            grain_min_depth: 0.0,
            grain_contrast: 1.0,
            grain_brightness: 0.0,
            count: 1,
            scatter: 0.0,
            scatter_rotation: 0.0,
            spacing: 0.25,
        }
    }
}

impl BrushPreferences {
    /// 夹到合法范围（和选项栏 / 画笔的边界一致）。
    pub fn clamped(&self) -> Self {
        Self {
            size: self.size.clamp(1.0, 200.0),
            opacity: self.opacity.clamp(0.05, 1.0),
            hardness: self.hardness.clamp(0.05, 1.0),
            smooth: self.smooth.clamp(0.0, 1.0),
            symmetry: self.symmetry.min(3),
            angle_mode: self.angle_mode.min(2),
            grain_scale: self.grain_scale.clamp(0.05, 8.0),
            grain_contrast: self.grain_contrast.clamp(0.0, 8.0),
            grain_brightness: self.grain_brightness.clamp(-1.0, 1.0),
            grain_depth: self.grain_depth.clamp(0.0, 1.0),
            grain_min_depth: self
                .grain_min_depth
                .clamp(0.0, self.grain_depth.clamp(0.0, 1.0)),
            count: self.count.clamp(1, crate::tools::brush::MAX_STAMPS),
            scatter: self.scatter.clamp(0.0, 4.0),
            scatter_rotation: self.scatter_rotation.clamp(0.0, 1.0),
            spacing: self.spacing.clamp(0.02, 1.0),
            ..self.clone()
        }
    }

    fn to_text(&self) -> String {
        format!(
            "size={}\nopacity={}\nhardness={}\nsmooth={}\nsolid={}\nsquare={}\nsymmetry={}\n\
texture={}\nangle={}\nangle_mode={}\ngrain_scale={}\ngrain_depth={}\n\
grain_min_depth={}\ngrain_contrast={}\ngrain_brightness={}\ncount={}\nscatter={}\nscatter_rotation={}\nspacing={}\n",
            self.size,
            self.opacity,
            self.hardness,
            self.smooth,
            self.solid as u8,
            self.square as u8,
            self.symmetry,
            self.texture,
            self.angle,
            self.angle_mode,
            self.grain_scale,
            self.grain_depth,
            self.grain_min_depth,
            self.grain_contrast,
            self.grain_brightness,
            self.count,
            self.scatter,
            self.scatter_rotation,
            self.spacing
        )
    }

    /// 解析 `key=value` 行；不认识的键 / 解析失败的值都跳过（保留默认值）。
    fn parse(text: &str) -> Self {
        let mut prefs = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            let number = value.parse::<f32>().ok();
            match key {
                "size" => prefs.size = number.unwrap_or(prefs.size),
                "opacity" => prefs.opacity = number.unwrap_or(prefs.opacity),
                "hardness" => prefs.hardness = number.unwrap_or(prefs.hardness),
                "smooth" => prefs.smooth = number.unwrap_or(prefs.smooth),
                "solid" => prefs.solid = value != "0",
                "square" => prefs.square = value != "0",
                "symmetry" => prefs.symmetry = value.parse::<u8>().unwrap_or(prefs.symmetry),
                "texture" => prefs.texture = value.to_string(),
                "angle" => prefs.angle = number.unwrap_or(prefs.angle),
                "angle_mode" => prefs.angle_mode = value.parse::<u8>().unwrap_or(prefs.angle_mode),
                "grain_scale" => prefs.grain_scale = number.unwrap_or(prefs.grain_scale),
                "grain_depth" => prefs.grain_depth = number.unwrap_or(prefs.grain_depth),
                "grain_min_depth" => {
                    prefs.grain_min_depth = number.unwrap_or(prefs.grain_min_depth);
                }
                "grain_contrast" => prefs.grain_contrast = number.unwrap_or(prefs.grain_contrast),
                "grain_brightness" => {
                    prefs.grain_brightness = number.unwrap_or(prefs.grain_brightness);
                }
                "count" => prefs.count = value.parse::<u32>().unwrap_or(prefs.count),
                "scatter" => prefs.scatter = number.unwrap_or(prefs.scatter),
                "scatter_rotation" => {
                    prefs.scatter_rotation = number.unwrap_or(prefs.scatter_rotation);
                }
                "spacing" => prefs.spacing = number.unwrap_or(prefs.spacing),
                _ => {}
            }
        }
        prefs.clamped()
    }
}

/// 偏好文件路径。
pub fn preferences_path() -> PathBuf {
    config_dir().join("preferences")
}

/// 读偏好（没有文件 / 读不动都返回默认值）。
pub fn load() -> BrushPreferences {
    load_at(&preferences_path())
}

/// 写偏好（失败只报错，不 panic）。
pub fn save(prefs: &BrushPreferences) -> Result<(), IoError> {
    save_at(&preferences_path(), prefs)
}

/// [`load`] 的显式路径版本（测试注入用）。
pub fn load_at(path: &Path) -> BrushPreferences {
    match std::fs::read_to_string(path) {
        Ok(text) => BrushPreferences::parse(&text),
        Err(_) => BrushPreferences::default(),
    }
}

/// [`save`] 的显式路径版本（测试注入用）。
pub fn save_at(path: &Path, prefs: &BrushPreferences) -> Result<(), IoError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, prefs.clamped().to_text())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "image_editor_prefs_{}_{n}_{tag}",
            std::process::id()
        ))
    }

    #[test]
    fn the_defaults_match_the_default_pixel_brush() {
        let prefs = BrushPreferences::default();
        assert_eq!(
            prefs,
            BrushPreferences::default().clamped(),
            "默认值本身合法"
        );
        assert_eq!((prefs.size, prefs.opacity, prefs.hardness), (1.0, 1.0, 1.0));
        assert!(prefs.solid && prefs.square, "默认是像素硬笔");
        assert_eq!(prefs.symmetry, 0);
    }

    #[test]
    fn preferences_round_trip_through_the_file() {
        let path = temp_path("roundtrip");
        let prefs = BrushPreferences {
            size: 24.0,
            opacity: 0.35,
            hardness: 0.6,
            smooth: 0.0,
            solid: false,
            square: false,
            symmetry: 3,
            texture: "ink".to_string(),
            angle: 15.0,
            angle_mode: 1,
            grain_scale: 0.5,
            grain_depth: 0.8,
            grain_min_depth: 0.1,
            grain_contrast: 2.5,
            grain_brightness: -0.1,
            count: 3,
            scatter: 0.2,
            scatter_rotation: 1.0,
            spacing: 0.12,
        };
        save_at(&path, &prefs).expect("写偏好");
        assert_eq!(load_at(&path), prefs);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn garbage_lines_and_values_fall_back_to_the_defaults() {
        let path = temp_path("garbage");
        std::fs::write(
            &path,
            "size=not-a-number\n\n# 注释\nwat=1\nopacity=0.5\nsymmetry=99\n",
        )
        .expect("写坏文件");
        let prefs = load_at(&path);
        assert_eq!(prefs.size, 1.0, "解析不出来的值保留默认");
        assert_eq!(prefs.opacity, 0.5, "能解析的照常生效");
        assert_eq!(prefs.symmetry, 3, "越界的枚举夹到最大值");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_loads_the_defaults() {
        assert_eq!(load_at(&temp_path("missing")), BrushPreferences::default());
    }
}
