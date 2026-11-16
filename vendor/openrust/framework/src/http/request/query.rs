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
    parse_pairs(raw, false)
}

pub(super) fn parse_form(raw: &str) -> Result<Value, HttpError> {
    parse_pairs(raw, true)
}

fn parse_pairs(raw: &str, form: bool) -> Result<Value, HttpError> {
    let mut bag = VariableBag::default();
    for (count, pair) in raw.split('&').filter(|pair| !pair.is_empty()).enumerate() {
        if count >= MAX_FIELDS {
            return Err(HttpError::PayloadTooLarge);
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = decode(key, form)?;
        let value = decode(value, form)?;
        if form {
            bag.insert(&key, value)?;
        } else {
            bag.insert_uri(&key, value)?;
        }
    }
    Ok(bag.into_value())
}

fn decode(value: &str, form: bool) -> Result<String, HttpError> {
    let form_value;
    let value = if form {
        form_value = value.replace('+', " ");
        &form_value
    } else {
        value
    };
    percent_encoding::percent_decode_str(value)
        .decode_utf8()
        .map(std::borrow::Cow::into_owned)
        .map_err(|_| HttpError::InvalidRequest)
}
