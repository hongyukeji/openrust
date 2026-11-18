use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    notifications::{
        Notifiable, Notification, NotificationFuture, NotificationResult, NotificationRoute,
        delivery,
        messages::VonageMessage,
        sms::{SmsDriver, SmsMessage, international_number},
    },
    support::facades::{Config, Http},
};

use super::{Channel, ChannelResponse, secure_endpoint};

const ENDPOINT: &str = "https://rest.nexmo.com/sms/json";

/// Submits text and Unicode messages to the Vonage SMS API.
pub struct VonageSmsChannel;

#[derive(Deserialize, Serialize)]
struct PreparedSms {
    to: String,
    message: VonageMessage,
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

impl PreparedSms {
    fn prepare(
        recipient: &dyn Notifiable,
        notification: &dyn Notification,
    ) -> NotificationResult<Option<Self>> {
        let Some(NotificationRoute::Vonage(number)) =
            recipient.route_notification_for("vonage", notification)?
        else {
            return Ok(None);
        };
        if number.trim().is_empty() {
            return Ok(None);
        }
        let Some(message) = notification.to_vonage(recipient)? else {
            return Ok(None);
        };
        let to = international_number(&number)
            .map_err(|_| delivery("The Vonage recipient must be an international phone number."))?;
        if message.content.trim().is_empty() {
            return Err(delivery("The Vonage SMS content cannot be empty."));
        }
        Ok(Some(Self {
            to: to.into(),
            message,
        }))
    }

    async fn submit(self) -> NotificationResult<Value> {
        let settings = Config::get("vonage")
            .map_err(|_| delivery("Vonage configuration is unavailable."))?
            .unwrap_or_default();
        let client = match self.message.client.as_deref() {
            Some(name) => settings["clients"]
                .get(name)
                .ok_or_else(|| delivery("The Vonage client is not configured."))?,
            None => &settings,
        };
        let key = nonempty(client["api_key"].as_str())
            .ok_or_else(|| delivery("The Vonage API key is required."))?;
        let secret = nonempty(client["api_secret"].as_str())
            .ok_or_else(|| delivery("The Vonage API secret is required."))?;
        let from = nonempty(self.message.from.as_deref())
            .or_else(|| nonempty(settings["sms_from"].as_str()))
            .ok_or_else(|| delivery("The Vonage SMS sender is required."))?;
        let endpoint = client["endpoint"].as_str().unwrap_or(ENDPOINT);
        validate_endpoint(endpoint)?;
        let form = SmsForm {
            to: &self.to,
            from,
            text: self.message.content.trim(),
            encoding: if self.message.unicode {
                "unicode"
            } else {
                "text"
            },
            client_reference: nonempty(self.message.client_reference.as_deref()),
            callback: nonempty(self.message.status_callback.as_deref()),
        };
        let response = Http::asForm()
            .map_err(|_| delivery("The Vonage SMS HTTP client is unavailable."))?
            .withBasicAuth(key, secret)
            .acceptJson()
            .withoutRedirecting()
            .timeout(Duration::from_secs(10))
            .maxResponseBytes(64 * 1024)
            .post(endpoint, &form)
            .await
            .map_err(|_| {
                delivery("The Vonage SMS request failed; submission status may be unknown.")
            })?;
        if !response.successful() {
            return Err(delivery(format!(
                "The Vonage SMS API returned HTTP {}.",
                response.status().as_u16()
            )));
        }
        let payload: Value = response
            .json()
            .map_err(|_| delivery("The Vonage SMS API returned an invalid response."))?;
        let messages = payload["messages"]
            .as_array()
            .filter(|messages| !messages.is_empty())
            .ok_or_else(|| delivery("The Vonage SMS API returned no message results."))?;
        for entry in messages {
            let Some(status) = entry["status"]
                .as_str()
                .and_then(|status| status.parse::<u32>().ok())
            else {
                return Err(delivery(
                    "The Vonage SMS API returned an invalid message status.",
                ));
            };
            if status != 0 {
                return Err(delivery(format!(
                    "The Vonage SMS API rejected the message (status {status})."
                )));
            }
            if nonempty(entry["message-id"].as_str()).is_none() {
                return Err(delivery(
                    "The Vonage SMS API returned no message identifier.",
                ));
            }
        }
        Ok(payload)
    }
}

impl Channel for VonageSmsChannel {
    fn send<'a>(
        &'a self,
        recipient: &'a dyn Notifiable,
        notification: &'a dyn Notification,
    ) -> NotificationFuture<'a, NotificationResult<Option<ChannelResponse>>> {
        Box::pin(async move {
            match PreparedSms::prepare(recipient, notification)? {
                Some(message) => message
                    .submit()
                    .await
                    .map(|value| Some(ChannelResponse::Custom(value))),
                None => Ok(None),
            }
        })
    }

    fn queue_payload<'a>(
        &'a self,
        recipient: &'a dyn Notifiable,
        notification: &'a dyn Notification,
    ) -> NotificationFuture<'a, NotificationResult<Option<Value>>> {
        Box::pin(async move {
            PreparedSms::prepare(recipient, notification)?
                .map(serde_json::to_value)
                .transpose()
                .map_err(delivery)
        })
    }

    fn send_queued(
        &self,
        payload: Value,
    ) -> NotificationFuture<'_, NotificationResult<Option<ChannelResponse>>> {
        Box::pin(async move {
            let message: PreparedSms = serde_json::from_value(payload)
                .map_err(|_| delivery("The queued Vonage SMS is invalid."))?;
            message
                .submit()
                .await
                .map(|value| Some(ChannelResponse::Custom(value)))
        })
    }
}

pub(in crate::notifications) struct VonageDriver {
    client: Option<String>,
}

impl VonageDriver {
    pub(in crate::notifications) fn new(config: &Value) -> NotificationResult<Self> {
        let client = match config.get("client") {
            None => None,
            Some(Value::String(name)) if !name.trim().is_empty() => Some(name.clone()),
            Some(_) => {
                return Err(delivery(
                    "The SMS profile contains an invalid Vonage client.",
                ));
            }
        };
        Ok(Self { client })
    }
}

impl SmsDriver for VonageDriver {
    fn send(&self, message: SmsMessage) -> NotificationFuture<'_, NotificationResult<Value>> {
        Box::pin(async move {
            message.validate()?;
            let content = message.content();
            let mut vonage = VonageMessage::new().content(content);
            if !content.is_ascii() {
                vonage = vonage.unicode();
            }
            vonage.client.clone_from(&self.client);
            PreparedSms {
                to: international_number(message.to())?.to_owned(),
                message: vonage,
            }
            .submit()
            .await
        })
    }
}

fn validate_endpoint(endpoint: &str) -> NotificationResult<()> {
    if !secure_endpoint(endpoint).map_err(|_| delivery("The Vonage SMS endpoint is invalid."))? {
        return Err(delivery(
            "The Vonage SMS endpoint must use HTTPS or loopback HTTP.",
        ));
    }
    Ok(())
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
