//! 多节点渐变（游戏引擎式 color ramp）。
//!
//! 一条渐变是一串**色标**（`position` 0..=1 + `color`），按 `position` 排序，
//! 至少两个。采样时定位 `t` 落在哪个区间，对两端颜色做 RGBA 插值；两端 clamp。
//! 渐变工具用它把前景→背景的默认 ramp 换成任意多节点，编辑器可增删 / 拖动节点。

use super::color::Color;

/// 一个色标：位置 0..=1 + 颜色。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    pub position: f32,
    pub color: Color,
}

impl GradientStop {
    pub const fn new(position: f32, color: Color) -> Self {
        Self { position, color }
    }
}

/// 多色标渐变（`stops` 按 `position` 升序，长度 >= 2）。
#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    stops: Vec<GradientStop>,
}

impl Gradient {
    /// 两端色标（前景@0、背景@1）。默认渐变用它。
    pub fn from_colors(start: Color, end: Color) -> Self {
        Self {
            stops: vec![GradientStop::new(0.0, start), GradientStop::new(1.0, end)],
        }
    }

    pub fn stops(&self) -> &[GradientStop] {
        &self.stops
    }

    /// 采样：`t` 所在区间两端插值；两端 clamp。`stops` 保证 >= 2。
    pub fn evaluate(&self, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let first = self.stops[0];
        if t <= first.position {
            return first.color;
        }
        for pair in self.stops.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if t <= b.position {
                let span = b.position - a.position;
                let local = if span <= f32::EPSILON {
                    0.0
                } else {
                    (t - a.position) / span
                };
                return a.color.lerp(b.color, local);
            }
        }
        self.stops[self.stops.len() - 1].color
    }

    /// 把首 / 尾色标设成前景 / 背景（默认渐变跟随前景背景时用）。
    pub fn set_endpoints(&mut self, start: Color, end: Color) {
        let last = self.stops.len() - 1;
        self.stops[0].color = start;
        self.stops[last].color = end;
    }

    /// 插入一个色标，返回它的下标（保持排序）。
    pub fn add_stop(&mut self, position: f32, color: Color) -> usize {
        let position = position.clamp(0.0, 1.0);
        let index = self
            .stops
            .iter()
            .position(|stop| stop.position > position)
            .unwrap_or(self.stops.len());
        self.stops.insert(index, GradientStop::new(position, color));
        index
    }

    /// 删除一个色标；只剩两个时返回 `false`（不删）。
    pub fn remove_stop(&mut self, index: usize) -> bool {
        if self.stops.len() <= 2 || index >= self.stops.len() {
            return false;
        }
        self.stops.remove(index);
        true
    }

    /// 移动一个色标到 `position`，返回它重排后的新下标（保持排序）。
    pub fn move_stop(&mut self, index: usize, position: f32) -> usize {
        if index >= self.stops.len() {
            return index;
        }
        let mut stop = self.stops.remove(index);
        stop.position = position.clamp(0.0, 1.0);
        let new_index = self
            .stops
            .iter()
            .position(|candidate| candidate.position > stop.position)
            .unwrap_or(self.stops.len());
        self.stops.insert(new_index, stop);
        new_index
    }

    /// 改某个色标的颜色。
    pub fn set_color(&mut self, index: usize, color: Color) {
        if let Some(stop) = self.stops.get_mut(index) {
            stop.color = color;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two() -> Gradient {
        Gradient::from_colors(Color::BLACK, Color::WHITE)
    }

    #[test]
    fn a_two_stop_gradient_interpolates_between_the_endpoints() {
        let gradient = two();
        assert_eq!(gradient.evaluate(0.0), Color::BLACK);
        assert_eq!(gradient.evaluate(1.0), Color::WHITE);
        assert_eq!(gradient.evaluate(0.5), Color::new(128, 128, 128, 255));
        // 两端 clamp。
        assert_eq!(gradient.evaluate(-1.0), Color::BLACK);
        assert_eq!(gradient.evaluate(2.0), Color::WHITE);
    }

    #[test]
    fn a_middle_stop_creates_two_segments() {
        let mut gradient = two();
        gradient.add_stop(0.5, Color::RED);
        assert_eq!(gradient.evaluate(0.5), Color::RED);
        // 0..0.5 在黑与红之间。
        assert_eq!(gradient.evaluate(0.25), Color::new(128, 0, 0, 255));
        // 0.5..1 在红与白之间。
        assert_eq!(gradient.evaluate(0.75), Color::new(255, 128, 128, 255));
    }

    #[test]
    fn stops_stay_sorted_when_added_or_moved() {
        let mut gradient = two();
        gradient.add_stop(0.8, Color::RED);
        gradient.add_stop(0.3, Color::RED);
        let positions: Vec<f32> = gradient.stops().iter().map(|s| s.position).collect();
        assert_eq!(positions, vec![0.0, 0.3, 0.8, 1.0]);

        // 把 0.3 移到 0.9，应排在 0.8 后面。
        let index = gradient.move_stop(1, 0.9);
        assert_eq!(index, 2);
        let positions: Vec<f32> = gradient.stops().iter().map(|s| s.position).collect();
        assert_eq!(positions, vec![0.0, 0.8, 0.9, 1.0]);
    }

    #[test]
    fn a_gradient_keeps_at_least_two_stops() {
        let mut gradient = two();
        assert!(!gradient.remove_stop(0), "只剩两个时不能删");
        assert_eq!(gradient.stops().len(), 2);

        gradient.add_stop(0.5, Color::RED);
        assert!(gradient.remove_stop(1));
        assert_eq!(gradient.stops().len(), 2);
    }

    #[test]
    fn setting_endpoints_only_touches_the_ends() {
        let mut gradient = two();
        gradient.add_stop(0.5, Color::RED);
        gradient.set_endpoints(Color::new(1, 2, 3, 4), Color::new(5, 6, 7, 8));
        assert_eq!(gradient.stops()[0].color, Color::new(1, 2, 3, 4));
        assert_eq!(gradient.stops()[1].color, Color::RED, "中间节点不动");
        assert_eq!(gradient.stops()[2].color, Color::new(5, 6, 7, 8));
    }

    #[test]
    fn duplicate_positions_do_not_panic() {
        let mut gradient = two();
        gradient.add_stop(0.5, Color::RED);
        gradient.add_stop(0.5, Color::new(0, 255, 0, 255));
        // 采样仍能返回一个颜色。
        let _ = gradient.evaluate(0.5);
        assert_eq!(gradient.stops().len(), 4);
    }
}
