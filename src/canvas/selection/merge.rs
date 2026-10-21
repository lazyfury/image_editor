//! 选区合并：Shift 并集 / Alt 差集，以及从修饰键推合并模式。

use super::Selection;

impl Selection {
    /// 并集（加选）：两块掩码取或，面积 / 包围盒 / 轮廓跟着重算。
    ///
    /// 尺寸不同（文档换了）时退回 `self`，不 panic。
    pub fn union(&self, other: &Self) -> Self {
        if (self.width, self.height) != (other.width, other.height) {
            return self.clone();
        }
        // 并集取较大覆盖率（硬掩码下等价于逻辑或）。
        let mask = self
            .mask
            .iter()
            .zip(&other.mask)
            .map(|(a, b)| (*a).max(*b))
            .collect();
        Self::from_mask((self.width, self.height), mask)
    }

    /// 差集（减选）：从自己身上挖掉 `other`。
    pub fn subtract(&self, other: &Self) -> Self {
        if (self.width, self.height) != (other.width, other.height) {
            return self.clone();
        }
        // 差集按覆盖率相减（硬掩码下等价于 a && !b）。
        let mask = self
            .mask
            .iter()
            .zip(&other.mask)
            .map(|(a, b)| a.saturating_sub(*b))
            .collect();
        Self::from_mask((self.width, self.height), mask)
    }
}

/// 新选区怎么合到旧选区上：框选 / 套索 / 魔棒按住 Shift 加、Alt 减。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MergeMode {
    /// 直接替换旧选区。
    #[default]
    Replace,
    /// 并集（Shift）。
    Add,
    /// 差集（Alt）。
    Subtract,
}

impl MergeMode {
    /// 从修饰键推：`Alt` 优先于 `Shift`（和 PS 一致）。
    pub const fn from_modifiers(add: bool, subtract: bool) -> Self {
        if subtract {
            Self::Subtract
        } else if add {
            Self::Add
        } else {
            Self::Replace
        }
    }

    /// 把 `next` 合到 `base` 上；结果为空就返回 `None`（等于没有选区）。
    pub fn apply(self, base: Option<&Selection>, next: Selection) -> Option<Selection> {
        let merged = match (self, base) {
            (Self::Add, Some(base)) => base.union(&next),
            (Self::Subtract, Some(base)) => base.subtract(&next),
            _ => next,
        };
        (!merged.is_empty()).then_some(merged)
    }
}
