//! 主页：画廊条目（打开中的工作区 + 最近文件）、回到主页、画廊点击请求。

use super::*;

/// 主页画廊的条目：打开中的工作区 + 磁盘上的最近工作区（按路径去重）。
pub(super) fn home_entries(workspaces: &Workspaces) -> Vec<HomeEntry> {
    let mut entries: Vec<HomeEntry> = workspaces
        .tabs()
        .iter()
        .enumerate()
        .map(|(index, tab)| HomeEntry {
            name: tab.name.clone(),
            detail: if tab.dirty {
                "打开中 · 未保存".to_string()
            } else {
                "打开中".to_string()
            },
            open_index: Some(index),
            path: None,
        })
        .collect();
    let open_paths = workspaces.paths();
    for path in crate::io::load_recent() {
        if open_paths.contains(&path) {
            continue;
        }
        let summary = crate::io::read_workspace_summary(&path);
        let name = match &summary {
            Ok(summary) => summary.name.clone(),
            Err(_) => path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("工作区")
                .to_string(),
        };
        let detail = match &summary {
            Ok(summary) => format!(
                "{} × {} · {} 图层",
                summary.width, summary.height, summary.layers
            ),
            Err(_) => path.display().to_string(),
        };
        entries.push(HomeEntry {
            name,
            detail,
            open_index: None,
            path: Some(path),
        });
    }
    entries
}

impl ImageEditorLogic {
    /// 回到主页（工作区保留在内存里）。
    pub(super) fn go_home(&mut self) {
        let mut home = HomeView::new(self.theme, home_entries(&self.workspaces));
        if let Some(measurer) = self.measurer.clone() {
            home.set_text_measurer(measurer);
        }
        self.screen = Screen::Home(home);
        self.title_dirty = true;
    }

    /// 主页画廊的点击请求。
    pub(super) fn handle_home_requests(&mut self, ctx: &FrameContext<'_>) {
        let request = match &self.screen {
            Screen::Home(home) => home.take_open_request(),
            Screen::Editor => None,
        };
        match request {
            Some(HomeOpenRequest::Workspace(index)) => {
                self.workspaces.set_active(index);
                if let Some(workspace) = self.workspaces.active_mut() {
                    workspace.editor.mark_texture_dirty();
                }
                self.screen = Screen::Editor;
                self.title_dirty = true;
            }
            Some(HomeOpenRequest::Recent(path)) => self.open_workspace(ctx, &path),
            None => {}
        }
    }
}
