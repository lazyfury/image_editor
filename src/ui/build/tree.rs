//! 建整棵树（`build` 的第一步）：共享格 → 页面 → `SceneTree`。
//!
//! 三步各自成模块：共享格在 [`cells`](super::cells)，各面板 / 条带在
//! [`panels`](super::panels)，页面顺序在 [`page`](super::page)。这里只串起来，
//! 顺便把页面挂到根上（布局根的子节点按 anchors 摆，flex 从下一层才开始，所以
//! 页面 column 必须是根的唯一子节点，demo_app / file_browser 同款形状）。

use super::*;

pub(super) fn build_tree(
    theme: &'static dyn Theme,
    state: &Rc<RefCell<AppState>>,
) -> (SceneTree, NodeId, Wiring, Lists) {
    let mut wiring = Wiring::new();
    let (page, lists) = page::page(theme, state, &mut wiring);

    let page_ref = NodeRef::new();
    let root_flex = Flex::new()
        .gap(0.0)
        .padding(Edges::ZERO)
        .mouse_filter(MouseFilter::Ignore)
        .child(page.ref_(&page_ref));
    let mut tree = SceneTree::new();
    tree.add_child(tree.root(), root_flex);
    let page = page_ref.get().expect("页面已挂载");
    (tree, page, wiring, lists)
}
