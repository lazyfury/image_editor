//! 笔刷偏好的读写桥：视图只做「导出快照 / 应用快照」，读写配置文件是宿主的活。
//!
//! 这样测试和 `--selfcheck` 永远不会碰用户的配置目录，宿主在 `init` 时读、
//! 每帧问一次 [`EditorView::take_preferences`]（变了才写盘）。

use crate::io::BrushPreferences;
use crate::tools::brush::{Grain, GrainMapping};
use crate::tools::Symmetry;

use super::EditorView;

impl EditorView {
    /// 当前的笔刷外观快照（写进配置用）。
    pub fn preferences(&self) -> BrushPreferences {
        BrushPreferences {
            size: self.brush.size,
            opacity: self.brush.opacity,
            hardness: self.brush.hardness,
            smooth: self.brush.smooth,
            solid: self.pixel_mode.get(),
            square: self.square_mode.get(),
            symmetry: symmetry_code(self.symmetry_mode.get()),
            // 颗粒按名字存（`Rc` 反查缓存），下次启动按名字加载回来。
            texture: self
                .brush
                .grain
                .as_ref()
                .and_then(|grain| self.brush_textures.name_of(grain.texture()))
                .unwrap_or_default(),
            angle: self
                .brush
                .grain
                .as_ref()
                .map_or(0.0, |grain| grain.angle.to_degrees()),
            angle_mode: self
                .brush
                .grain
                .as_ref()
                .map_or(0, |grain| grain_mapping_code(grain.mapping)),
            grain_scale: self.brush.grain.as_ref().map_or(1.0, |grain| grain.scale),
            grain_depth: self.brush.grain.as_ref().map_or(1.0, |grain| grain.depth),
            grain_min_depth: self
                .brush
                .grain
                .as_ref()
                .map_or(0.0, |grain| grain.min_depth),
            grain_contrast: self
                .brush
                .grain
                .as_ref()
                .map_or(1.0, |grain| grain.contrast),
            grain_brightness: self
                .brush
                .grain
                .as_ref()
                .map_or(0.0, |grain| grain.brightness),
            count: self.brush.count,
            scatter: self.brush.scatter,
            scatter_rotation: self.brush.scatter_rotation,
            spacing: self.brush.spacing_ratio,
        }
    }

    /// 应用一份偏好（宿主启动时读配置后调；同步进选项栏的共享开关）。
    pub fn apply_preferences(&mut self, prefs: &BrushPreferences) {
        let prefs = prefs.clamped();
        self.brush.size = prefs.size;
        self.brush.opacity = prefs.opacity;
        self.brush.hardness = prefs.hardness;
        self.brush.smooth = prefs.smooth;
        self.pixel_mode.set(prefs.solid);
        self.square_mode.set(prefs.square);
        self.symmetry_mode.set(symmetry_from_code(prefs.symmetry));
        self.brush.grain = self
            .brush_textures
            .get(&prefs.texture)
            .map(|texture| Grain {
                texture,
                mapping: grain_mapping_from_code(prefs.angle_mode),
                angle: prefs.angle.to_radians(),
                scale: prefs.grain_scale,
                depth: prefs.grain_depth,
                min_depth: prefs.grain_min_depth,
                contrast: prefs.grain_contrast,
                brightness: prefs.grain_brightness,
            });
        self.brush.count = prefs.count;
        self.brush.scatter = prefs.scatter;
        self.brush.scatter_rotation = prefs.scatter_rotation;
        self.brush.spacing_ratio = prefs.spacing;
        // 应用即视为「已落盘」，免得下一帧又写一遍。
        self.saved_preferences = self.preferences();
    }

    /// 偏好变了就返回新的一份（宿主拿去写盘）；没变返回 `None`。
    pub fn take_preferences(&mut self) -> Option<BrushPreferences> {
        let current = self.preferences();
        if current == self.saved_preferences {
            return None;
        }
        self.saved_preferences = current.clone();
        Some(current)
    }
}

/// 对称模式 <-> 配置里的数字（`.igui` 之外的格式只追加不改号）。
pub const fn symmetry_code(mode: Symmetry) -> u8 {
    match mode {
        Symmetry::Off => 0,
        Symmetry::Vertical => 1,
        Symmetry::Horizontal => 2,
        Symmetry::Both => 3,
    }
}

/// 颗粒映射 <-> 配置里的数字（0 每站重贴 / 1 跟笔 / 2 钉画布）。
pub const fn grain_mapping_code(mapping: GrainMapping) -> u8 {
    match mapping {
        GrainMapping::Stamp => 0,
        GrainMapping::Dragged => 1,
        GrainMapping::CanvasLocked => 2,
    }
}

pub const fn grain_mapping_from_code(code: u8) -> GrainMapping {
    match code {
        1 => GrainMapping::Dragged,
        2 => GrainMapping::CanvasLocked,
        _ => GrainMapping::Stamp,
    }
}

pub const fn symmetry_from_code(code: u8) -> Symmetry {
    match code {
        1 => Symmetry::Vertical,
        2 => Symmetry::Horizontal,
        3 => Symmetry::Both,
        _ => Symmetry::Off,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AppState;

    fn view() -> EditorView {
        EditorView::new(crate::theme::editor_theme(false), AppState::default())
    }

    #[test]
    fn a_fresh_view_has_nothing_to_save() {
        // 默认参数必须和 `BrushPreferences::default()` 一致，否则每次启动都会
        // 立刻写一次配置文件。
        let mut view = view();
        assert_eq!(view.preferences(), BrushPreferences::default());
        assert_eq!(view.take_preferences(), None);
    }

    #[test]
    fn applying_preferences_sets_the_brush_and_the_option_toggles() {
        let mut view = view();
        let prefs = BrushPreferences {
            size: 24.0,
            opacity: 0.35,
            hardness: 0.6,
            smooth: 0.0,
            solid: false,
            square: false,
            symmetry: 3,
            texture: "bristle".to_string(),
            angle: 0.0,
            angle_mode: grain_mapping_code(GrainMapping::Dragged),
            grain_scale: 1.0,
            grain_depth: 0.8,
            grain_min_depth: 0.1,
            grain_contrast: 1.0,
            grain_brightness: 0.0,
            count: 1,
            scatter: 0.0,
            scatter_rotation: 0.0,
            spacing: 0.1,
        };
        view.apply_preferences(&prefs);
        assert_eq!(view.preferences(), prefs);
        // 选项栏开关也同步了（它们是绘制源的显示源）。
        assert!(!view.pixel_mode() && !view.square_mode());
        assert_eq!(view.symmetry_mode(), Symmetry::Both);
        assert_eq!(view.take_preferences(), None, "刚应用过，不该再写盘");
    }

    #[test]
    fn changing_the_brush_reports_a_new_snapshot_once() {
        let mut view = view();
        assert_eq!(view.take_preferences(), None);
        view.brush.size = 12.0;
        assert_eq!(view.take_preferences().map(|prefs| prefs.size), Some(12.0));
        assert_eq!(view.take_preferences(), None, "同样的参数只报一次");
    }

    #[test]
    fn a_textured_brush_survives_the_round_trip() {
        // 选「毛笔」-> 存快照 -> 新建视图应用 -> 笔尖纹理、角度模式、间距都回来。
        let mut view = view();
        view.brush_preset_request.set(Some(6));
        view.update();
        assert!(view.brush.grain.is_some(), "毛笔应带颗粒");
        let saved = view.preferences();
        assert_eq!(saved.texture, "ink");
        assert_eq!(
            saved.angle_mode,
            grain_mapping_code(GrainMapping::Dragged),
            "毛笔 = 跟笔滚动"
        );

        let mut restored = EditorView::new(
            crate::theme::editor_theme(false),
            crate::app::state::AppState::default(),
        );
        restored.apply_preferences(&saved);
        assert_eq!(restored.preferences(), saved);
        assert!(restored.brush.grain.is_some(), "颗粒按名字加载回来");
    }

    #[test]
    fn symmetry_codes_round_trip() {
        for mode in [
            Symmetry::Off,
            Symmetry::Vertical,
            Symmetry::Horizontal,
            Symmetry::Both,
        ] {
            assert_eq!(symmetry_from_code(symmetry_code(mode)), mode);
        }
        assert_eq!(symmetry_from_code(42), Symmetry::Off, "未知值退回关");
    }
}
