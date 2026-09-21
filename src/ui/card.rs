//! 项目内的简单卡片：和左侧调色盘面板同一套外观。
//!
//! 一个竖直 `Flex`：`surface(SurfaceLevel::Surface)` 底 + 内边距 + 行间距，
//! 不带边框 / 圆角。右栏的「文件 / 图层 / 属性 / 历史」和左侧调色盘都用它，
//! 于是侧栏各块看起来是同一系列的卡片（而不是核心 `draw_components::Card`
//! 的 raised 圆角风格）。
//!
//! 只是外观容器：标题、分隔线、内容由调用方自己 `.child(..)`。

use draw_components::{Component, Spec};
use draw_core::{Color, Edges};
use draw_theme::{space, SurfaceLevel, Theme};
use draw_ui::{FlexStyle, MouseFilter, SurfaceStyle, Widget};

/// 一块侧栏卡片：surface 底 + 内边距 + 纵向间距。
pub struct Card {
    spec: Spec,
    theme: &'static dyn Theme,
    gap: f32,
    padding: Edges,
    /// 覆盖卡片底色（`None` = 用主题的 `SurfaceLevel::Raised`）。
    fill: Option<Color>,
}

impl Card {
    /// 默认：`space::SM` 的内边距和行间距（和调色盘面板一致）。
    pub fn new(theme: &'static dyn Theme) -> Self {
        Self {
            spec: Spec::default(),
            theme,
            gap: space::SM,
            padding: Edges::all(space::SM),
            fill: None,
        }
    }

    /// 行间距（覆盖默认的 `space::SM`）。
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// 覆盖卡片底色（否则用主题的 raised 面）。
    ///
    /// 这是 `Card` 自己的方法，优先级高于默认底色；`prepare` 不会把它冲掉。
    pub fn background(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }

    pub fn padding(mut self, padding: Edges) -> Self {
        self.padding = padding;
        self
    }
}

impl Component for Card {
    fn spec(&mut self) -> &mut Spec {
        &mut self.spec
    }

    fn name(&self) -> &'static str {
        "Card"
    }

    fn widget(&self) -> Widget {
        Widget::Flex(FlexStyle::column().gap(self.gap).padding(self.padding))
    }

    fn prepare(&mut self) {
        let theme = self.theme;
        let fill = self.fill;
        // 容器本身不吃指针，卡片里的按钮 / 列表照常命中。
        self.spec.data.mouse_filter = MouseFilter::Ignore;
        // 调用方用 `.surface(..)` / `.dynamic_background(..)` 设过背景就别覆盖；
        // 否则用 `.background(color)` 的覆盖色，再否则用主题的 raised 面。
        if self.spec.background.is_none() {
            self.spec.background = Some(Box::new(move |_| {
                SurfaceStyle::new(fill.unwrap_or_else(|| theme.surface(SurfaceLevel::Raised)))
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use draw_ui::InteractState;

    fn resolved_fill(card: &Card) -> Color {
        (card.spec.background.as_ref().expect("background"))(InteractState::default()).fill
    }

    #[test]
    fn an_explicit_background_overrides_the_theme_surface() {
        let mut card = Card::new(crate::theme::editor_theme(false)).background(Color::RED);
        card.prepare();
        assert_eq!(resolved_fill(&card), Color::RED);
    }

    #[test]
    fn the_theme_surface_is_the_default() {
        let theme = crate::theme::editor_theme(false);
        let mut card = Card::new(theme);
        card.prepare();
        assert_eq!(resolved_fill(&card), theme.surface(SurfaceLevel::Raised));
    }
}
