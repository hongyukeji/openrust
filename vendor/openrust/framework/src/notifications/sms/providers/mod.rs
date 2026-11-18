mod aliyun;
mod tencent;
mod twilio;

use std::{fmt::Write as _, sync::Arc, time::Duration};

use aws_lc_rs::{digest, hmac};
use http::{HeaderMap, HeaderValue, header::AUTHORIZATION};
use serde_json::Value;
use url::Url;

use crate::{
    http::client::{PendingRequest, Response},
    notifications::{NotificationResult, channels::secure_endpoint, delivery},
    support::facades::Http,
};

use super::SmsManager;

pub(super) fn register(manager: &SmsManager) {
    manager.extend("aliyun", |profile| {
        Ok(Arc::new(aliyun::Aliyun::new(profile)?))
    });
    manager.extend("aliyun_international", |profile| {
        Ok(Arc::new(aliyun::AliyunInternational::new(profile)?))
    });
    manager.extend("tencent", |profile| {
        Ok(Arc::new(tencent::Tencent::new(profile)?))
    });
    manager.extend("twilio", |profile| {
        Ok(Arc::new(twilio::Twilio::new(profile)?))
    });
}

fn setting(profile: &Value, field: &str) -> Option<String> {
    profile[field]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

fn required(profile: &Value, field: &str) -> NotificationResult<String> {
    setting(profile, field).ok_or_else(|| delivery(format!("The SMS profile requires {field}.")))
}

/// A profile may restrict delivery to a market and interpret local Mainland numbers.
enum Market {
    Any,
    Mainland,
    International,
}

impl Market {
    fn from_profile(profile: &Value) -> NotificationResult<Self> {
        match profile.get("market").filter(|value| !value.is_null()) {
            None => Ok(Self::Any),
            Some(value) if value.as_str() == Some("mainland") => Ok(Self::Mainland),
            Some(value) if value.as_str() == Some("international") => Ok(Self::International),
            Some(_) => Err(delivery(
                "The SMS market must be mainland or international.",
            )),
        }
    }

    fn number(&self, number: &str) -> NotificationResult<String> {
        let number = number.trim();
        let digits = super::international_number(number)?;
        let normalized = if matches!(self, Self::Mainland)
            && !number.starts_with('+')
            && digits.len() == 11
            && digits.starts_with('1')
        {
            format!("86{digits}")
        } else if number.starts_with('+') {
            digits.to_owned()
        } else {
            return Err(delivery(
                "Enter the SMS number with its + country calling code.",
            ));
        };
        let mainland = normalized.starts_with("86");
        if mainland && (normalized.len() != 13 || !normalized.starts_with("861")) {
            return Err(delivery(
                "The Mainland China SMS number must be +86 followed by an 11-digit mobile number.",
            ));
        }
        match self {
            Self::Mainland if !mainland => Err(delivery(
                "The SMS profile accepts Mainland China numbers only.",
            )),
            Self::International if mainland => Err(delivery(
                "The SMS profile accepts international and Hong Kong, Macao, and Taiwan numbers only.",
            )),
            _ => Ok(normalized),
        }
    }
}

fn endpoint(profile: &Value, default: &str) -> NotificationResult<Url> {
    let value = match profile.get("endpoint").filter(|value| !value.is_null()) {
        Some(value) => value
            .as_str()
            .ok_or_else(|| delivery("The SMS endpoint must be a URL."))?,
        None => default,
    };
    if !secure_endpoint(value).unwrap_or(false) {
        return Err(delivery(
            "The SMS endpoint must use HTTPS or loopback HTTP without credentials, a query, or a fragment.",
        ));
    }
    Url::parse(value).map_err(|_| delivery("The SMS endpoint is invalid."))
}

fn rpc_endpoint(profile: &Value, default: &str) -> NotificationResult<Url> {
    let url = endpoint(profile, default)?;
    if url.path() != "/" {
        return Err(delivery("The SMS API endpoint must use the root path."));
    }
    Ok(url)
}

fn request() -> NotificationResult<PendingRequest> {
    Ok(Http::acceptJson()
        .map_err(|_| delivery("The SMS HTTP client is unavailable."))?
        .withoutRedirecting()
        .timeout(Duration::from_secs(10))
        .maxResponseBytes(64 * 1024))
}

fn response(result: crate::http::client::Result<Response>) -> NotificationResult<Value> {
    let response = result
        .map_err(|_| delivery("The SMS request failed; submission status may be unknown."))?;
    if !response.successful() {
        return Err(delivery(format!(
            "The SMS provider returned HTTP {}.",
            response.status().as_u16()
        )));
    }
    response
        .json()
        .map_err(|_| delivery("The SMS provider returned an invalid JSON response."))
}

fn signed_headers(
    values: &[(&'static str, &str)],
    authorization: &str,
) -> NotificationResult<HeaderMap> {
    let mut headers = HeaderMap::new();
    let parse = |value: &str| {
        HeaderValue::from_str(value)
            .map_err(|_| delivery("The SMS profile contains an invalid request header."))
    };
    for (name, value) in values {
        headers.insert(*name, parse(value)?);
    }
    let mut value = parse(authorization)?;
    value.set_sensitive(true);
    headers.insert(AUTHORIZATION, value);
    Ok(headers)
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing to String");
    }
    output
}

fn sha256(value: &[u8]) -> String {
    hex(digest::digest(&digest::SHA256, value).as_ref())
}

fn sign(key: &[u8], value: &str) -> hmac::Tag {
    hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), value.as_bytes())
}

fn now() -> chrono::DateTime<chrono::Utc> {
    std::time::SystemTime::now().into()
}
