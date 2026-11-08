//! RFC 3986 查询输出及 PHP 数组编码，共用于 URI 与 HTTP 请求。

use super::{data, scalars};
use crate::{Error, Result};
use serde_json::{Map, Value};

pub(crate) const MAX_OUTPUT_BYTES: usize = 128 * 1024;

/// 按 PHP `http_build_query` 的 RFC3986 模式编码当前输入树。
pub(crate) fn encode(value: &Value) -> Result<String> {
    validate(value)?;
    let mut output = String::new();
    for (key, value) in entries(value)? {
        encode_entry(&key, value, &mut output)?;
    }
    Ok(output)
}

pub(crate) fn validate(value: &Value) -> Result<()> {
    if !matches!(value, Value::Object(_) | Value::Array(_)) {
        return Err(invalid());
    }
    data::validate_limits(value)?;
    // 输入树限制展开键；另在 clone 前限制实际字符串总量，避免调用方注入无界值。
    validate_bytes(value, &mut 0)
}

fn validate_bytes(value: &Value, total: &mut usize) -> Result<()> {
    match value {
        Value::String(text) => *total = total.saturating_add(text.len()),
        Value::Array(values) => {
            for value in values {
                validate_bytes(value, total)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                *total = total.saturating_add(key.len());
                validate_bytes(value, total)?;
            }
        }
        _ => {}
    }
    if *total > MAX_OUTPUT_BYTES {
        return Err(data::limit_error());
    }
    Ok(())
}

pub(crate) fn entries(value: &Value) -> Result<Vec<(String, &Value)>> {
    match value {
        Value::Object(values) => Ok(values
            .iter()
            .map(|(key, value)| (key.clone(), value))
            .collect()),
        Value::Array(values) => Ok(values
            .iter()
            .enumerate()
            .map(|(index, value)| (index.to_string(), value))
            .collect()),
        _ => Err(invalid()),
    }
}

/// PHP 数组展开与 `array_merge` 的顶层规则。
pub(crate) fn merge_arrays(left: &Value, right: &Value) -> Result<Value> {
    validate(left)?;
    validate(right)?;
    let mut merged = Map::new();
    let mut next = 0_usize;
    for (key, value) in entries(left)?.into_iter().chain(entries(right)?) {
        if key
            .parse::<i64>()
            .is_ok_and(|number| number.to_string() == key)
        {
            merged.insert(next.to_string(), value.clone());
            next += 1;
        } else {
            merged.insert(key, value.clone());
        }
    }
    Ok(data::packed(merged))
}

fn encode_entry(key: &str, value: &Value, output: &mut String) -> Result<()> {
    match value {
        Value::Null => Ok(()),
        Value::Array(_) | Value::Object(_) => {
            for (child_key, child) in entries(value)? {
                encode_entry(&format!("{key}[{child_key}]"), child, output)?;
            }
            Ok(())
        }
        scalar => {
            if !output.is_empty() {
                append(output, "&")?;
            }
            escape(key, output)?;
            append(output, "=")?;
            match scalar {
                Value::String(text) => escape(text, output),
                // http_build_query(false) 为 0，区别于 PHP 普通字符串转换的空串。
                Value::Bool(value) => append(output, if *value { "1" } else { "0" }),
                _ => escape(&scalars::string(scalar), output),
            }
        }
    }
}

const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

fn append(output: &mut String, text: &str) -> Result<()> {
    if output.len().saturating_add(text.len()) > MAX_OUTPUT_BYTES {
        return Err(data::limit_error());
    }
    output.push_str(text);
    Ok(())
}

fn escape(text: &str, output: &mut String) -> Result<()> {
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            if output.len() >= MAX_OUTPUT_BYTES {
                return Err(data::limit_error());
            }
            output.push(char::from(byte));
        } else {
            let encoded = [
                b'%',
                HEX_UPPER[usize::from(byte >> 4)],
                HEX_UPPER[usize::from(byte & 15)],
            ];
            // 这里三个字节均来自固定 ASCII 表，不接触输入 UTF-8 解码。
            if output.len().saturating_add(3) > MAX_OUTPUT_BYTES {
                return Err(data::limit_error());
            }
            output.extend(encoded.into_iter().map(char::from));
        }
    }
    Ok(())
}

pub(crate) fn invalid() -> Error {
    Error::custom_with_status(
        http::StatusCode::BAD_REQUEST,
        "url_query_invalid",
        "URL查询参数必须是数组或对象",
    )
}
