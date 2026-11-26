use super::TableCell;

/// 区分单层与多层表头，避免把嵌套数组当作单元文本。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableHeaders {
    Cells(Vec<TableCell>),
    Rows(Vec<Vec<TableCell>>),
}
impl<I, T> From<I> for TableHeaders
where
    I: IntoIterator<Item = T>,
    T: Into<TableCell>,
{
    fn from(headers: I) -> Self {
        Self::Cells(headers.into_iter().map(Into::into).collect())
    }
}
impl TableHeaders {
    pub fn rows<I, R, T>(rows: I) -> Self
    where
        I: IntoIterator<Item = R>,
        R: IntoIterator<Item = T>,
        T: Into<TableCell>,
    {
        Self::Rows(
            rows.into_iter()
                .map(|row| row.into_iter().map(Into::into).collect())
                .collect(),
        )
    }
}
