mod aliyun;
mod log;
mod smsbao;
mod tencent;
mod twilio;
mod vonage;
mod yunpian;

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

use super::{SmsDriver, SmsFailure, SmsFailureKind, SmsManager, SmsMessage};

pub(super) fn register(manager: &SmsManager) {
    manager.extend("log", |_| Ok(Arc::new(log::LogDriver)));
    manager.extend("vonage", |profile| {
        Ok(Arc::new(vonage::Vonage::new(profile)?))
    });
    register_routed(manager, "aliyun", |profile, market| match market {
        Market::Mainland => Ok(Box::new(aliyun::Aliyun::new(profile)?)),
        _ => Ok(Box::new(aliyun::AliyunInternational::new(profile)?)),
    });
    register_routed(manager, "smsbao", |profile, market| {
        Ok(Box::new(smsbao::SmsBao::new(profile, market)?))
    });
    register_routed(manager, "yunpian", |profile, market| {
        Ok(Box::new(yunpian::Yunpian::new(profile, market)?))
    });
    register_routed(manager, "tencent", |profile, market| {
        Ok(Box::new(tencent::Tencent::new(profile, market)?))
    });
    register_routed(manager, "twilio", |profile, market| {
        Ok(Box::new(twilio::Twilio::new(profile, market)?))
    });
}

type MarketFactory = fn(&Value, Market) -> NotificationResult<Box<dyn SmsDriver>>;

fn register_routed(manager: &SmsManager, name: &'static str, factory: MarketFactory) {
    manager.extend(name, move |profile| {
        Ok(Arc::new(RoutedDriver {
            restriction: Market::from_profile(profile)?,
            profile: profile.clone(),
            factory,
        }))
    });
}

struct RoutedDriver {
    restriction: Market,
    profile: Value,
    factory: MarketFactory,
}

impl SmsDriver for RoutedDriver {
    fn send(
        &self,
        message: SmsMessage,
    ) -> crate::notifications::NotificationFuture<'_, NotificationResult<Value>> {
        Box::pin(async move {
            let digits = self.restriction.number(message.to())?;
            let market = Market::from_digits(&digits);
            let mut profile = self.profile.clone();
            if let Some(settings) = self.profile["markets"].get(market.key()) {
                let settings = settings
                    .as_object()
                    .ok_or_else(|| delivery("The SMS market settings must be an object."))?;
                profile
                    .as_object_mut()
                    .expect("configured profile object")
                    .extend(settings.clone());
            }
            (self.factory)(&profile, market)?.send(message).await
        })
    }
}

pub(super) fn market_key(number: &str, profile: &Value) -> NotificationResult<&'static str> {
    let digits = Market::from_profile(profile)?.number(number)?;
    Ok(Market::from_digits(&digits).key())
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
            Some(value) if value.as_str() == Some("auto") => Ok(Self::Any),
            Some(value) if value.as_str() == Some("mainland") => Ok(Self::Mainland),
            Some(value) if value.as_str() == Some("international") => Ok(Self::International),
            Some(_) => Err(delivery(
                "The SMS market must be auto, mainland, or international.",
            )),
        }
    }

    fn key(&self) -> &'static str {
        match self {
            Self::Mainland => "mainland",
            Self::International => "international",
            Self::Any => "auto",
        }
    }

    fn from_digits(digits: &str) -> Self {
        if digits.starts_with("86") {
            Self::Mainland
        } else {
            Self::International
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
    fn provider_number(&self, number: &str) -> NotificationResult<String> {
        let digits = self.number(number)?;
        Ok(if matches!(self, Self::Mainland) {
            digits[2..].into()
        } else {
            format!("+{digits}")
        })
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
        .redact_payload()
        .withoutRedirecting()
        .timeout(Duration::from_secs(10))
        .maxResponseBytes(64 * 1024))
}

pub(in crate::notifications) fn checked_response(
    result: crate::http::client::Result<Response>,
) -> NotificationResult<Response> {
    let response =
        result.map_err(|_| unknown("The SMS request failed; submission status may be unknown."))?;
    if !response.successful() {
        let status = response.status();
        let kind = if status.as_u16() == 429 {
            SmsFailureKind::Retryable
        } else if status.is_client_error() && status.as_u16() != 408 {
            SmsFailureKind::Rejected
        } else {
            SmsFailureKind::SubmissionUnknown
        };
        return Err(delivery(SmsFailure::new(
            kind,
            Some(status.as_str()),
            "The SMS provider returned HTTP",
        )));
    }
    Ok(response)
}

pub(in crate::notifications) fn unknown(
    reason: &'static str,
) -> crate::notifications::NotificationError {
    delivery(SmsFailure::new(
        SmsFailureKind::SubmissionUnknown,
        None,
        reason,
    ))
}

fn rejected(reason: &'static str, code: Option<&str>) -> crate::notifications::NotificationError {
    delivery(SmsFailure::new(SmsFailureKind::Rejected, code, reason))
}

fn response(result: crate::http::client::Result<Response>) -> NotificationResult<Value> {
    checked_response(result)?
        .json()
        .map_err(|_| unknown("The SMS provider returned an invalid JSON response."))
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
