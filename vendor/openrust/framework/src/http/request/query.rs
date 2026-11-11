use serde_json::Value;

use crate::http::HttpError;

use super::variables::VariableBag;

const MAX_QUERY_BYTES: usize = 16 * 1024;
const MAX_FIELDS: usize = 1024;

pub(super) fn parse(raw: &str) -> Result<Value, HttpError> {
    if raw.len() > MAX_QUERY_BYTES {
        return Err(HttpError::PayloadTooLarge);
    }
    parse_form(raw.split('\0').next().unwrap_or_default())
}

pub(super) fn parse_form(raw: &str) -> Result<Value, HttpError> {
    let mut bag = VariableBag::default();
    for (count, pair) in raw.split('&').filter(|pair| !pair.is_empty()).enumerate() {
        if count >= MAX_FIELDS {
            return Err(HttpError::PayloadTooLarge);
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        bag.insert(&decode(key)?, decode(value)?)?;
    }
    Ok(bag.into_value())
}

fn decode(value: &str) -> Result<String, HttpError> {
    let value = value.replace('+', " ");
    percent_encoding::percent_decode_str(&value)
        .decode_utf8()
        .map(std::borrow::Cow::into_owned)
        .map_err(|_| HttpError::InvalidRequest)
}
