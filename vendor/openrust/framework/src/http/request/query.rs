use serde_json::Value;

use crate::http::HttpError;

use super::variables::VariableBag;

const MAX_QUERY_BYTES: usize = 16 * 1024;
const MAX_FIELDS: usize = 1024;

pub(crate) fn parse(raw: &str) -> Result<Value, HttpError> {
    if raw.len() > MAX_QUERY_BYTES {
        return Err(HttpError::PayloadTooLarge);
    }
    parse_form(raw.split('\0').next().unwrap_or_default())
}

pub(crate) fn parse_uri(raw: &str) -> Result<Value, HttpError> {
    if raw.len() > MAX_QUERY_BYTES {
        return Err(HttpError::PayloadTooLarge);
    }
    parse_pairs(raw, QueryMode::Uri)
}

pub(super) fn parse_form(raw: &str) -> Result<Value, HttpError> {
    parse_pairs(raw, QueryMode::Form)
}

pub(super) fn parse_canonical(raw: &str) -> Result<Value, HttpError> {
    if raw.len() > MAX_QUERY_BYTES {
        return Err(HttpError::PayloadTooLarge);
    }
    parse_pairs(raw, QueryMode::Canonical)
}

enum QueryMode {
    Form,
    Uri,
    Canonical,
}

fn parse_pairs(raw: &str, mode: QueryMode) -> Result<Value, HttpError> {
    let form_encoding = !matches!(mode, QueryMode::Uri);
    let mut bag = VariableBag::default();
    for (count, pair) in raw.split('&').filter(|pair| !pair.is_empty()).enumerate() {
        if count >= MAX_FIELDS {
            return Err(HttpError::PayloadTooLarge);
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = decode(key, form_encoding, matches!(mode, QueryMode::Canonical))?;
        let value = decode(value, form_encoding, false)?;
        match mode {
            QueryMode::Form => bag.insert(&key, value)?,
            QueryMode::Uri => bag.insert_uri(&key, value)?,
            QueryMode::Canonical => bag.insert_uri(key.trim_start_matches(' '), value)?,
        }
    }
    Ok(bag.into_value())
}

fn decode(value: &str, form: bool, truncate_at_nul: bool) -> Result<String, HttpError> {
    let form_value;
    let value = if form {
        form_value = value.replace('+', " ");
        &form_value
    } else {
        value
    };
    let decoded = percent_encoding::percent_decode_str(value);
    if truncate_at_nul {
        return String::from_utf8(decoded.take_while(|byte| *byte != 0).collect())
            .map_err(|_| HttpError::InvalidRequest);
    }
    decoded
        .decode_utf8()
        .map(std::borrow::Cow::into_owned)
        .map_err(|_| HttpError::InvalidRequest)
}
