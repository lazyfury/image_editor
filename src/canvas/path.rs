//! 贝塞尔路径：锚点 + 两侧控制柄，展平成折线。
//!
//! 钢笔（[`crate::tools::pen`]）用它攒路径；视图把路径展平后画预览，选区把它
//! 光栅化（[`Selection::polygon`](crate::canvas::Selection::polygon)），描边把它
//! 喂给画笔。这里只有几何，不认识 UI、文档和像素。
//!
//! 展平是**自适应细分**：控制点离弦太远就对半分，直到「控制点到弦的距离」都在
//! 容差以内。所以调用方可以用容差控制精度 —— 预览按 `0.25 / zoom` 展平（放大后
//! 曲线更细），光栅化按 `0.25` 文档像素展平。

use igui_core::Vec2;

/// 细分深度上限：只是防浮点死循环，正常的曲线远到不了。
const MAX_DEPTH: u32 = 12;

/// 一个路径锚点。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    /// 锚点位置（文档坐标）。
    pub point: Vec2,
    /// 进来方向的控制柄（相对 [`Anchor::point`]）。
    pub handle_in: Vec2,
    /// 出去方向的控制柄（相对 [`Anchor::point`]）。
    pub handle_out: Vec2,
}

impl Anchor {
    /// 尖角：两侧都是零柄，相邻段退化成直线。
    pub fn corner(point: Vec2) -> Self {
        Self {
            point,
            handle_in: Vec2::ZERO,
            handle_out: Vec2::ZERO,
        }
    }

    /// 平滑锚点：从 `point` 朝 `out` 拖出控制柄，进来那侧对称。
    pub fn smooth(point: Vec2, out: Vec2) -> Self {
        Self {
            point,
            handle_in: Vec2::ZERO - out,
            handle_out: out,
        }
    }
}

/// 一条路径：锚点序列 + 是否闭合。
///
/// `closed` 只影响展平与段数；[`Path::push`] 会重新打开它（追加锚点即继续画）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Path {
    anchors: Vec<Anchor>,
    closed: bool,
}

impl Path {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn anchors(&self) -> &[Anchor] {
        &self.anchors
    }

    pub fn len(&self) -> usize {
        self.anchors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.anchors.is_empty()
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 追加一个锚点（已闭合的路径被重新打开：追加就等于继续画）。
    pub fn push(&mut self, anchor: Anchor) {
        self.anchors.push(anchor);
        self.closed = false;
    }

    /// 闭合路径。至少要两个锚点（一个锚点围不出任何东西）。
    pub fn close(&mut self) -> bool {
        if self.anchors.len() < 2 {
            return false;
        }
        self.closed = true;
        true
    }

    /// 删掉最后一个锚点（`Backspace`）；没了就返回 `false`。
    pub fn undo_anchor(&mut self) -> bool {
        let removed = self.anchors.pop().is_some();
        if removed {
            self.closed = false;
        }
        removed
    }

    pub fn clear(&mut self) {
        self.anchors.clear();
        self.closed = false;
    }

    /// 段数：闭合路径多一段（最后锚点绕回第一个），开放路径是 `n - 1`。
    pub fn segment_count(&self) -> usize {
        match self.anchors.len() {
            0 | 1 => 0,
            len if self.closed => len,
            len => len - 1,
        }
    }

    /// 按路径自己的开闭状态展平。
    ///
    /// 返回的折线**包含**首锚点，不重复末尾；闭合路径的折线也是「首尾由多边形
    /// 自己接上」（[`Selection::polygon`](crate::canvas::Selection::polygon) 就是这么用的）。
    pub fn flatten(&self, tolerance: f32) -> Vec<Vec2> {
        self.flatten_segments(tolerance, self.closed)
    }

    /// 不管路径开没闭合，都绕回第一个锚点展平 —— 选区 / 填充需要闭合多边形。
    pub fn flatten_closed(&self, tolerance: f32) -> Vec<Vec2> {
        self.flatten_segments(tolerance, true)
    }

    /// 第 `index` 个锚点的可变引用（钢笔拖控制柄用）。
    pub fn anchor_mut(&mut self, index: usize) -> Option<&mut Anchor> {
        self.anchors.get_mut(index)
    }

    /// 命中最近的锚点（`position` 与 `radius` 用同一套坐标，调用方自己换算）。
    pub fn hit_anchor(&self, position: Vec2, radius: f32) -> Option<usize> {
        self.anchors
            .iter()
            .enumerate()
            .map(|(index, anchor)| (index, (anchor.point - position).length()))
            .filter(|(_, distance)| *distance <= radius)
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(index, _)| index)
    }

    fn flatten_segments(&self, tolerance: f32, wrap: bool) -> Vec<Vec2> {
        let mut points = Vec::new();
        let Some(first) = self.anchors.first() else {
            return points;
        };
        points.push(first.point);
        for index in 0..self.segment_count_for(wrap) {
            let start = &self.anchors[index];
            let end = &self.anchors[(index + 1) % self.anchors.len()];
            let segment = flatten_segment(start, end, tolerance);
            points.extend(segment.into_iter().skip(1));
        }
        points
    }

    /// `wrap = true` 时把开放路径也当成闭合的（多一段绕回）。
    fn segment_count_for(&self, wrap: bool) -> usize {
        match self.anchors.len() {
            0 | 1 => 0,
            len if wrap => len,
            len => len - 1,
        }
    }
}

/// 展平「`start` 到 `end`」这一段三次贝塞尔，返回**含两个端点**的采样点。
///
/// 钢笔预览的橡皮筋段（终点还没有控制柄）也走这里。
pub fn flatten_segment(start: &Anchor, end: &Anchor, tolerance: f32) -> Vec<Vec2> {
    let tolerance = tolerance.max(1e-3);
    let mut points = vec![start.point];
    flatten_cubic(
        start.point,
        start.point + start.handle_out,
        end.point + end.handle_in,
        end.point,
        tolerance,
        0,
        &mut points,
    );
    points
}

/// 展开一段三次贝塞尔，把除起点外的采样点追加到 `out`。
fn flatten_cubic(
    p0: Vec2,
    p1: Vec2,
    p2: Vec2,
    p3: Vec2,
    tolerance: f32,
    depth: u32,
    out: &mut Vec<Vec2>,
) {
    if depth >= MAX_DEPTH || flat_enough(p0, p1, p2, p3, tolerance) {
        out.push(p3);
        return;
    }
    // de Casteljau 对半分。
    let (p01, p12, p23) = (midpoint(p0, p1), midpoint(p1, p2), midpoint(p2, p3));
    let (p012, p123) = (midpoint(p01, p12), midpoint(p12, p23));
    let center = midpoint(p012, p123);
    flatten_cubic(p0, p01, p012, center, tolerance, depth + 1, out);
    flatten_cubic(center, p123, p23, p3, tolerance, depth + 1, out);
}

/// 控制点离弦足够近（弦长退化时退化成「离起点足够近」）。
fn flat_enough(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, tolerance: f32) -> bool {
    let chord = p3 - p0;
    let length = chord.length();
    if length <= tolerance {
        return (p1 - p0).length().max((p2 - p0).length()) <= tolerance;
    }
    let direction = chord / length;
    let distance = |point: Vec2| {
        let offset = point - p0;
        (offset - direction * offset.dot(direction)).length()
    };
    distance(p1).max(distance(p2)) <= tolerance
}

fn midpoint(a: Vec2, b: Vec2) -> Vec2 {
    (a + b) * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32) -> Vec2 {
        Vec2::new(x, y)
    }

    #[test]
    fn a_corner_segment_flattens_to_just_its_two_endpoints() {
        // 两个尖角锚点之间是直线：不该被子细分出一堆点（描边 / 光栅化都省事）。
        let mut path = Path::new();
        path.push(Anchor::corner(at(2.0, 3.0)));
        path.push(Anchor::corner(at(40.0, 3.0)));
        assert_eq!(path.flatten(0.25), vec![at(2.0, 3.0), at(40.0, 3.0)]);
        assert_eq!(path.segment_count(), 1);
    }

    #[test]
    fn a_smooth_anchor_bends_the_curve_but_never_leaves_the_control_hull() {
        // 两点 + 强控制柄：曲线要离开弦（不是直线），但不能跑出控制多边形的包围盒。
        let mut path = Path::new();
        path.push(Anchor::smooth(at(0.0, 0.0), at(0.0, 20.0)));
        path.push(Anchor::smooth(at(40.0, 0.0), at(0.0, -20.0)));
        let points = path.flatten(0.25);
        assert!(points.len() > 4, "曲线应该被细分成多段：{}", points.len());

        // 包围盒：锚点 (0,0)/(40,0) + 控制点 (0,20)/(40,-20)。
        // B(t) 的两端柄只贡献一半幅度（三次贝塞尔的最大偏移是控制偏移的 3/4）。
        for point in &points {
            assert!((0.0..=40.0).contains(&point.x), "{point:?}");
            assert!((-20.0..=20.0).contains(&point.y), "{point:?}");
        }
        // 中点确实离开了弦（y = 0 那条线）。
        let middle = points[points.len() / 2];
        assert!(middle.y.abs() > 5.0, "中点 {middle:?} 应该被柄拉出去");
    }

    #[test]
    fn a_tighter_tolerance_flattens_finer() {
        let mut path = Path::new();
        path.push(Anchor::smooth(at(0.0, 0.0), at(0.0, 30.0)));
        path.push(Anchor::smooth(at(60.0, 0.0), at(0.0, -30.0)));
        let coarse = path.flatten(4.0).len();
        let fine = path.flatten(0.25).len();
        assert!(fine > coarse, "fine {fine} 应该比 coarse {coarse} 细");
    }

    #[test]
    fn a_closed_path_adds_the_wrapping_segment() {
        let mut path = Path::new();
        for point in [at(0.0, 0.0), at(10.0, 0.0), at(10.0, 10.0)] {
            path.push(Anchor::corner(point));
        }
        assert_eq!(path.segment_count(), 2, "开着的时候两段");
        assert_eq!(path.flatten(0.25).len(), 3);
        assert!(path.close());
        assert_eq!(path.segment_count(), 3, "闭合多一段绕回");
        assert!(path.is_closed());
    }

    #[test]
    fn flattening_closed_wraps_an_open_path_even_with_curved_ends() {
        // 开放路径的两端有柄：flatten 只画两段，flatten_closed 还多一段绕回，
        // 而且那一段是曲线（点多于三个锚点）。
        let mut path = Path::new();
        path.push(Anchor::smooth(at(0.0, 0.0), at(10.0, 0.0)));
        path.push(Anchor::corner(at(50.0, 0.0)));
        path.push(Anchor::smooth(at(50.0, 50.0), at(-10.0, 0.0)));
        let open = path.flatten(0.25).len();
        let closed = path.flatten_closed(0.25).len();
        assert!(
            closed > open,
            "closed {closed} 应比 open {open} 多出绕回那段"
        );
        assert_eq!(path.segment_count(), 2);
    }

    #[test]
    fn a_path_needs_two_anchors_to_close() {
        let mut path = Path::new();
        path.push(Anchor::corner(at(1.0, 1.0)));
        assert!(!path.close(), "一个锚点围不出东西");
        assert!(!path.is_closed());
    }

    #[test]
    fn pushing_after_closing_reopens_the_path() {
        let mut path = Path::new();
        path.push(Anchor::corner(at(0.0, 0.0)));
        path.push(Anchor::corner(at(10.0, 0.0)));
        assert!(path.close());
        path.push(Anchor::corner(at(10.0, 10.0)));
        assert!(!path.is_closed(), "继续画就是开放路径");
        assert_eq!(path.len(), 3);
    }

    #[test]
    fn undo_anchor_drops_the_last_point_and_nothing_when_empty() {
        let mut path = Path::new();
        assert!(!path.undo_anchor());
        path.push(Anchor::corner(at(0.0, 0.0)));
        path.push(Anchor::corner(at(10.0, 0.0)));
        assert!(path.close());
        assert!(path.undo_anchor());
        assert_eq!(path.len(), 1);
        assert!(!path.is_closed(), "删点后不再闭合");
    }

    #[test]
    fn hit_anchor_picks_the_nearest_anchor_inside_the_radius() {
        let mut path = Path::new();
        path.push(Anchor::corner(at(10.0, 10.0)));
        path.push(Anchor::corner(at(50.0, 10.0)));
        // 离第二个锚点 3px：命中下标 1。
        assert_eq!(path.hit_anchor(at(53.0, 11.0), 4.0), Some(1));
        // 半径不够就是没命中。
        assert_eq!(path.hit_anchor(at(53.0, 11.0), 2.0), None);
        // 空路径也不会命中。
        assert_eq!(Path::new().hit_anchor(Vec2::ZERO, 8.0), None);
    }
}
