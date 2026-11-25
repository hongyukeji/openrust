use super::ConsoleError;
use std::{error::Error, fmt, string::FromUtf8Error};
/// 表格配置与文本渲染错误，保留底层格式化和编码来源。
#[derive(Debug)]
pub enum ConsoleTableError {
    UndefinedStyle(String),
    MissingDefinitions,
    EmptyPadding,
    RowIndexOverflow,
    UnsupportedWrappingFormatter,
    InvalidColumnWidth(i64),
    InvalidFormat(ConsoleError),
    InvalidPaddingEncoding(FromUtf8Error),
}
impl fmt::Display for ConsoleTableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDefinitions => f.write_str("表格命名样式需要应用装配作用域"),
            Self::UndefinedStyle(name) => write!(f, "表格样式未定义：{name}"),
            Self::UnsupportedWrappingFormatter => f.write_str("当前格式器不支持列宽换行"),
            Self::InvalidColumnWidth(width) => write!(f, "表格有效列宽无效：{width}"),
            Self::RowIndexOverflow => f.write_str("表格无法追加行：数字索引已达到上限"),
            Self::EmptyPadding => f.write_str("表格填充字符不能为空"),
            Self::InvalidFormat(error) => write!(f, "表格格式模板无效：{error}"),
            Self::InvalidPaddingEncoding(error) => {
                write!(f, "表格填充结果不是有效的UTF-8文本：{error}")
            }
        }
    }
}
impl Error for ConsoleTableError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidFormat(error) => Some(error),
            Self::InvalidPaddingEncoding(error) => Some(error),
            _ => None,
        }
    }
}
