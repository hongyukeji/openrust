//! Cookie wire 使用 PHP SAPI 解码边界；变量注册与 Query/POST 共用同一树。
use crate::support::php_variables::{Registration, VariableBag};
use http::{HeaderMap, HeaderValue, header};
use serde_json::Value;

const MAX_BYTES: usize = 64 * 1024;
const MAX_HEADER_VALUES: usize = 128;
const MAX_FIELDS: usize = 1024;

#[derive(Clone)]
pub(crate) struct OriginalCookies {
    pub(crate) headers: Vec<HeaderValue>,
    pub(crate) root_names: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct ParsedCookies {
    pub(crate) value: Value,
    pub(crate) original: OriginalCookies,
}

pub(crate) fn parse_headers(headers: &HeaderMap) -> crate::Result<ParsedCookies> {
    let mut root = VariableBag::default();
    let mut total_bytes = 0usize;
    let mut fields = 0usize;
    let mut original = OriginalCookies {
        headers: Vec::new(),
        root_names: Vec::new(),
    };
    for wire in headers.get_all(header::COOKIE) {
        total_bytes = total_bytes
            .checked_add(wire.as_bytes().len())
            .ok_or_else(super::cookie_limits_error)?;
        if total_bytes > MAX_BYTES || original.headers.len() >= MAX_HEADER_VALUES {
            return Err(super::cookie_limits_error());
        }
        let wire_text = std::str::from_utf8(wire.as_bytes()).map_err(|_| encoding_error())?;
        original.headers.push(wire.clone());
        // PHP cookie_data 是 C 字符串；百分号解码发生在这个边界之后。
        for pair in wire_text.split('\0').next().unwrap_or_default().split(';') {
            let pair =
                pair.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '\u{000b}');
            if pair.is_empty() || pair.starts_with('=') {
                continue;
            }
            fields = fields
                .checked_add(1)
                .ok_or_else(super::cookie_limits_error)?;
            if fields > MAX_FIELDS {
                return Err(super::cookie_limits_error());
            }
            let (name, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
            let value = decode_value(raw_value)?;
            let registered = root
                .insert(name, value, Registration::Cookie)
                .map_err(|error| {
                    if error.code() == "input_limits_exceeded" {
                        super::cookie_limits_error()
                    } else {
                        error
                    }
                })?;
            if let Some(name) = registered {
                original.root_names.push(name);
            }
        }
    }
    let value = root.into_value();
    crate::support::data::validate_limits(&value).map_err(|_| super::cookie_limits_error())?;
    Ok(ParsedCookies { value, original })
}

// rawurldecode 保留加号和无法组成完整 %HH 的字节，不使用 form_urlencoded。
fn decode_value(raw: &str) -> crate::Result<String> {
    let input = raw.as_bytes();
    let mut output = Vec::with_capacity(input.len());
    let mut position = 0;
    while position < input.len() {
        if input[position] == b'%'
            && position + 2 < input.len()
            && let (Some(high), Some(low)) = (hex(input[position + 1]), hex(input[position + 2]))
        {
            output.push(high * 16 + low);
            position += 3;
        } else {
            output.push(input[position]);
            position += 1;
        }
    }
    String::from_utf8(output).map_err(|_| encoding_error())
}
fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
fn encoding_error() -> crate::Error {
    crate::Error::custom_with_status(
        http::StatusCode::BAD_REQUEST,
        "input_cookie_encoding_invalid",
        "Cookie 必须使用有效 UTF-8",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matches_real_sapi_wire_and_classifies_transport_and_utf8() -> crate::Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/http/fixtures/php85/cookie-wire-golden.json"
        ))
        .map_err(crate::Error::internal)?;
        let mut counts = [0; 3];
        for case in fixture["cases"].as_array().into_iter().flatten() {
            let mut headers = HeaderMap::new();
            for wire in case["cookie_headers_latin1"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let raw = wire.as_str().unwrap_or_default();
                headers.append(
                    header::COOKIE,
                    HeaderValue::from_bytes(raw.as_bytes()).map_err(crate::Error::internal)?,
                );
            }
            match case["id"].as_str().unwrap_or_default() {
                "value_invalid_utf8" => {
                    let error = parse_headers(&headers)
                        .err()
                        .ok_or_else(|| crate::Error::custom("test", "应拒绝非 UTF-8"))?;
                    assert_eq!(error.code(), "input_cookie_encoding_invalid");
                    assert_eq!(error.status(), http::StatusCode::BAD_REQUEST);
                    counts[1] += 1;
                }
                "duplicate_headers" => {
                    let parsed = parse_headers(&headers)?;
                    assert_eq!(parsed.value, json!({"duph":"first"}));
                    assert_eq!(case["result"], json!({"duph":"first, duph=second"}));
                    assert_eq!(parsed.original.root_names, ["duph", "duph"]);
                    counts[2] += 1;
                }
                _ => {
                    assert_eq!(
                        parse_headers(&headers)?.value,
                        case["result"],
                        "{}",
                        case["id"]
                    );
                    counts[0] += 1;
                }
            }
        }
        assert_eq!(counts, [20, 1, 1]);
        Ok(())
    }

    #[test]
    fn wire_limits_reject_instead_of_truncating() -> crate::Result<()> {
        for raw in [
            format!("a={}", "x".repeat(MAX_BYTES)),
            (0..1025)
                .map(|n| format!("k{n}=x"))
                .collect::<Vec<_>>()
                .join(";"),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::COOKIE,
                HeaderValue::from_str(&raw).map_err(crate::Error::internal)?,
            );
            assert_eq!(
                parse_headers(&headers)
                    .err()
                    .map(|error| error.code().to_owned()),
                Some("input_cookie_limits_exceeded".to_owned())
            );
        }
        Ok(())
    }
}
