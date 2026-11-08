//! 查询字符串的非改名解码与 Laravel 点路径变更。

use super::{Uri, parse};
use crate::{
    Error, Result,
    support::{data, php_variables::VariableBag, query_encoding, scalars},
};
use serde_json::Value;
use std::{collections::HashSet, fmt};

/// 借用 URI 查询字符串；读取保留 RFC 3986 解码后的键名。
#[derive(Clone, Copy)]
pub struct UriQueryString<'a> {
    raw: &'a str,
}

impl<'a> UriQueryString<'a> {
    /// 取得编码后的查询文本，空查询与缺失在此入口均返回空串。
    pub fn value(&self) -> &'a str {
        self.raw
    }

    /// 按 rawurldecode 解码查询字节，保留字面 `+` 与非 UTF-8 字节。
    pub fn decode_bytes(&self) -> Vec<u8> {
        parse::decode_bytes(self.raw)
    }

    /// 解码整个查询文本，非 UTF-8 输入返回错误。
    pub fn decode(&self) -> Result<String> {
        String::from_utf8(self.decode_bytes()).map_err(|_| invalid_encoding())
    }

    /// 解析 PHP 数组形状。重复键后值覆盖，括号索引保留原顺序。
    pub fn to_array(&self) -> Result<Value> {
        if self.raw.len() > query_encoding::MAX_OUTPUT_BYTES {
            return Err(data::limit_error());
        }
        let mut bag = VariableBag::default();
        for (index, pair) in self
            .raw
            .split('&')
            .filter(|pair| !pair.is_empty())
            .enumerate()
        {
            if index >= 1024 {
                return Err(data::limit_error());
            }
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let key = decode(key)?;
            bag.insert_unmangled(&key, decode(value)?)?;
        }
        let value = bag.into_value();
        data::validate_limits(&value)?;
        Ok(value)
    }

    /// 取得全部解码参数。
    pub fn all(&self) -> Result<Value> {
        self.to_array()
    }

    /// 选择若干键；优先读取完整键，按点路径写入结果并保留缺失项为 null。
    pub fn all_keys(&self, keys: &[&str]) -> Result<Value> {
        let source = self.to_array()?;
        if keys.is_empty() {
            return Ok(source);
        }
        let mut selected = empty_query();
        for key in keys {
            data::set(
                &mut selected,
                key,
                data::arr_get(&source, key).cloned().unwrap_or(Value::Null),
            )?;
        }
        Ok(selected)
    }

    /// 按 `data_get` 读取嵌套路径，支持通配符。缺失返回 `None`。
    pub fn get(&self, key: &str) -> Result<Option<Value>> {
        data::checked_get(&self.to_array()?, key)
    }

    /// 检查完整键或嵌套路径是否存在。
    pub fn has(&self, key: &str) -> Result<bool> {
        Ok(data::has(&self.to_array()?, key))
    }

    /// 检查完整键及嵌套路径是否都缺失。
    pub fn missing(&self, key: &str) -> Result<bool> {
        self.has(key).map(|present| !present)
    }
}

impl fmt::Display for UriQueryString<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.raw)
    }
}

impl fmt::Debug for UriQueryString<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("UriQueryString([REDACTED])")
    }
}

impl Uri {
    /// 取得结构化查询入口，保留原始 URI 值。
    pub fn query(&self) -> UriQueryString<'_> {
        UriQueryString {
            raw: self.query_string().unwrap_or_default(),
        }
    }

    /// 按 Laravel 的查询文本替换规则生成解码后的展示字符串。
    pub fn decode(&self) -> Result<String> {
        let query = self.query();
        if query.to_array()? == empty_query() {
            return Ok(self.value().to_owned());
        }
        Ok(self.value().replace(query.value(), &query.decode()?))
    }

    /// 按点路径合并查询参数，返回新的 URI。
    pub fn with_query(&self, additional: &Value) -> Result<Self> {
        self.update_query(self.query().to_array()?, additional)
    }

    /// 完整替换查询参数。空数组和省略后的空结果清除查询分隔符。
    pub fn replace_query(&self, query: &Value) -> Result<Self> {
        self.update_query(empty_query(), query)
    }

    /// 仅设置原 URI 中缺失的查询路径。
    pub fn with_query_if_missing(&self, additional: &Value) -> Result<Self> {
        let current = self.query().to_array()?;
        query_encoding::validate(additional)?;
        let mut missing = additional.clone();
        for (key, _) in query_encoding::entries(additional)? {
            if data::has(&current, &key) {
                data::forget(&mut missing, &key)?;
            }
        }
        self.update_query(current, &missing)
    }

    /// 移除完整键或点路径，保留其余键的顺序及稀疏数字索引。
    pub fn without_query(&self, keys: &[&str]) -> Result<Self> {
        let mut query = self.query().to_array()?;
        for key in keys {
            data::forget(&mut query, key)?;
        }
        self.replace_query(&query)
    }

    /// 向查询参数追加值，现有列表按 PHP 字符串规则去重，关联数组按展开语义合并。
    pub fn push_onto_query(&self, key: &str, value: Value) -> Result<Self> {
        let mut query = self.query().to_array()?;
        let new = wrap(value);
        let value = match data::checked_get(&query, key)? {
            Some(current @ Value::Array(_)) => {
                unique_list(&query_encoding::merge_arrays(&current, &new)?)?
            }
            Some(current @ Value::Object(_)) => query_encoding::merge_arrays(&current, &new)?,
            Some(value) if !value.is_null() => {
                query_encoding::merge_arrays(&Value::Array(vec![value]), &new)?
            }
            _ => new,
        };
        data::set(&mut query, key, value)?;
        self.with_encoded_query(&query)
    }

    fn update_query(&self, mut current: Value, additional: &Value) -> Result<Self> {
        query_encoding::validate(additional)?;
        for (key, value) in query_encoding::entries(additional)? {
            data::set(&mut current, &key, value.clone())?;
        }
        self.with_encoded_query(&current)
    }

    fn with_encoded_query(&self, query: &Value) -> Result<Self> {
        let encoded = query_encoding::encode(query)?;
        let mut parts = self.parts.clone();
        parts.query = (!encoded.is_empty()).then_some(encoded);
        Self::checked_parts(parts)
    }
}

fn empty_query() -> Value {
    Value::Array(Vec::new())
}

fn wrap(value: Value) -> Value {
    match value {
        Value::Null => empty_query(),
        Value::Array(_) | Value::Object(_) => value,
        value => Value::Array(vec![value]),
    }
}

fn unique_list(value: &Value) -> Result<Value> {
    let mut keys = HashSet::new();
    let mut unique = Vec::new();
    for (_, value) in query_encoding::entries(value)? {
        if value.is_array() || value.is_object() {
            return Err(Error::custom(
                "support_uri_query_scalar_required",
                "查询列表去重需要标量",
            ));
        }
        if keys.insert(scalars::string(value)) {
            unique.push(value.clone());
        }
    }
    Ok(Value::Array(unique))
}

fn decode(value: &str) -> Result<String> {
    String::from_utf8(parse::decode_bytes(value)).map_err(|_| invalid_encoding())
}

fn invalid_encoding() -> Error {
    Error::custom(
        "support_uri_query_encoding",
        "URI 查询参数必须使用有效 UTF-8",
    )
}
