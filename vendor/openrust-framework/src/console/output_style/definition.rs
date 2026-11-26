use super::super::{TableCell, TableSeparator};
use crate::support::ArrayKey;

/// 定义列表中的标题、保序键值数组或显式分隔线。
///
/// 不支持的联合类型在编译时拒绝：
/// ```compile_fail
/// use openrust::console::DefinitionListItem;
/// let item = DefinitionListItem::from(true);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DefinitionListItem {
    Title(String),
    Entries(Vec<(ArrayKey, TableCell)>),
    Separator(TableSeparator),
}
impl DefinitionListItem {
    pub fn entry(key: impl Into<ArrayKey>, value: impl Into<TableCell>) -> Self {
        Self::Entries(vec![(key.into(), value.into())])
    }
    /// 与数组输入一致，仅首个键值用于渲染；空数组仍保留空项。
    pub fn entries<I, K, V>(entries: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<ArrayKey>,
        V: Into<TableCell>,
    {
        Self::Entries(
            entries
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect(),
        )
    }
}
impl From<&str> for DefinitionListItem {
    fn from(title: &str) -> Self {
        Self::Title(title.to_owned())
    }
}
impl From<String> for DefinitionListItem {
    fn from(title: String) -> Self {
        Self::Title(title)
    }
}
impl From<TableSeparator> for DefinitionListItem {
    fn from(separator: TableSeparator) -> Self {
        Self::Separator(separator)
    }
}
