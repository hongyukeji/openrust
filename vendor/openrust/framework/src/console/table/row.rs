use super::TableCell;
/// 整行分隔线；与空数据行区分，不参与列宽计算。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableSeparator {
    pub(super) cell: TableCell,
}
impl Default for TableSeparator {
    fn default() -> Self {
        Self {
            cell: TableCell::separator(),
        }
    }
}
impl TableSeparator {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_options(mut self, options: super::TableCellOptions) -> Self {
        self.cell = self.cell.with_options(options);
        self
    }
    pub fn get_colspan(&self) -> i64 {
        self.cell.get_colspan()
    }
    pub fn get_rowspan(&self) -> i64 {
        self.cell.get_rowspan()
    }
    pub fn get_style(&self) -> Option<&super::TableCellStyle> {
        self.cell.get_style()
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
        Self::Separator(separator.clone())
    }
}
