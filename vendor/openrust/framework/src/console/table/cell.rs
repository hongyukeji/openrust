use std::fmt::{self, Display};
/// 保留文本、样式及分隔标记；普通纯量与显式对象沿用同一个渲染管线。
#[derive(Clone, Debug)]
pub struct TableCell {
    value: String,
    options: Option<Box<TableCellOptions>>,
    separator: bool,
    explicit: bool,
}
impl PartialEq for TableCell {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
            && self.separator == other.separator
            && self.get_style() == other.get_style()
            && self.get_colspan() == other.get_colspan()
            && self.get_rowspan() == other.get_rowspan()
    }
}
impl Eq for TableCell {}
impl TableCell {
    pub fn new(value: impl Into<Self>) -> Self {
        let value: Self = value.into();
        Self {
            value: value.value,
            options: None,
            separator: false,
            explicit: true,
        }
    }
    fn text(value: String) -> Self {
        Self {
            value,
            options: None,
            separator: false,
            explicit: false,
        }
    }
    fn options_mut(&mut self) -> &mut TableCellOptions {
        self.options
            .get_or_insert_with(|| Box::new(TableCellOptions::default()))
    }
    pub fn with_style(mut self, style: super::TableCellStyle) -> Self {
        self.options_mut().style = Some(style);
        self.explicit = true;
        self
    }
    pub fn get_style(&self) -> Option<&super::TableCellStyle> {
        self.options
            .as_deref()
            .and_then(|options| options.style.as_ref())
    }
    pub fn with_options(mut self, options: TableCellOptions) -> Self {
        self.options = Some(Box::new(options));
        self.explicit = true;
        self
    }
    pub fn with_colspan(mut self, value: impl Into<serde_json::Value>) -> Self {
        self.options_mut().colspan = value.into();
        self.explicit = true;
        self
    }
    pub fn with_rowspan(mut self, value: impl Into<serde_json::Value>) -> Self {
        self.options_mut().rowspan = value.into();
        self.explicit = true;
        self
    }
    pub fn get_colspan(&self) -> i64 {
        self.options.as_ref().map_or(1, |options| {
            crate::support::input_scalars::integer(&options.colspan)
        })
    }
    pub fn get_rowspan(&self) -> i64 {
        self.options.as_ref().map_or(1, |options| {
            crate::support::input_scalars::integer(&options.rowspan)
        })
    }
    pub(super) fn attributes(&self) -> Option<super::cell_style::Attributes> {
        self.explicit.then(|| super::cell_style::Attributes {
            style: self.get_style().cloned(),
            separator: self.separator,
            colspan: self.get_colspan(),
            rowspan: self.get_rowspan(),
        })
    }
    pub(super) fn separator() -> Self {
        Self {
            value: String::new(),
            options: None,
            separator: true,
            explicit: true,
        }
    }
    pub fn display(value: impl Display) -> Self {
        Self::text(value.to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.value
    }
}
impl Display for TableCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}
impl From<String> for TableCell {
    fn from(value: String) -> Self {
        Self::text(value)
    }
}
impl From<&String> for TableCell {
    fn from(value: &String) -> Self {
        Self::text(value.clone())
    }
}
impl From<&str> for TableCell {
    fn from(value: &str) -> Self {
        Self::text(value.to_owned())
    }
}
impl From<serde_json::Value> for TableCell {
    fn from(value: serde_json::Value) -> Self {
        Self::text(crate::support::input_scalars::string(value))
    }
}
impl From<&serde_json::Value> for TableCell {
    fn from(value: &serde_json::Value) -> Self {
        value.clone().into()
    }
}
macro_rules! scalar {
    ($($ty:ty),*)=>{$(impl From<$ty> for TableCell {fn from(value:$ty)->Self {serde_json::json!(value).into()}})*};
}
scalar!(bool, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl From<f64> for TableCell {
    fn from(value: f64) -> Self {
        if value.is_nan() {
            Self::text("NAN".into())
        } else if value.is_infinite() {
            Self::text(
                if value.is_sign_negative() {
                    "-INF"
                } else {
                    "INF"
                }
                .into(),
            )
        } else {
            serde_json::json!(value).into()
        }
    }
}
impl From<f32> for TableCell {
    fn from(value: f32) -> Self {
        if value.is_finite() {
            serde_json::json!(value).into()
        } else {
            f64::from(value).into()
        }
    }
}

impl From<super::TableSeparator> for TableCell {
    fn from(separator: super::TableSeparator) -> Self {
        separator.cell
    }
}
impl From<&TableCell> for TableCell {
    fn from(cell: &TableCell) -> Self {
        cell.clone()
    }
}

/// 可选构造参数；跨度复用既有纯量转换，样式通过类型系统限定为有效对象。
///
/// 未知选项由正式参数类型拒绝：
/// ```compile_fail,E0560
/// use openrust::console::TableCellOptions;
/// let _ = TableCellOptions { unknown: serde_json::Value::Null, ..Default::default() };
/// ```
/// 样式必须是对应对象，不能以普通值冒充：
/// ```compile_fail,E0308
/// use openrust::console::TableCellOptions;
/// let _ = TableCellOptions { style: Some(serde_json::json!("invalid")), ..Default::default() };
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableCellOptions {
    pub colspan: serde_json::Value,
    pub rowspan: serde_json::Value,
    pub style: Option<super::TableCellStyle>,
}
impl Default for TableCellOptions {
    fn default() -> Self {
        Self {
            colspan: 1.into(),
            rowspan: 1.into(),
            style: None,
        }
    }
}
impl Default for TableCell {
    fn default() -> Self {
        Self::new("")
    }
}
