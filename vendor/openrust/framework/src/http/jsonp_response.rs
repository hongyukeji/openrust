use std::sync::LazyLock;

use http::{HeaderValue, header::CONTENT_TYPE};
use serde::Serialize;

use super::json_formatter::JsonFormatter;

use super::{Body, HttpError, Response, StatusCode};

impl Response {
    /// Wraps JSON in a validated JavaScript callback for a JSONP response.
    pub fn jsonp(
        status: StatusCode,
        callback: Option<&str>,
        data: impl Serialize,
    ) -> Result<Self, HttpError> {
        let Some(callback) = callback else {
            return Self::json(status, data);
        };
        if !valid_callback(callback) {
            return Err(HttpError::InvalidRequest);
        }
        let bytes = encode(callback, data)?;
        if !Self::allows_body(status) {
            return Ok(Self::empty(status));
        }
        Ok(Self::new(status, Body::from(bytes)).header(
            CONTENT_TYPE,
            HeaderValue::from_static("text/javascript; charset=utf-8"),
        ))
    }
}

fn valid_callback(callback: &str) -> bool {
    static PART: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(concat!(
            r#"^[$_\p{L}][$_\p{L}\p{Mn}\p{Mc}\p{Nd}\p{Pc}\u{200c}\u{200d}]*"#,
            r#"(?:\[(?:[0-9]+|"(?:\\[^\x00-\x1f]|[^"\\])*"|'(?:\\[^\x00-\x1f]|[^'\\])*')\])*\z"#,
        ))
        .expect("valid JSONP callback pattern")
    });
    const RESERVED: &[&str] = &[
        "break",
        "do",
        "instanceof",
        "typeof",
        "case",
        "else",
        "new",
        "var",
        "catch",
        "finally",
        "return",
        "void",
        "continue",
        "for",
        "switch",
        "while",
        "debugger",
        "function",
        "this",
        "with",
        "default",
        "if",
        "throw",
        "delete",
        "in",
        "try",
        "class",
        "enum",
        "extends",
        "super",
        "const",
        "export",
        "import",
        "implements",
        "let",
        "private",
        "public",
        "yield",
        "interface",
        "package",
        "protected",
        "static",
        "null",
        "true",
        "false",
    ];
    !callback
        .chars()
        .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
        && callback
            .split('.')
            .all(|part| !RESERVED.contains(&part) && PART.is_match(part))
}

fn encode(callback: &str, data: impl Serialize) -> Result<Vec<u8>, HttpError> {
    let mut output = Vec::from(b"/**/".as_slice());
    output.extend_from_slice(callback.as_bytes());
    output.push(b'(');
    JsonFormatter::DEFAULT.write(&mut output, &data)?;
    output.extend_from_slice(b");");
    Ok(output)
}
