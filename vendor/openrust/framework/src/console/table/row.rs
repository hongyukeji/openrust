use super::TableCell;
/// 整行分隔线；与空数据行区分，不参与列宽计算。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TableSeparator {
    _private: (),
}
impl TableSeparator {
    pub fn new() -> Self {
        Self::default()
    }
}
/// 异构行集合保留数据行与显式分隔线的区别。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableRow {
    Cells(Vec<TableCell>),
    Separator(TableSeparator),
}
impl<I, T> From<I> for TableRow
where
    I: IntoIterator<Item = T>,
    T: Into<TableCell>,
{
    fn from(row: I) -> Self {
        Self::Cells(row.into_iter().map(Into::into).collect())
    }
}
impl From<TableSeparator> for TableRow {
    fn from(separator: TableSeparator) -> Self {
        Self::Separator(separator)
    }
}

impl From<&TableRow> for TableRow {
    fn from(row: &TableRow) -> Self {
        row.clone()
    }
}
impl From<&TableSeparator> for TableRow {
    fn from(separator: &TableSeparator) -> Self {
        Self::Separator(*separator)
    }
}
