use std::fmt::{self, Display};
/// 文本单元格；JSON标量沿用框架的标量转换，业务类型可显式使用Display适配。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableCell(String);
impl TableCell {
    pub fn new(value: impl Into<Self>) -> Self {
        value.into()
    }
    pub fn display(value: impl Display) -> Self {
        Self(value.to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl Display for TableCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl From<String> for TableCell {
    fn from(value: String) -> Self {
        Self(value)
    }
}
impl From<&String> for TableCell {
    fn from(value: &String) -> Self {
        Self(value.clone())
    }
}
impl From<&str> for TableCell {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}
impl From<serde_json::Value> for TableCell {
    fn from(value: serde_json::Value) -> Self {
        Self(crate::support::input_scalars::string(value))
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
            Self("NAN".into())
        } else if value.is_infinite() {
            Self(
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
