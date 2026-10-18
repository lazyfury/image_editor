//! 工具栏分组：把相关工具收进一个「组按钮」，长按展开切换。
//!
//! 组内第一个工具是默认「脸」——组按钮显示它的图标 / 标签，短按激活它；长按
//! 弹出浮层，列出组内全部工具。键盘快捷键 / 浮层选中的工具会把所属组的「脸」
//! 更新成它（见 [`crate::ui::EditorView`]），所以激活组的图标永远等于当前工具。

use super::state::ActiveTool;

/// 工具栏上的一个槽位：单工具，或一组工具。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolGroup {
    /// 组内工具，顺序即浮层里的顺序；第一个是默认「脸」。
    pub tools: &'static [ActiveTool],
}

impl ToolGroup {
    /// 默认「脸」。
    pub const fn face(self) -> ActiveTool {
        self.tools[0]
    }

    /// 是否只有一个工具（单工具槽位不画「可展开」标记）。
    pub const fn is_single(self) -> bool {
        self.tools.len() == 1
    }

    pub fn contains(self, tool: ActiveTool) -> bool {
        self.tools.contains(&tool)
    }
}

/// 工具栏从上到下的分组。
///
/// 画笔 / 橡皮各自单独一组；矩形填充和自由填充归一组；油漆桶单独一组。
pub const TOOL_GROUPS: [ToolGroup; 7] = [
    ToolGroup {
        tools: &[ActiveTool::Move],
    },
    ToolGroup {
        tools: &[ActiveTool::Brush],
    },
    ToolGroup {
        tools: &[ActiveTool::Eraser],
    },
    ToolGroup {
        tools: &[ActiveTool::RectangleSelect, ActiveTool::LassoSelect],
    },
    ToolGroup {
        tools: &[ActiveTool::PaintBucket, ActiveTool::Gradient],
    },
    ToolGroup {
        tools: &[ActiveTool::RectangleFill, ActiveTool::FreeFill],
    },
    ToolGroup {
        tools: &[ActiveTool::Eyedropper],
    },
];

/// `tool` 属于哪一组（[`TOOL_GROUPS`] 的下标）。
pub fn group_index(tool: ActiveTool) -> Option<usize> {
    TOOL_GROUPS.iter().position(|group| group.contains(tool))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个工具恰好属于一个组，且分组覆盖全部工具。
    #[test]
    fn every_tool_belongs_to_exactly_one_group() {
        for tool in ActiveTool::ALL {
            let matches = TOOL_GROUPS.iter().filter(|g| g.contains(tool)).count();
            assert_eq!(matches, 1, "{tool:?} 应恰好属于一个组");
        }
        assert_eq!(group_index(ActiveTool::Eraser), Some(2));
        assert_eq!(group_index(ActiveTool::LassoSelect), Some(3));
        assert_eq!(group_index(ActiveTool::Gradient), Some(4));
        assert_eq!(group_index(ActiveTool::FreeFill), Some(5));
    }

    #[test]
    fn the_default_faces_are_the_first_tool_of_each_group() {
        assert_eq!(TOOL_GROUPS[1].face(), ActiveTool::Brush);
        assert_eq!(TOOL_GROUPS[2].face(), ActiveTool::Eraser);
        assert_eq!(TOOL_GROUPS[3].face(), ActiveTool::RectangleSelect);
        assert_eq!(TOOL_GROUPS[4].face(), ActiveTool::PaintBucket);
        assert_eq!(TOOL_GROUPS[5].face(), ActiveTool::RectangleFill);
        // 画笔 / 橡皮 / 移动 / 吸管是单工具槽位；选区 / 油漆桶 / 填充形状可展开。
        assert!(TOOL_GROUPS[0].is_single());
        assert!(TOOL_GROUPS[1].is_single());
        assert!(TOOL_GROUPS[2].is_single());
        assert!(!TOOL_GROUPS[3].is_single());
        assert!(!TOOL_GROUPS[4].is_single());
        assert!(!TOOL_GROUPS[5].is_single());
        assert!(TOOL_GROUPS[6].is_single());
    }
}
