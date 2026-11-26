//! League 9.4.1 `AbstractGrant::parseParam` 的请求标量边界。
//!
//! 依据 MIT 许可的 thephpleague/oauth2-server 固定源码；参数值不进入调试输出。

use serde_json::{Map, Value};

use super::OauthError;

/// OAuth 请求中已解析的参数集合；字符串、数字、布尔值遵循 PHP 标量转换。
///
/// 传输层负责解析表单或查询字符串；数组和对象在读取时作为无效请求拒绝。
/// 此类型不将重复参数自动合并为数组，也不负责选择不同传输来源的优先级。
#[derive(Clone, Default)]
pub struct OauthParameters {
    values: Map<String, Value>,
}

impl std::fmt::Debug for OauthParameters {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OauthParameters")
            .finish_non_exhaustive()
    }
}

impl OauthParameters {
    pub(crate) fn raw_string_matches(&self, name: &str, expected: &str) -> bool {
        self.values.get(name).and_then(Value::as_str) == Some(expected)
    }

    /// 使用传输层解析后的参数建立集合，不记录凭据值。
    #[must_use]
    pub fn new(values: Map<String, Value>) -> Self {
        Self { values }
    }

    // JSON body复用原标量/错误/trim路径；只有空或缺失才使用Basic原字节默认。
    pub(crate) fn secret_parameter(
        &self,
        name: &str,
        default: Option<&[u8]>,
    ) -> Result<Option<zeroize::Zeroizing<Vec<u8>>>, OauthError> {
        if let Some(value) = self.parameter(name, None)? {
            return Ok(Some(zeroize::Zeroizing::new(value.into_bytes())));
        }
        Ok(normalized_bytes(default).map(|value| zeroize::Zeroizing::new(value.to_vec())))
    }

    /// 读取参数，裁剪 PHP 默认六字符集合；空值或缺失值使用同样裁剪后的默认值。
    ///
    /// `null` 等同缺失；`true` 转为 `"1"`，`false` 转为空值。FF 与 Unicode 空白保留。
    /// 数组或对象始终返回该参数的 `invalid_request`，即使提供了默认值。
    pub fn parameter(
        &self,
        name: &str,
        default: Option<&str>,
    ) -> Result<Option<String>, OauthError> {
        let value = match self.values.get(name) {
            None | Some(Value::Null) => None,
            Some(Value::Array(_) | Value::Object(_)) => {
                return Err(OauthError::invalid_request(name, None));
            }
            Some(value) => {
                let scalar = match value {
                    Value::String(value) => value.clone(),
                    Value::Bool(value) => if *value { "1" } else { "" }.into(),
                    Value::Number(value) => value
                        .as_i64()
                        .map(|n| n.to_string())
                        .or_else(|| value.as_u64().map(|n| n.to_string()))
                        .unwrap_or_else(|| {
                            openrust::support::php_float_string(
                                value.as_f64().expect("JSON number"),
                            )
                        }),
                    _ => return Err(OauthError::invalid_request(name, None)),
                };
                normalized(Some(&scalar)).map(str::to_owned)
            }
        };
        Ok(value.or_else(|| normalized(default).map(str::to_owned)))
    }
}

// 六ASCII字节trim保任意UTF8以外的secret，尤其FF和内部NUL不改变。
pub(super) fn normalized_bytes(value: Option<&[u8]>) -> Option<&[u8]> {
    let value = value?;
    let trim = |byte: &u8| matches!(*byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0);
    let first = value.iter().position(|byte| !trim(byte))?;
    let last = value.iter().rposition(|byte| !trim(byte))?;
    Some(&value[first..=last])
}

// PHP trim 不包含 form feed；避免与 Rust 的 Unicode trim 混淆。
pub(super) fn normalized(value: Option<&str>) -> Option<&str> {
    value
        .map(|value| {
            value.trim_matches(|character| {
                matches!(character, ' ' | '\t' | '\n' | '\r' | '\u{000b}' | '\0')
            })
        })
        .filter(|value| !value.is_empty())
}
