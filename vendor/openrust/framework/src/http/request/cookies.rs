use ::http::header::COOKIE;
use percent_encoding::percent_decode_str;
use serde_json::Value;

use super::{
    super::{HeaderMap, HttpError},
    variables::VariableBag,
};

pub(super) fn parse(headers: &HeaderMap) -> Result<Value, HttpError> {
    let mut bag = VariableBag::default();
    let mut bytes = 0usize;
    let mut fields = 0usize;
    for (index, header) in headers.get_all(COOKIE).iter().enumerate() {
        bytes = bytes.saturating_add(header.as_bytes().len());
        if bytes > 64 * 1024 || index >= 128 {
            return Err(HttpError::PayloadTooLarge);
        }
        let wire = std::str::from_utf8(header.as_bytes()).map_err(|_| HttpError::InvalidRequest)?;
        for pair in wire.split(';').map(str::trim_ascii_start) {
            if pair.is_empty() || pair.starts_with('=') {
                continue;
            }
            fields += 1;
            if fields > 1024 {
                return Err(HttpError::PayloadTooLarge);
            }
            let (name, raw) = pair.split_once('=').unwrap_or((pair, ""));
            let value = percent_decode_str(raw)
                .decode_utf8()
                .map_err(|_| HttpError::InvalidRequest)?;
            bag.insert_cookie(name, value.into_owned())?;
        }
    }
    Ok(bag.into_value())
}
