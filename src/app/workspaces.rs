//! 多工作区：一个工作区 = 一个 [`EditorView`]（含 `AppState`）+ 落盘信息。
//!
//! 宿主只把 `event` / `layout` / `paint` 发给**当前激活**的那个编辑器；切换工作区
//! 就是换 `active`。工作区文件路径和上次保存时的文档 revision 用来算脏标记。

use std::path::PathBuf;

use crate::ui::{EditorView, WorkspaceTab};

/// 一个打开中的工作区。
pub struct Workspace {
    pub editor: EditorView,
    /// 保存过的 `.igui` 路径；没保存过为 `None`。
    pub path: Option<PathBuf>,
    /// 上次保存时的文档 revision。
    pub saved_revision: u64,
}

impl Workspace {
    /// 新建（未落盘）的工作区。
    pub fn new(editor: EditorView) -> Self {
        let saved_revision = editor.document_revision();
        Self {
            editor,
            path: None,
            saved_revision,
        }
    }

    /// 从文件打开的工作区。
    pub fn from_path(editor: EditorView, path: PathBuf) -> Self {
        let saved_revision = editor.document_revision();
        Self {
            editor,
            path: Some(path),
            saved_revision,
        }
    }

    /// 文档改过、还没保存。
    pub fn dirty(&self) -> bool {
        self.editor.document_revision() != self.saved_revision
    }

    /// 保存成功：记下路径与 revision。
    pub fn mark_saved(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.saved_revision = self.editor.document_revision();
    }

    /// 标签 / 窗口标题用的名字：优先文件名，否则文档名。
    pub fn title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|path| path.file_stem())
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| self.editor.document_name())
    }

    /// 标签条要的信息。
    pub fn tab(&self) -> WorkspaceTab {
        WorkspaceTab {
            name: self.title(),
            dirty: self.dirty(),
        }
    }
}

/// 打开中的工作区列表 + 激活下标。
#[derive(Default)]
pub struct Workspaces {
    items: Vec<Workspace>,
    active: usize,
}

impl Workspaces {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn set_active(&mut self, index: usize) {
        if index < self.items.len() {
            self.active = index;
        }
    }

    /// 追加一个工作区并激活它，返回它的下标。
    pub fn push(&mut self, workspace: Workspace) -> usize {
        self.items.push(workspace);
        self.active = self.items.len() - 1;
        self.active
    }

    pub fn active(&self) -> Option<&Workspace> {
        self.items.get(self.active)
    }

    pub fn active_mut(&mut self) -> Option<&mut Workspace> {
        self.items.get_mut(self.active)
    }

    /// 按下标取一个工作区（确认关闭 / 保存指定工作区用）。
    pub fn get(&self, index: usize) -> Option<&Workspace> {
        self.items.get(index)
    }

    /// 按下标取一个工作区（可变）。
    pub fn get_mut(&mut self, index: usize) -> Option<&mut Workspace> {
        self.items.get_mut(index)
    }

    /// 关闭一个工作区；激活下标跟着收缩。返回是否删掉了。
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        self.items.remove(index);
        // 删掉的是激活之前的：后面的都往前挪一格，激活下标也跟着减一。
        if index < self.active {
            self.active -= 1;
        }
        if self.active >= self.items.len() {
            self.active = self.items.len().saturating_sub(1);
        }
        true
    }

    /// 标签条要的列表。
    pub fn tabs(&self) -> Vec<WorkspaceTab> {
        self.items.iter().map(Workspace::tab).collect()
    }

    /// 已经打开的工作区文件路径（画廊去重用）。
    pub fn paths(&self) -> Vec<PathBuf> {
        self.items
            .iter()
            .filter_map(|workspace| workspace.path.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AppState;

    fn editor() -> EditorView {
        EditorView::new(crate::theme::editor_theme(false), AppState::default())
    }

    fn from_path(name: &str) -> Workspace {
        Workspace::from_path(editor(), PathBuf::from(format!("/tmp/{name}.igui")))
    }

    #[test]
    fn pushing_activates_the_new_workspace() {
        let mut workspaces = Workspaces::new();
        assert!(workspaces.is_empty());
        workspaces.push(from_path("A"));
        workspaces.push(from_path("B"));
        assert_eq!(workspaces.tabs().len(), 2);
        assert_eq!(workspaces.active_index(), 1);
        assert_eq!(workspaces.active().unwrap().title(), "B");
    }

    #[test]
    fn closing_shrinks_the_active_index() {
        let mut workspaces = Workspaces::new();
        workspaces.push(from_path("A"));
        workspaces.push(from_path("B"));
        assert!(workspaces.remove(1));
        assert_eq!(workspaces.active_index(), 0);
        assert_eq!(workspaces.active().unwrap().title(), "A");
        assert!(workspaces.remove(0));
        assert!(workspaces.is_empty());
    }

    #[test]
    fn closing_an_earlier_tab_keeps_the_same_workspace_active() {
        let mut workspaces = Workspaces::new();
        workspaces.push(from_path("A"));
        workspaces.push(from_path("B"));
        workspaces.push(from_path("C"));
        workspaces.set_active(2); // C
        assert!(workspaces.remove(0)); // 关掉 A
        assert_eq!(workspaces.active().unwrap().title(), "C", "激活的仍是 C");
    }

    #[test]
    fn a_new_workspace_is_clean_until_saved_then_edited() {
        let mut workspace = Workspace::new(editor());
        assert!(!workspace.dirty(), "刚建的工作区不脏");
        workspace.mark_saved(PathBuf::from("/tmp/a.igui"));
        assert!(!workspace.dirty());
        assert_eq!(workspace.title(), "a");
    }
}
