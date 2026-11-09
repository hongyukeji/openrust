//! Eloquent 属性赋值中的 PHP 有序数组路径。
use super::{
    AttributeCast, AttributeValue, ModelMetadata,
    attribute_cast::{encode_projection_json, from_json, scalar_string},
};
use crate::{Error, Result};
use serde_json::{Map, Value};

pub(super) fn write(
    metadata: &ModelMetadata,
    base: &str,
    path: &str,
    value: AttributeValue,
    raw: Option<&AttributeValue>,
    full_cast: Option<&AttributeCast>,
) -> Result<AttributeValue> {
    let value = value.into_array_json()?;
    let value = if !value.is_null() && encodes_json(full_cast) {
        check_depth(&value)?;
        encode_projection_json(&value, full_cast)?
    } else {
        value
    };
    let base_cast = metadata.write_cast(base)?;
    #[cfg(feature = "encryption")]
    let encrypter = match &base_cast {
        Some(AttributeCast::Encrypted(cast)) => Some(cast.encrypter()),
        Some(AttributeCast::Class(cast)) => cast.encrypter(),
        _ => None,
    };
    let root = match raw.filter(|raw| !raw.is_null()) {
        None => Value::Null,
        #[cfg(feature = "encryption")]
        Some(raw) if let Some(encrypter) = encrypter => {
            let plain = super::native_encrypted_cast::decrypt(encrypter, raw)?;
            super::native_encrypted_cast::decode_plain_json(&plain, true, true)?
        }
        Some(raw) => from_json(raw.as_json()?, true)?,
    };
    let document = update(root, path, value)?;
    check_depth(&document)?;
    let json = encode_projection_json(&document, base_cast.as_ref())?;
    #[cfg(feature = "encryption")]
    if let Some(encrypter) = encrypter {
        let json = super::native_encrypted_cast::encoded_plaintext(json)?;
        return encrypter
            .encrypt_string(json.as_bytes())
            .map(|value| Value::String(value).into());
    }
    Ok(json.into())
}

fn encodes_json(cast: Option<&AttributeCast>) -> bool {
    match cast {
        #[cfg(feature = "encryption")]
        Some(AttributeCast::Encrypted(cast)) => cast.encodes_json(),
        Some(cast) => cast.is_json_projection(),
        None => false,
    }
}

fn update(root: Value, path: &str, mut value: Value) -> Result<Value> {
    let path = path.replace("->", ".");
    let keys = path.split('.');
    // Laravel JSON 编码默认深度为 512，路径超过该边界无法发布。
    if keys.clone().count() > 512 {
        return Err(path_error());
    }
    let mut current = match root {
        Value::Null | Value::Bool(false) => Value::Object(Map::new()),
        value @ (Value::Array(_) | Value::Object(_)) => value,
        Value::String(text) if !path.contains('.') => {
            return update_string_offset(text, &path, &value);
        }
        _ => return Err(path_error()),
    };
    let mut parents = Vec::new();
    for key in keys {
        let mut parent = match current {
            Value::Object(values) => values,
            Value::Array(values) => values
                .into_iter()
                .enumerate()
                .map(|(index, value)| (index.to_string(), value))
                .collect(),
            _ => Map::new(),
        };
        // 原位占位保留旧键位置，新增键依 PHP 规则插入末尾。
        current = std::mem::take(parent.entry(key.to_owned()).or_insert(Value::Null));
        parents.push((parent, key));
    }
    for (mut parent, key) in parents.into_iter().rev() {
        parent.insert(key.to_owned(), value);
        value = if parent
            .keys()
            .enumerate()
            .all(|(index, key)| *key == index.to_string())
        {
            Value::Array(parent.into_values().collect())
        } else {
            Value::Object(parent)
        };
    }
    Ok(value)
}

fn update_string_offset(text: String, offset: &str, value: &Value) -> Result<Value> {
    let offset = offset
        .trim_ascii()
        .parse::<i64>()
        .map_err(|_| path_error())?;
    let mut bytes = text.into_bytes();
    let index = if offset.is_negative() {
        bytes
            .len()
            .checked_sub(usize::try_from(offset.unsigned_abs()).map_err(|_| path_error())?)
    } else {
        usize::try_from(offset).ok()
    }
    .ok_or_else(path_error)?;
    let size = index.checked_add(1).ok_or_else(path_error)?;
    // 一个数字路径不可在没有相应输入的情况下分配任意大小的填充。
    let growth = size.saturating_sub(bytes.len());
    if growth > 1024 * 1024 {
        return Err(path_error());
    }
    let value = scalar_string(value)?;
    // Laravel 的默认异常处理器会抛出空字符串及多字节赋值的 PHP 警告。
    let [first] = value.as_bytes() else {
        return Err(path_error());
    };
    if growth != 0 {
        bytes.try_reserve(growth).map_err(|_| path_error())?;
        bytes.resize(size, b' ');
    }
    bytes[index] = *first;
    String::from_utf8(bytes)
        .map(Value::String)
        .map_err(|_| path_error())
}

fn check_depth(document: &Value) -> Result<()> {
    let mut pending = vec![(document, 0)];
    while let Some((value, depth)) = pending.pop() {
        if depth >= 512 && matches!(value, Value::Array(_) | Value::Object(_)) {
            return Err(path_error());
        }
        match value {
            Value::Array(values) => pending.extend(values.iter().map(|value| (value, depth + 1))),
            Value::Object(values) => {
                pending.extend(values.values().map(|value| (value, depth + 1)));
            }
            _ => {}
        }
    }
    Ok(())
}

fn path_error() -> Error {
    Error::custom(
        "model_json_path_assignment_failed",
        "模型 JSON 路径无法写入",
    )
}

#[cfg(test)]
#[path = "../../../tests/unit/database/eloquent/json_path.rs"]
mod tests;
