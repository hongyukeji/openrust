// 第三方许可见 ../LICENSE.symfony。
use std::{ops::Index, sync::Arc};
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Prefixes {
    Static([&'static str; 6]),
    Owned(Arc<[String; 6]>),
}
impl Index<usize> for Prefixes {
    type Output = str;
    fn index(&self, index: usize) -> &str {
        match self {
            Self::Static(parts) => parts[index],
            Self::Owned(parts) => &parts[index],
        }
    }
}
/// 树形连接线的六个只读前缀。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeStyle {
    pub(super) prefixes: Prefixes,
}
impl TreeStyle {
    pub fn new(
        end_has_next: impl Into<String>,
        end_last: impl Into<String>,
        left: impl Into<String>,
        mid_has_next: impl Into<String>,
        mid_last: impl Into<String>,
        right: impl Into<String>,
    ) -> Self {
        Self {
            prefixes: Prefixes::Owned(Arc::new([
                end_has_next.into(),
                end_last.into(),
                left.into(),
                mid_has_next.into(),
                mid_last.into(),
                right.into(),
            ])),
        }
    }
    const fn borrowed(prefixes: [&'static str; 6]) -> Self {
        Self {
            prefixes: Prefixes::Static(prefixes),
        }
    }
    pub const fn r#box() -> Self {
        Self::borrowed(["┃╸ ", "┗╸ ", "", "┃  ", "   ", ""])
    }
    pub const fn box_double() -> Self {
        Self::borrowed(["╠═ ", "╚═ ", "", "║  ", "  ", ""])
    }
    pub const fn compact() -> Self {
        Self::borrowed(["├ ", "└ ", "", "│ ", "  ", ""])
    }
    pub const fn light() -> Self {
        Self::borrowed(["|-- ", "`-- ", "", "|   ", "    ", ""])
    }
    pub const fn minimal() -> Self {
        Self::borrowed([". ", ". ", "", ". ", "  ", ""])
    }
    pub const fn rounded() -> Self {
        Self::borrowed(["├─ ", "╰─ ", "", "│  ", "   ", ""])
    }
    pub fn apply_prefixes(&self, iterator: &mut super::TreeIterator<'_>) {
        iterator.style = self.clone();
    }
}
impl Default for TreeStyle {
    fn default() -> Self {
        Self::borrowed(["├── ", "└── ", "", "│   ", "   ", ""])
    }
}
