//! League 9.4.1 `AbstractGrant::parseParam` 的请求标量边界。
//!
//! 依据 MIT 许可的 thephpleague/oauth2-server 固定源码；参数值不进入调试输出。

use serde_json::{Map, Value};

use super::OAuthError;

/// OAuth 请求中已解析的参数集合；字符串、数字、布尔值遵循 PHP 标量转换。
///
/// 传输层负责解析表单或查询字符串；数组和对象在读取时作为无效请求拒绝。
/// 此类型不将重复参数自动合并为数组，也不负责选择不同传输来源的优先级。
#[derive(Clone, Default)]
pub struct OAuthParameters {
    values: Map<String, Value>,
}

impl std::fmt::Debug for OAuthParameters {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OAuthParameters")
            .finish_non_exhaustive()
    }
}

impl OAuthParameters {
    pub(crate) fn is_set(&self, name: &str) -> bool {
        self.values.get(name).is_some_and(|value| !value.is_null())
    }

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
    ) -> Result<Option<zeroize::Zeroizing<Vec<u8>>>, OAuthError> {
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
    ) -> Result<Option<String>, OAuthError> {
        let value = match self.values.get(name) {
            None | Some(Value::Null) => None,
            Some(Value::Array(_) | Value::Object(_)) => {
                return Err(OAuthError::invalid_request(name, None));
            }
            Some(value) => {
                let Some(Value::String(scalar)) =
                    openrust::database::eloquent::AttributeCast::String
                        .read(Some(value.clone()))
                        .map_err(|_| OAuthError::invalid_request(name, None))?
                else {
                    return Err(OAuthError::invalid_request(name, None));
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scalar_conversion_and_default_follow_upstream() -> Result<(), OAuthError> {
        for (value, expected) in [
            (Value::Null, "plain"),
            (json!(false), "plain"),
            (json!(true), "1"),
            (json!(42), "42"),
            (json!(1.5), "1.5"),
            (json!(" \t\r\n\u{b}\0"), "plain"),
            (json!("\0 S256 \n"), "S256"),
            (json!("\u{c}"), "\u{c}"),
            (json!("\u{a0}"), "\u{a0}"),
        ] {
            let values = Map::from_iter([("method".to_owned(), value)]);
            assert_eq!(
                OAuthParameters::new(values).parameter("method", Some(" plain\n"))?,
                Some(expected.to_owned())
            );
        }
        let empty = OAuthParameters::default();
        assert_eq!(empty.parameter("absent", None)?, None);
        assert_eq!(empty.parameter("absent", Some("\0 \n"))?, None);
        Ok(())
    }

    #[test]
    fn arrays_and_objects_never_fall_back_or_expose_values() {
        for value in [json!(["secret"]), json!({"secret":"token"})] {
            let parameters = OAuthParameters::new(Map::from_iter([("code".to_owned(), value)]));
            let result = parameters.parameter("code", Some("fallback"));
            assert!(result.is_err());
            let error = result.err();
            assert!(!format!("{parameters:?}").contains("secret"));
            assert!(!format!("{error:?}").contains("secret"));
        }
    }
}

#[cfg(test)]
mod byte_secret_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn body_scalar_secret_reuses_parameter_rules_and_only_empty_uses_raw_default()
    -> Result<(), OAuthError> {
        for (body, expected) in [
            (Value::Null, b"\xff\0mid".as_slice()),
            (json!(false), b"\xff\0mid".as_slice()),
            (json!(" \t\r\n\u{b}\0"), b"\xff\0mid".as_slice()),
            (json!(true), b"1".as_slice()),
            (json!(0), b"0".as_slice()),
            (json!(" \0body\0middle\0 "), b"body\0middle".as_slice()),
            (json!("\u{c}"), b"\x0c".as_slice()),
            (json!("\u{a0}"), "\u{a0}".as_bytes()),
        ] {
            let parameters =
                OAuthParameters::new(Map::from_iter([("client_secret".to_owned(), body)]));
            let actual =
                parameters.secret_parameter("client_secret", Some(b" \t\0\xff\0mid\0\n"))?;
            assert_eq!(
                actual.as_ref().map(|secret| secret.as_slice()),
                Some(expected)
            );
        }
        Ok(())
    }

    #[test]
    fn binary_default_trim_is_exactly_six_ascii_bytes() -> Result<(), OAuthError> {
        let parameters = OAuthParameters::default();
        for (raw, expected) in [
            (b" \t\r\n\x0b\0".as_slice(), None),
            (
                b" \xff\xfe\x80\0middle\0 ".as_slice(),
                Some(b"\xff\xfe\x80\0middle".as_slice()),
            ),
            (
                b"\x0csecret\x0c".as_slice(),
                Some(b"\x0csecret\x0c".as_slice()),
            ),
            (
                b"\xc2\xa0secret\xc2\xa0".as_slice(),
                Some(b"\xc2\xa0secret\xc2\xa0".as_slice()),
            ),
        ] {
            let actual = parameters.secret_parameter("client_secret", Some(raw))?;
            assert_eq!(actual.as_ref().map(|secret| secret.as_slice()), expected);
        }
        Ok(())
    }

    #[test]
    fn non_scalar_body_secret_never_falls_back_to_basic_bytes() -> Result<(), serde_json::Error> {
        for value in [
            json!(["private-body-material"]),
            json!({"private-body-material":true}),
        ] {
            let parameters =
                OAuthParameters::new(Map::from_iter([("client_secret".to_owned(), value)]));
            let result = parameters.secret_parameter("client_secret", Some(b"basic-default"));
            assert!(result.is_err());
            if let Err(error) = result {
                let body = serde_json::to_value(error)?;
                assert_eq!(body["error"], "invalid_request");
                assert!(!body.to_string().contains("private-body-material"));
            }
        }
        Ok(())
    }
}
