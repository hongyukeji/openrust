//! 有界动态属性对象；不拥有或重新解析 HTTP 正文。

use crate::Result;
use serde::Serialize;
use serde_json::Value;

/// 拥有属性快照的对象；显式 getter 替代 PHP 魔术属性访问。
#[derive(Clone, Serialize)]
#[serde(transparent)]
pub struct Fluent {
    attributes: Value,
    #[serde(skip)]
    calendar: super::CalendarContext,
}
impl std::fmt::Debug for Fluent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Fluent").finish_non_exhaustive()
    }
}
impl Default for Fluent {
    fn default() -> Self {
        Self {
            attributes: Value::Array(Vec::new()),
            calendar: super::CalendarContext::default(),
        }
    }
}
impl Fluent {
    /// 仅数组或对象产生属性；标量按 PHP foreach 无属性处理。
    pub fn from_value(value: Value) -> Result<Self> {
        super::data::validate_limits(&value)?;
        Ok(Self {
            attributes: match value {
                Value::Array(_) | Value::Object(_) => super::data::cast_json(value),
                _ => Value::Array(Vec::new()),
            },
            calendar: super::CalendarContext::default(),
        })
    }

    pub(crate) fn from_stored_attributes(value: Value) -> Option<Self> {
        // 持久属性已经过 cast 解码。构造 Fluent 只重建外层 PHP 数组。
        let attributes = match value {
            value @ Value::Array(_) => value,
            Value::Object(values) => super::data::packed(values),
            _ => return None,
        };
        Some(Self {
            attributes,
            calendar: super::CalendarContext::default(),
        })
    }
    pub(crate) fn with_calendar(mut self, calendar: super::CalendarContext) -> Self {
        self.calendar = calendar;
        self
    }
    /// 返回全部属性快照。
    pub fn all(&self) -> Value {
        self.attributes.clone()
    }
    /// 按 dot path 读取属性，显式 null 保持存在。
    pub fn get(&self, key: &str) -> Result<Option<Value>> {
        super::data::checked_get(&self.attributes, key)
    }
    /// 按直接键读取，点号不解释为路径。
    pub fn value(&self, key: &str) -> Option<&Value> {
        match &self.attributes {
            Value::Object(values) => values.get(key),
            Value::Array(values) => key
                .parse::<usize>()
                .ok()
                .filter(|index| index.to_string() == key)
                .and_then(|index| values.get(index)),
            _ => None,
        }
    }
    /// 判断全部路径存在；null 算存在。
    pub fn has(&self, keys: &[&str]) -> bool {
        keys.iter()
            .all(|key| super::data::has(&self.attributes, key))
    }
    /// 原子设置 dot path，失败保持原属性不变。
    pub fn set(&mut self, key: &str, value: Value) -> Result<&mut Self> {
        super::data::set(&mut self.attributes, key, value)?;
        Ok(self)
    }
    /// 原子删除路径，数组索引不重新编号。
    pub fn forget(&mut self, key: &str) -> Result<&mut Self> {
        super::data::forget(&mut self.attributes, key)?;
        Ok(self)
    }
    /// 创建指定属性的独立对象；标量包装为数组元素。
    pub fn scope(&self, key: &str) -> Result<Self> {
        Ok(Self::from_value(super::data::cast_json(
            self.get(key)?.unwrap_or(Value::Null),
        ))?
        .with_calendar(self.calendar.clone()))
    }
    /// 获取指定路径的属性子集。
    pub fn only(&self, keys: &[&str]) -> Result<Value> {
        super::data::only(&self.attributes, keys)
    }
    /// 判断对象是否没有属性。
    pub fn is_empty(&self) -> bool {
        match &self.attributes {
            Value::Array(v) => v.is_empty(),
            Value::Object(v) => v.is_empty(),
            _ => true,
        }
    }
    /// 按原 PHP 数组形状输出属性。
    pub fn to_value(&self) -> Value {
        self.all()
    }
    /// 按对象携带的时区与每次调用的时钟解析日期。
    pub fn date(&self, key: &str) -> Result<Option<super::InputDate>> {
        self.date_with(key, super::DateOptions::default())
    }
    /// 使用 PHP 格式与可选显式时区解析日期，不读取或重新解析正文。
    pub fn date_with(
        &self,
        key: &str,
        options: super::DateOptions<'_>,
    ) -> Result<Option<super::InputDate>> {
        let value = self.get(key)?.unwrap_or(Value::Null);
        super::calendar::parse_date(&value, options, &self.calendar)
    }
    /// 读取保留原分量的 Carbon 区间，不进行自动进位。
    pub fn interval(&self, key: &str) -> Result<Option<super::InputInterval>> {
        let value = self.get(key)?;
        super::calendar::parse_interval(value.as_ref(), None)
    }
    /// 使用显式 Carbon Unit 名称读取区间。
    pub fn interval_with_unit(
        &self,
        key: &str,
        unit: impl AsRef<str>,
    ) -> Result<Option<super::InputInterval>> {
        let value = self.get(key)?;
        super::calendar::parse_interval(value.as_ref(), Some(unit.as_ref()))
    }
    super::typed::typed_methods!(attributes);
    super::typed::bag_methods!(attributes);
}

impl super::DataObject for Fluent {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn json_projection(&self) -> Option<Value> {
        Some(self.to_value())
    }
}
