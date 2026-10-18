//! 选区描边：黑白相间的「马赛克线段」（marching ants）。
//!
//! 纯色 1px 线在浅色画布上几乎看不见（之前用的是半透明的 `palette.selection`，
//! 那是给填充用的）。这里沿选区轮廓（[`Selection::outline`] 给的线段）拆成等长
//! 的黑白线段交替拼出来，不管画布是亮是暗，总有一半线段能看清。画在文档图像
//! 之上、UI 之下。

use igui_core::{Color, Vec2};
use igui_render::{Paint, PaintContext};

/// 每段线段的长度（逻辑像素）。
const DASH: f32 = 5.0;
/// 线段宽度。
const WIDTH: f32 = 2.0;

/// 沿一串线段（屏幕坐标）画黑白相间的马赛克。
pub fn paint(ctx: &mut PaintContext, edges: &[(Vec2, Vec2)]) {
    let colors = [Paint::new(Color::BLACK), Paint::new(Color::WHITE)];
    let mut index = 0usize;
    for (from, to) in edges {
        let (from, to) = (*from, *to);
        let length = (to - from).length();
        if length <= 0.0 {
            continue;
        }
        let direction = (to - from) / length;
        let mut offset = 0.0;
        while offset < length {
            let segment = DASH.min(length - offset);
            ctx.draw_line(
                from + direction * offset,
                from + direction * (offset + segment),
                WIDTH,
                colors[index % 2],
            );
            offset += segment;
            index += 1;
        }
    }
}

/// 沿一条闭合折线画（套索拖动中 / 矩形填充预览用）。
pub fn paint_polyline(ctx: &mut PaintContext, points: &[Vec2]) {
    if points.len() < 2 {
        return;
    }
    let edges: Vec<(Vec2, Vec2)> = (0..points.len())
        .map(|index| (points[index], points[(index + 1) % points.len()]))
        .collect();
    paint(ctx, &edges);
}

#[cfg(test)]
mod tests {
    use super::*;
    use igui_core::{Rect, Size};
    use igui_render::DrawCommand;

    /// 矩形轮廓的 4 个角点。
    fn rect_points(rect: Rect) -> [Vec2; 4] {
        [
            rect.min(),
            Vec2::new(rect.right(), rect.top()),
            rect.max(),
            Vec2::new(rect.left(), rect.bottom()),
        ]
    }

    /// 闭合折线 -> 线段，方便测试。
    fn edges(points: &[Vec2]) -> Vec<(Vec2, Vec2)> {
        (0..points.len())
            .map(|index| (points[index], points[(index + 1) % points.len()]))
            .collect()
    }

    fn border(edges: &[(Vec2, Vec2)]) -> Vec<(Color, f32)> {
        let mut ctx = PaintContext::new();
        paint(&mut ctx, edges);
        ctx.into_draw_list()
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Line {
                    from, to, paint, ..
                } => Some((paint.color, (*to - *from).length())),
                _ => None,
            })
            .collect()
    }

    /// 边框同时有黑白两种线段，才在亮底 / 暗底上都看得见。
    #[test]
    fn the_border_alternates_black_and_white() {
        let points = rect_points(Rect::from_min_size(
            Vec2::new(10.0, 20.0),
            Size::new(100.0, 60.0),
        ));
        let lines = border(&edges(&points));
        assert!(
            lines.iter().any(|(color, _)| *color == Color::BLACK),
            "缺少黑段"
        );
        assert!(
            lines.iter().any(|(color, _)| *color == Color::WHITE),
            "缺少白段"
        );
    }

    /// 线段把整个周长铺满（每段不超过 `DASH`，总和等于周长）。
    #[test]
    fn the_dashes_cover_the_whole_perimeter() {
        let rect = Rect::from_min_size(Vec2::new(10.0, 20.0), Size::new(100.0, 60.0));
        let total: f32 = border(&edges(&rect_points(rect)))
            .iter()
            .map(|(_, length)| length)
            .sum();
        let perimeter = 2.0 * (rect.size.width + rect.size.height);
        assert!(
            (total - perimeter).abs() < 1e-3,
            "total={total} perimeter={perimeter}"
        );
    }

    /// 少于两个点不画（空 / 单点选区）。
    #[test]
    fn a_degenerate_outline_draws_nothing() {
        assert!(border(&[]).is_empty());
        let mut ctx = PaintContext::new();
        paint_polyline(&mut ctx, &[Vec2::new(1.0, 1.0)]);
        assert!(ctx.into_draw_list().commands().is_empty());
    }
}
