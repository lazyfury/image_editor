//! 笔触采样：把稀疏、不等距的光标输入变成**等距**的落笔点。
//!
//! 光标事件的密度由系统决定（快速拖动时会被合并成很少几个点），所以
//! 「每个事件压一个 stamp」在快笔画里会变成一段段直尺线，慢笔画又会在
//! 事件点上叠得更密。这里做两件事：
//!
//! 1. **二次中点平滑**：三个连续光标点 `a / b / c` 画一段以 `b` 为控制点、
//!    端点为 `(a+b)/2`、`(b+c)/2` 的二次贝塞尔。曲线落在三点的凸包内、
//!    不过冲；直线输入仍是直线，折角被抹圆。
//! 2. **等距取点**：沿着这段曲线按固定间距取点，所以手快（点稀）手慢
//!    （点密）落笔密度一样，不透明度不会因为事件密集而在某些位置叠深。
//!
//! 曲线需要「下一个点」才能确定，所以画的过程中笔迹末端最多落后光标半个
//! 事件段；抬笔时调用方用终点再取一次点，把最后半段补完。

use igui_core::Vec2;

/// 采样间距的下限：再小就是同一个像素被压很多次，只是更慢。
pub const MIN_SPACING: f32 = 0.5;

/// 两点中点：相邻两段平滑曲线在这里接上（上一段的 `t = 1`）。
pub fn midpoint(a: Vec2, b: Vec2) -> Vec2 {
    (a + b) * 0.5
}

/// 沿二次贝塞尔 `from -> to`（控制点 `ctrl`）按固定间距取点，追加到 `out`。
///
/// **不含** `from`（上一段已经压过它）、含 `to`；`from == to` 时什么都不加。
///
/// 取点是**按弧长**的：先把曲线细分成短折线（步长 ≤ `spacing / 2`），再沿着
/// 折线累积长度、每走满 `spacing` 落一个点。参数 `t` 与弧长并不成正比
/// （退化曲线 `from == ctrl` 上最明显），所以不能直接均匀分 `t`。
pub fn curve_dabs(from: Vec2, ctrl: Vec2, to: Vec2, spacing: f32, out: &mut Vec<Vec2>) {
    let spacing = spacing.max(MIN_SPACING);
    // 控制多边形长度是弧长的上界，用它定细分密度。
    let bound = (ctrl - from).length() + (to - ctrl).length();
    if bound <= f32::EPSILON {
        return;
    }
    let fine = ((bound / (spacing * 0.5)).ceil() as u32).max(1);

    let mut previous = from;
    // 从上一个落笔点走到 `previous` 已经累计的弧长。
    let mut carried = 0.0;
    for step in 1..=fine {
        let point = at(from, ctrl, to, step as f32 / fine as f32);
        let mut segment = point - previous;
        let mut length = segment.length();
        while length > f32::EPSILON {
            let remaining = spacing - carried;
            if length < remaining {
                carried += length;
                break;
            }
            let dab = previous + segment * (remaining / length);
            out.push(dab);
            previous = dab;
            segment = point - previous;
            length = segment.length();
            carried = 0.0;
        }
        previous = point;
    }
    // 终点必须精确压到（下一段从这里接上）；已经压过了就不重复压。
    if let Some(last) = out.last() {
        if (*last - to).length() <= spacing * 0.05 {
            return;
        }
    }
    out.push(to);
}

/// 二次贝塞尔在 `t` 处的点。
fn at(from: Vec2, ctrl: Vec2, to: Vec2, t: f32) -> Vec2 {
    let u = 1.0 - t;
    from * (u * u) + ctrl * (2.0 * u * t) + to * (t * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_are_spaced_by_arc_length_not_by_curve_parameter() {
        let mut out = Vec::new();
        let from = Vec2::new(0.0, 0.0);
        let to = Vec2::new(10.0, 0.0);
        let spacing = 2.0;
        // 退化二次曲线（控制点 == 起点）在参数上极不均匀，按弧长取点才是等距的。
        curve_dabs(from, from, to, spacing, &mut out);
        assert_eq!(out.last().copied(), Some(to));
        let mut previous = from;
        for point in &out {
            let gap = (*point - previous).length();
            assert!(gap <= spacing, "间距 {gap} 大于采样间距");
            previous = *point;
        }
    }

    #[test]
    fn a_curve_stays_inside_the_hull_of_its_control_points() {
        // 无过冲：二次贝塞尔整体落在控制多边形的包围盒里。
        let (from, ctrl, to) = (
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(20.0, 0.0),
        );
        let mut out = Vec::new();
        curve_dabs(from, ctrl, to, 1.0, &mut out);
        assert!(!out.is_empty());
        for point in &out {
            assert!((0.0..=20.0).contains(&point.x), "{point:?}");
            assert!((0.0..=10.0).contains(&point.y), "{point:?}");
        }
        // 终点是精确的：下一段从这里接上。
        assert_eq!(out.last().copied(), Some(to));
    }

    #[test]
    fn a_zero_length_curve_emits_nothing() {
        let mut out = Vec::new();
        let point = Vec2::new(3.0, 4.0);
        curve_dabs(point, point, point, 1.0, &mut out);
        assert!(out.is_empty());
    }
}
