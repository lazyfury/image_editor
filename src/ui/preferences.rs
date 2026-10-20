//! 笔刷偏好的读写桥：视图只做「导出快照 / 应用快照」，读写配置文件是宿主的活。
//!
//! 这样测试和 `--selfcheck` 永远不会碰用户的配置目录，宿主在 `init` 时读、
//! 每帧问一次 [`EditorView::take_preferences`]（变了才写盘）。

use crate::io::BrushPreferences;
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
        // 应用即视为「已落盘」，免得下一帧又写一遍。
        self.saved_preferences = self.preferences();
    }

    /// 偏好变了就返回新的一份（宿主拿去写盘）；没变返回 `None`。
    pub fn take_preferences(&mut self) -> Option<BrushPreferences> {
        let current = self.preferences();
        if current == self.saved_preferences {
            return None;
        }
        self.saved_preferences = current;
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
