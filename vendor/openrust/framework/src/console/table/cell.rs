use std::fmt::{self, Display};
/// 保留文本、样式及分隔标记；普通纯量与显式对象沿用同一个渲染管线。
#[derive(Clone, Debug)]
pub struct TableCell {
    value: String,
    style: Option<super::TableCellStyle>,
    separator: bool,
    explicit: bool,
}
impl PartialEq for TableCell {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value && self.style == other.style && self.separator == other.separator
    }
}
impl Eq for TableCell {}
impl TableCell {
    pub fn new(value: impl Into<Self>) -> Self {
        let value: Self = value.into();
        Self {
            value: value.value,
            style: None,
            separator: false,
            explicit: true,
        }
    }
    fn text(value: String) -> Self {
        Self {
            value,
            style: None,
            separator: false,
            explicit: false,
        }
    }
    pub fn with_style(mut self, style: super::TableCellStyle) -> Self {
        self.style = Some(style);
        self.explicit = true;
        self
    }
    pub fn get_style(&self) -> Option<&super::TableCellStyle> {
        self.style.as_ref()
    }
    pub(super) fn attributes(&self) -> Option<super::cell_style::Attributes> {
        if self.separator {
            Some(super::cell_style::Attributes::Separator)
        } else {
            self.style
                .clone()
                .map(super::cell_style::Attributes::Style)
                .or_else(|| {
                    self.explicit
                        .then_some(super::cell_style::Attributes::Plain)
                })
        }
    }
    pub(super) fn separator() -> Self {
        Self {
            value: String::new(),
            style: None,
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
    fn from(_: super::TableSeparator) -> Self {
        Self::separator()
    }
}
impl From<&TableCell> for TableCell {
    fn from(cell: &TableCell) -> Self {
        cell.clone()
    }
}
