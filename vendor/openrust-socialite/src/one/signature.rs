use crate::{Error, Result};
use base64::Engine as _;
use std::collections::BTreeMap;

pub(super) fn encode(value: &str) -> String {
    encode_bytes(value.as_bytes())
}
fn encode_bytes(value: &[u8]) -> String {
    value
        .iter()
        .copied()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}
fn normalize(value: &str) -> String {
    // League uses byte-preserving rawurldecode before rawurlencode.
    let mut decoded = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            decoded.push(byte);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    encode_bytes(&decoded)
}
/// League-compatible HMAC-SHA1 for Twitter endpoint/query parameters.
/// Secrets and signatures are deliberately excluded from error messages.
pub fn sign(
    method: &str,
    endpoint: &str,
    parameters: &BTreeMap<String, String>,
    consumer_secret: &str,
    token_secret: &str,
) -> Result<String> {
    let mut url = url::Url::parse(endpoint).map_err(|_| Error::Configuration("OAuth1 endpoint"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(Error::Configuration("OAuth1 endpoint"));
    }
    let (parts, body) = http::Request::builder()
        .uri(endpoint)
        .body(openrust::http::Body::empty())
        .map_err(|_| Error::Configuration("OAuth1 endpoint"))?
        .into_parts();
    let request = openrust::http::Request::from_parts(parts, body);
    let mut data = request
        .query(None)?
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    for (key, value) in parameters {
        data.insert(key.clone(), value.clone().into());
    }
    let normalized = data
        .into_iter()
        .map(|(k, v)| (normalize(&k), v))
        .collect::<BTreeMap<_, _>>();
    fn pairs(key: String, value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(fields) => {
                for (k, v) in fields {
                    pairs(format!("{key}[{}]", normalize(k)), v, out);
                }
            }
            serde_json::Value::Array(fields) => {
                for (i, v) in fields.iter().enumerate() {
                    pairs(format!("{key}[{i}]"), v, out);
                }
            }
            serde_json::Value::String(v) => out.push(encode(&format!("{key}={}", normalize(v)))),
            _ => {}
        }
    }
    let mut query = Vec::new();
    for (key, value) in normalized {
        pairs(key, &value, &mut query);
    }
    let query = query.join("%26");
    url.set_query(None);
    url.set_fragment(None);
    let base = format!("{}&{}&{}", encode(method), encode(url.as_str()), query);
    let key = zeroize::Zeroizing::new(format!(
        "{}&{}",
        encode(consumer_secret),
        encode(token_secret)
    ));
    let key = aws_lc_rs::hmac::Key::new(
        aws_lc_rs::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
        key.as_bytes(),
    );
    Ok(base64::engine::general_purpose::STANDARD
        .encode(aws_lc_rs::hmac::sign(&key, base.as_bytes()).as_ref()))
}
