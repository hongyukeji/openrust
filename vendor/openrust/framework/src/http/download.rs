use http::HeaderValue;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

use super::HttpError;

pub(crate) fn filename_header(disposition: &str, name: &str) -> Result<HeaderValue, HttpError> {
    if !matches!(disposition, "attachment" | "inline")
        || name.is_empty()
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
    {
        return Err(HttpError::InvalidRequest);
    }
    let mut fallback = deunicode::deunicode_with_tofu(name, "");
    fallback.retain(|character| !matches!(character, '%' | '/' | '\\') && !character.is_control());
    if fallback.is_empty() {
        fallback = "_".repeat(name.chars().count());
    }
    let extended = fallback != name;
    if !fallback
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"!#$&'*+-.^_`|~".contains(&byte))
    {
        fallback = format!("\"{}\"", fallback.replace('"', "\\\""));
    }
    let mut value = format!("{disposition}; filename={fallback}");
    if extended {
        use std::fmt::Write as _;
        const ENCODE: &percent_encoding::AsciiSet = &NON_ALPHANUMERIC
            .remove(b'-')
            .remove(b'_')
            .remove(b'.')
            .remove(b'~');
        write!(
            value,
            "; filename*=utf-8''{}",
            utf8_percent_encode(name, ENCODE)
        )
        .expect("writing to String");
    }
    HeaderValue::from_str(&value).map_err(|_| HttpError::InvalidRequest)
}
