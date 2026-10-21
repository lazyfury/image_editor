//! 页面组装：纵列 = 菜单栏 → 标签条预留行 → 选项栏 → 中间一行 → 状态栏。
//!
//! 顺序就是界面从上到下的顺序，改布局只看这一个函数。各块自己怎么建在
//! [`panels`](super::panels) 里。

use super::*;

pub(super) fn page(
    theme: &'static dyn Theme,
    state: &Rc<RefCell<AppState>>,
    wiring: &mut Wiring,
) -> (impl Component, Lists) {
    let (row, lists) = panels::main_row(theme, state, wiring);
    let page = Flex::column()
        .gap(0.0)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(menu::menu_bar(
            theme,
            wiring.intents.clone(),
            &mut wiring.menu_refs,
            &wiring.refs.menu_bar,
        ))
        // 菜单栏和选项栏之间给宿主留一条带子画工作区标签条（默认 0 高）。
        .child(
            Flex::row()
                .padding(Edges::ZERO)
                .gap(0.0)
                .mouse_filter(MouseFilter::Ignore)
                .ref_(&wiring.refs.tab_band),
        )
        .child(panels::options_strip(theme, wiring))
        .child(row)
        .child(status_bar::status_bar(
            theme,
            &wiring.refs.tool,
            &wiring.refs.message,
            &wiring.refs.zoom,
        ));
    (page, lists)
}
