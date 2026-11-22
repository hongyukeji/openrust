//! Native view values. JSON encoding is an explicit, fallible boundary.
use super::ViewData;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, ops::Index};

#[derive(Clone, Debug, PartialEq)]
enum NumberValue {
    Json(serde_json::Number),
    Float(f64),
}

/// A view number retains non-finite floats without encoding sentinel strings.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewNumber(NumberValue);
impl ViewNumber {
    pub fn as_i64(&self) -> Option<i64> {
        match &self.0 {
            NumberValue::Json(n) => n.as_i64(),
            NumberValue::Float(_) => None,
        }
    }
    pub fn as_u64(&self) -> Option<u64> {
        match &self.0 {
            NumberValue::Json(n) => n.as_u64(),
            NumberValue::Float(_) => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match &self.0 {
            NumberValue::Json(n) => n.as_f64(),
            NumberValue::Float(n) => Some(*n),
        }
    }
    pub fn is_i64(&self) -> bool {
        self.as_i64().is_some()
    }
    pub fn is_u64(&self) -> bool {
        self.as_u64().is_some()
    }
    pub fn is_f64(&self) -> bool {
        match &self.0 {
            NumberValue::Json(n) => n.is_f64(),
            NumberValue::Float(_) => true,
        }
    }
    pub fn is_finite(&self) -> bool {
        self.as_f64().is_some_and(f64::is_finite)
    }
}
impl From<serde_json::Number> for ViewNumber {
    fn from(value: serde_json::Number) -> Self {
        Self(NumberValue::Json(value))
    }
}
impl From<f64> for ViewNumber {
    fn from(value: f64) -> Self {
        Self(
            serde_json::Number::from_f64(value)
                .map_or(NumberValue::Float(value), NumberValue::Json),
        )
    }
}
impl Serialize for ViewNumber {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            NumberValue::Json(n) => n.serialize(serializer),
            NumberValue::Float(_) => Err(serde::ser::Error::custom(
                "Non-finite view number cannot be encoded as JSON.",
            )),
        }
    }
}
impl<'de> Deserialize<'de> for ViewNumber {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        serde_json::Number::deserialize(deserializer).map(Self::from)
    }
}
impl fmt::Display for ViewNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            NumberValue::Json(n) => n.fmt(f),
            NumberValue::Float(n) => f.write_str(&crate::support::input_scalars::double_string(*n)),
        }
    }
}

/// Ordered data passed to views without a JSON serialization round trip.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ViewValue {
    #[default]
    Null,
    Bool(bool),
    Number(ViewNumber),
    String(String),
    Array(Vec<ViewValue>),
    Object(ViewData),
    /// 当前作用域中的可调用对象，不经过 JSON 序列化。
    Instance(super::ObjectValue),
}
impl ViewValue {
    pub fn from_object(value: std::sync::Arc<dyn super::ViewObject>) -> Self {
        Self::Instance(super::ObjectValue::new(value))
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
    pub fn is_boolean(&self) -> bool {
        matches!(self, Self::Bool(_))
    }
    pub fn is_number(&self) -> bool {
        matches!(self, Self::Number(_))
    }
    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }
    pub fn is_array(&self) -> bool {
        matches!(self, Self::Array(_))
    }
    pub fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        if let Self::Bool(v) = self {
            Some(*v)
        } else {
            None
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        if let Self::Number(v) = self {
            v.as_i64()
        } else {
            None
        }
    }
    pub fn as_u64(&self) -> Option<u64> {
        if let Self::Number(v) = self {
            v.as_u64()
        } else {
            None
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        if let Self::Number(v) = self {
            v.as_f64()
        } else {
            None
        }
    }
    pub fn is_i64(&self) -> bool {
        self.as_i64().is_some()
    }
    pub fn is_u64(&self) -> bool {
        self.as_u64().is_some()
    }
    pub fn is_f64(&self) -> bool {
        matches!(self,Self::Number(n) if n.is_f64())
    }
    pub fn as_array(&self) -> Option<&Vec<Self>> {
        if let Self::Array(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Self>> {
        if let Self::Array(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn as_object(&self) -> Option<&ViewData> {
        if let Self::Object(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn as_object_mut(&mut self) -> Option<&mut ViewData> {
        if let Self::Object(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn get<I: ViewIndex>(&self, key: I) -> Option<&Self> {
        key.lookup(self)
    }
    pub fn get_mut<I: ViewIndex>(&mut self, key: I) -> Option<&mut Self> {
        key.lookup_mut(self)
    }
    pub fn take(&mut self) -> Self {
        std::mem::take(self)
    }
    /// Rejects non-finite numbers recursively instead of silently replacing them with null.
    pub fn to_json(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}
impl Serialize for ViewValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(v) => v.serialize(serializer),
            Self::Number(v) => v.serialize(serializer),
            Self::String(v) => v.serialize(serializer),
            Self::Array(v) => v.serialize(serializer),
            Self::Object(v) => v.serialize(serializer),
            Self::Instance(_) => Err(serde::ser::Error::custom("视图服务对象不能序列化。")),
        }
    }
}
impl<'de> Deserialize<'de> for ViewValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        serde_json::Value::deserialize(deserializer).map(Self::from)
    }
}
impl From<serde_json::Value> for ViewValue {
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(v) => Self::Bool(v),
            serde_json::Value::Number(v) => Self::Number(v.into()),
            serde_json::Value::String(v) => Self::String(v),
            serde_json::Value::Array(v) => Self::Array(v.into_iter().map(Self::from).collect()),
            serde_json::Value::Object(v) => {
                Self::Object(v.into_iter().map(|(k, v)| (k, v.into())).collect())
            }
        }
    }
}
impl From<ViewData> for ViewValue {
    fn from(v: ViewData) -> Self {
        Self::Object(v)
    }
}
impl<T: Into<ViewValue>> From<Vec<T>> for ViewValue {
    fn from(v: Vec<T>) -> Self {
        Self::Array(v.into_iter().map(Into::into).collect())
    }
}
impl<T: Into<ViewValue>> From<Option<T>> for ViewValue {
    fn from(v: Option<T>) -> Self {
        v.map_or(Self::Null, Into::into)
    }
}
impl From<bool> for ViewValue {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}
impl From<String> for ViewValue {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}
impl From<&str> for ViewValue {
    fn from(v: &str) -> Self {
        Self::String(v.to_owned())
    }
}
impl From<f64> for ViewValue {
    fn from(v: f64) -> Self {
        Self::Number(v.into())
    }
}
impl From<f32> for ViewValue {
    fn from(v: f32) -> Self {
        f64::from(v).into()
    }
}
macro_rules! integer_values {
    ($($kind:ty),*) => { $(
        impl From<$kind> for ViewValue { fn from(v: $kind) -> Self { Self::Number(serde_json::Number::from(v).into()) } }
    )* };
}
integer_values!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);
/// String keys address objects; numeric indices address lists, as with JSON input values.
pub trait ViewIndex {
    #[doc(hidden)]
    fn lookup(self, value: &ViewValue) -> Option<&ViewValue>;
    #[doc(hidden)]
    fn lookup_mut(self, value: &mut ViewValue) -> Option<&mut ViewValue>;
}
impl ViewIndex for &str {
    fn lookup(self, value: &ViewValue) -> Option<&ViewValue> {
        value.as_object()?.get(self)
    }
    fn lookup_mut(self, value: &mut ViewValue) -> Option<&mut ViewValue> {
        value.as_object_mut()?.get_mut(self)
    }
}
impl ViewIndex for &String {
    fn lookup(self, value: &ViewValue) -> Option<&ViewValue> {
        self.as_str().lookup(value)
    }
    fn lookup_mut(self, value: &mut ViewValue) -> Option<&mut ViewValue> {
        self.as_str().lookup_mut(value)
    }
}
impl ViewIndex for usize {
    fn lookup(self, value: &ViewValue) -> Option<&ViewValue> {
        value.as_array()?.get(self)
    }
    fn lookup_mut(self, value: &mut ViewValue) -> Option<&mut ViewValue> {
        value.as_array_mut()?.get_mut(self)
    }
}
impl<I: ViewIndex> Index<I> for ViewValue {
    type Output = Self;
    fn index(&self, key: I) -> &Self {
        self.get(key).unwrap_or(&Self::Null)
    }
}
impl fmt::Display for ViewValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        serde_json::to_string(self).map_err(|_| fmt::Error)?.fmt(f)
    }
}

impl PartialEq<str> for ViewValue {
    fn eq(&self, rhs: &str) -> bool {
        self.as_str() == Some(rhs)
    }
}
impl PartialEq<&str> for ViewValue {
    fn eq(&self, rhs: &&str) -> bool {
        self.as_str() == Some(*rhs)
    }
}
impl PartialEq<String> for ViewValue {
    fn eq(&self, rhs: &String) -> bool {
        self.as_str() == Some(rhs.as_str())
    }
}
impl PartialEq<bool> for ViewValue {
    fn eq(&self, rhs: &bool) -> bool {
        self.as_bool() == Some(*rhs)
    }
}
impl PartialEq<serde_json::Value> for ViewValue {
    fn eq(&self, rhs: &serde_json::Value) -> bool {
        match (self, rhs) {
            (Self::Null, serde_json::Value::Null) => true,
            (Self::Bool(a), serde_json::Value::Bool(b)) => a == b,
            (Self::String(a), serde_json::Value::String(b)) => a == b,
            (Self::Number(ViewNumber(NumberValue::Json(a))), serde_json::Value::Number(b)) => {
                a == b
            }
            (Self::Array(a), serde_json::Value::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a == b)
            }
            (Self::Object(a), serde_json::Value::Object(b)) => {
                a.len() == b.len() && a.iter().all(|(k, v)| b.get(k).is_some_and(|b| v == b))
            }
            _ => false,
        }
    }
}
