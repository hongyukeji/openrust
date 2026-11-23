use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use url::Url;

use crate::{
    notifications::{NotificationFuture, NotificationResult, delivery},
    support::facades::Http,
};

use super::super::{SmsDriver, SmsFailure, SmsFailureKind, SmsMessage, international_number};

const ENDPOINT: &str = "https://rest.nexmo.com/sms/json";

/// One Vonage account resolved by the unified SMS manager.
pub(super) struct Vonage {
    key: String,
    secret: String,
    from: String,
    endpoint: Url,
    client_reference: Option<String>,
    status_callback: Option<String>,
}

#[derive(Serialize)]
struct SmsForm<'a> {
    to: &'a str,
    from: &'a str,
    text: &'a str,
    #[serde(rename = "type")]
    encoding: &'static str,
    #[serde(rename = "client-ref", skip_serializing_if = "Option::is_none")]
    client_reference: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    callback: Option<&'a str>,
}

impl Vonage {
    pub(super) fn new(profile: &Value) -> NotificationResult<Self> {
        Ok(Self {
            key: super::required(profile, "api_key")?,
            secret: super::required(profile, "api_secret")?,
            from: super::required(profile, "from")?,
            endpoint: super::endpoint(profile, ENDPOINT)?,
            client_reference: super::setting(profile, "client_reference"),
            status_callback: super::setting(profile, "status_callback"),
        })
    }

    async fn submit(&self, message: SmsMessage) -> NotificationResult<Value> {
        message.validate()?;
        if message.approved_template().is_some() {
            return Err(delivery("The Vonage SMS driver requires text content."));
        }
        if !message.to().trim().starts_with('+') {
            return Err(delivery(
                "Enter the SMS number with its + country calling code.",
            ));
        }
        let to = international_number(message.to())?;
        let form = SmsForm {
            to,
            from: &self.from,
            text: message.content().trim(),
            encoding: if !message.content().chars().all(is_gsm_character) {
                "unicode"
            } else {
                "text"
            },
            client_reference: self.client_reference.as_deref(),
            callback: self.status_callback.as_deref(),
        };
        let result = Http::as_form()
            .map_err(|_| delivery("The Vonage SMS HTTP client is unavailable."))?
            .redact_request_payload_only()
            .with_basic_auth(&self.key, &self.secret)
            .accept_json()
            .without_redirecting()
            .timeout(Duration::from_secs(10))
            .max_response_bytes(64 * 1024)
            .post(self.endpoint.as_str(), &form)
            .await;
        let response = super::checked_response(result)?;
        let payload: Value = response
            .json()
            .map_err(|_| super::unknown("The Vonage SMS API returned an invalid response."))?;
        let messages = payload["messages"]
            .as_array()
            .filter(|messages| !messages.is_empty())
            .ok_or_else(|| delivery("The Vonage SMS API returned no message results."))?;
        let statuses = messages
            .iter()
            .map(|entry| {
                entry["status"]
                    .as_str()
                    .and_then(|status| status.parse::<u32>().ok())
                    .ok_or_else(|| {
                        super::unknown("The Vonage SMS API returned an invalid message status.")
                    })
            })
            .collect::<NotificationResult<Vec<_>>>()?;
        if let Some(status) = statuses.iter().find(|status| **status != 0) {
            let kind = if statuses.iter().all(|status| *status == 1) {
                SmsFailureKind::Retryable
            } else if statuses.contains(&0) {
                SmsFailureKind::SubmissionUnknown
            } else {
                SmsFailureKind::Rejected
            };
            return Err(delivery(SmsFailure::new(
                kind,
                Some(&status.to_string()),
                "The Vonage SMS API rejected one or more message parts.",
            )));
        }
        if messages
            .iter()
            .any(|entry| nonempty(entry["message-id"].as_str()).is_none())
        {
            return Err(super::unknown(
                "The Vonage SMS API returned no message identifier.",
            ));
        }
        Ok(payload)
    }
}

impl SmsDriver for Vonage {
    fn send(&self, message: SmsMessage) -> NotificationFuture<'_, NotificationResult<Value>> {
        Box::pin(self.submit(message))
    }
}

fn is_gsm_character(character: char) -> bool {
    const BASIC: &str = "@£$¥èéùìòÇ\nØø\rÅåΔ_ΦΓΛΩΠΨΣΘΞÆæßÉ !\"#¤%&'()*+,-./0123456789:;<=>?¡ABCDEFGHIJKLMNOPQRSTUVWXYZÄÖÑÜ§¿abcdefghijklmnopqrstuvwxyzäöñüà";
    const EXTENDED: &str = "\x0c^{}\\[~]|€";
    BASIC.contains(character) || EXTENDED.contains(character)
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
