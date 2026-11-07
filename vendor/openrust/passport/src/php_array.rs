//! PHP array 的公开读取载体；保留关联键与顺序并恢复PHP列表形状，不复制 PHP 比较内核。

use openrust::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 保留列表或关联数组形状、异质值与声明顺序。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Value", into = "Value")]
pub struct PhpArray(Value);
impl PhpArray {
    /// 返回原始 JSON 载体，不字符串化数组元素。
    #[must_use]
    pub fn as_value(&self) -> &Value {
        &self.0
    }
    /// 按原数组值的顺序迭代；关联键保留在载体，不参与 `in_array`。
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.0
            .as_array()
            .into_iter()
            .flatten()
            .chain(self.0.as_object().into_iter().flat_map(|map| map.values()))
    }
    /// 空数组没有成员；PHP空数组统一序列化为列表。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values().next().is_none()
    }
    /// 非 strict PHP `in_array`；比较由唯一框架互操作桥负责。
    #[must_use]
    pub fn contains_loose(&self, needle: &str) -> bool {
        self.values()
            .any(|value| openrust::support::php_loose_string_value_equal(needle, value))
    }
    /// 严格字符串成员判断；供 URI 白名单使用，不把 bool/number 当字符串。
    #[must_use]
    pub fn contains_strict_string(&self, needle: &str) -> bool {
        self.values().any(|value| value.as_str() == Some(needle))
    }
}
impl Default for PhpArray {
    fn default() -> Self {
        Self(Value::Array(Vec::new()))
    }
}
impl TryFrom<Value> for PhpArray {
    type Error = Error;
    fn try_from(value: Value) -> Result<Self> {
        if value.is_array() || value.is_object() {
            Ok(Self(normalize_array_values(value)))
        } else {
            Err(Error::custom_with_status(
                openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "passport_client_attribute_invalid",
                "客户端属性必须是数组",
            ))
        }
    }
}
impl From<Vec<String>> for PhpArray {
    fn from(values: Vec<String>) -> Self {
        Self(Value::Array(
            values.into_iter().map(Value::String).collect(),
        ))
    }
}
impl From<PhpArray> for Value {
    fn from(value: PhpArray) -> Self {
        value.0
    }
}

// JSON关联对象按PHP array规则转为顺序列表；保留非连续键及显式声明顺序。
fn normalize_array_values(value: Value) -> Value {
    match value {
        Value::Array(values) => {
            Value::Array(values.into_iter().map(normalize_array_values).collect())
        }
        Value::Object(values) => {
            let list = values
                .keys()
                .enumerate()
                .all(|(index, key)| *key == index.to_string());
            let entries = values
                .into_iter()
                .map(|(key, value)| (key, normalize_array_values(value)));
            if list {
                Value::Array(entries.map(|(_, value)| value).collect())
            } else {
                Value::Object(entries.collect())
            }
        }
        scalar => scalar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn php_empty_and_sequential_numeric_objects_serialize_as_lists_recursively() -> Result<()> {
        for (source, expected) in [
            (json!({}), json!([])),
            (json!({"0":"read"}), json!(["read"])),
            (json!({"0":{},"1":{"0":"nested"}}), json!([[], ["nested"]])),
        ] {
            let array = PhpArray::try_from(source)?;
            assert_eq!(
                serde_json::to_value(array).map_err(Error::internal)?,
                expected
            );
        }
        Ok(())
    }

    #[test]
    fn nonlist_keys_and_declared_order_are_not_reindexed() -> Result<()> {
        for source in [
            r#"{"1":"read"}"#,
            r#"{"01":"read"}"#,
            r#"{"1":"first","0":"second"}"#,
        ] {
            let value: Value = serde_json::from_str(source).map_err(Error::internal)?;
            let array = PhpArray::try_from(value)?;
            assert_eq!(
                serde_json::to_string(&array).map_err(Error::internal)?,
                source
            );
        }
        Ok(())
    }
}
