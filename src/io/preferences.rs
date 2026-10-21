//! 编辑器偏好：配置目录里的 `preferences`（`key=value` 纯文本，跨会话保留）。
//!
//! 两类：
//!
//! - **笔刷外观**（[`BrushPreferences`]：直径 / 不透明度 / 硬度 / 平滑 / 像素模式 /
//!   方形笔 / 对称 / 间距）——颜色、选区、路径属于文档或会话，不在这里；
//! - **过程记录开关**（[`EditorPreferences::record_timeline`]）——给延时动画 /
//!   回放用，默认关（要存每一步的像素补丁，占内存）。
//!
//! 读的时候坏行直接跳过、越界值夹到合法范围，所以手改坏了最多是回到默认值，
//! 不会画出 0.001px 的笔。

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
            spacing: self.spacing.clamp(0.02, 1.0),
            ..self.clone()
        }
    }

    fn to_text(&self) -> String {
        format!(
            "size={}\nopacity={}\nhardness={}\nsmooth={}\nsolid={}\nsquare={}\nsymmetry={}\n\
spacing={}\n",
            self.size,
            self.opacity,
            self.hardness,
            self.smooth,
            self.solid as u8,
            self.square as u8,
            self.symmetry,
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
                "spacing" => prefs.spacing = number.unwrap_or(prefs.spacing),
                _ => {}
            }
        }
        prefs.clamped()
    }
}

/// 编辑器偏好（`preferences` 文件的完整内容）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EditorPreferences {
    /// 笔刷外观（视图每帧同步进画笔）。
    pub brush: BrushPreferences,
    /// 记录整段编辑过程（延时动画 / 过程回放用）；默认关。
    pub record_timeline: bool,
}

impl EditorPreferences {
    fn to_text(&self) -> String {
        let mut text = self.brush.clamped().to_text();
        text.push_str(&format!(
            "record_timeline={}\n",
            u8::from(self.record_timeline)
        ));
        text
    }

    fn parse(text: &str) -> Self {
        // 笔刷那几行交给 `BrushPreferences::parse`（不认识的键它会跳过）。
        let record_timeline = text.lines().any(|line| match line.split_once('=') {
            Some((key, value)) => key.trim() == "record_timeline" && value.trim() != "0",
            None => false,
        });
        Self {
            brush: BrushPreferences::parse(text),
            record_timeline,
        }
    }
}

/// 偏好文件路径。
pub fn preferences_path() -> PathBuf {
    config_dir().join("preferences")
}

/// 读偏好（没有文件 / 读不动都返回默认值）。
pub fn load() -> EditorPreferences {
    load_at(&preferences_path())
}

/// 写偏好（失败只报错，不 panic）。
pub fn save(prefs: &EditorPreferences) -> Result<(), IoError> {
    save_at(&preferences_path(), prefs)
}

/// [`load`] 的显式路径版本（测试注入用）。
pub fn load_at(path: &Path) -> EditorPreferences {
    match std::fs::read_to_string(path) {
        Ok(text) => EditorPreferences::parse(&text),
        Err(_) => EditorPreferences::default(),
    }
}

/// [`save`] 的显式路径版本（测试注入用）。
pub fn save_at(path: &Path, prefs: &EditorPreferences) -> Result<(), IoError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, prefs.to_text())?;
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
        let prefs = EditorPreferences {
            brush: BrushPreferences {
                size: 24.0,
                opacity: 0.35,
                hardness: 0.6,
                smooth: 0.0,
                solid: false,
                square: false,
                symmetry: 3,
                spacing: 0.12,
            },
            record_timeline: true,
        };
        save_at(&path, &prefs).expect("写偏好");
        assert_eq!(load_at(&path), prefs);
        let _ = std::fs::remove_file(&path);
    }

    /// 「记录整段编辑过程」默认关（占内存），打开后能往返。
    #[test]
    fn the_timeline_recording_flag_defaults_off_and_round_trips() {
        assert!(!EditorPreferences::default().record_timeline);
        let path = temp_path("timeline");
        save_at(
            &path,
            &EditorPreferences {
                record_timeline: true,
                ..EditorPreferences::default()
            },
        )
        .expect("写偏好");
        assert!(load_at(&path).record_timeline);
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
        let prefs = load_at(&path).brush;
        assert_eq!(prefs.size, 1.0, "解析不出来的值保留默认");
        assert_eq!(prefs.opacity, 0.5, "能解析的照常生效");
        assert_eq!(prefs.symmetry, 3, "越界的枚举夹到最大值");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_loads_the_defaults() {
        assert_eq!(load_at(&temp_path("missing")), EditorPreferences::default());
    }
}
