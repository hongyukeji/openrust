use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    notifications::{
        Notifiable, Notification, NotificationFuture, NotificationResult, NotificationRoute,
        delivery,
    },
    support::facades::{Config, Crypt, Http},
};

use super::{Channel, ChannelResponse, secure_endpoint};

const CONFIG: &str = "services.slack.notifications";
const ENDPOINT: &str = "https://slack.com/api/chat.postMessage";

/// Posts Slack notifications through the Bot API or an incoming webhook.
pub struct SlackChannel;

#[derive(Deserialize, Serialize)]
struct QueuedSlackMessage {
    payload: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    encrypted_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    encrypted_webhook: Option<String>,
}

struct PreparedSlack {
    payload: Map<String, Value>,
    destination: Destination,
}

enum Destination {
    Bot(Option<String>),
    Webhook(String),
}

impl PreparedSlack {
    async fn send(self) -> NotificationResult<Option<ChannelResponse>> {
        match self.destination {
            Destination::Bot(token) => send_message(self.payload, token).await,
            Destination::Webhook(url) => send_webhook(self.payload, &url).await,
        }
    }
}

fn prepare(
    recipient: &dyn Notifiable,
    notification: &dyn Notification,
) -> NotificationResult<Option<PreparedSlack>> {
    let route = match recipient.route_notification_for("slack", notification)? {
        Some(NotificationRoute::Slack(route)) => route,
        None => crate::notifications::slack::SlackRoute::make(None, None),
        Some(_) => return Err(delivery("The Slack notification route is invalid.")),
    };
    if let Some(url) = route.webhook {
        let payload = if let Some(message) = notification.to_legacy_slack(recipient)? {
            message.toArray()
        } else if let Some(message) = notification.to_slack(recipient)? {
            message.toArray()?
        } else {
            return Ok(None);
        };
        validate_webhook(&url)?;
        return Ok(Some(PreparedSlack {
            payload,
            destination: Destination::Webhook(url),
        }));
    }
    let Some(message) = notification.to_slack(recipient)? else {
        return Ok(None);
    };
    let mut payload = message.toArray()?;
    let mut channel = route.channel.or_else(|| {
        payload
            .remove("channel")
            .and_then(|channel| channel.as_str().map(str::to_owned))
    });
    if channel.is_none() {
        channel = Config::get(&format!("{CONFIG}.channel"))
            .map_err(|_| delivery("Slack notification configuration is unavailable."))?
            .and_then(|value| value.as_str().map(str::to_owned));
    }
    let channel = channel
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| delivery("The Slack notification channel is not set."))?;
    payload.insert("channel".into(), channel.into());
    Ok(Some(PreparedSlack {
        payload,
        destination: Destination::Bot(route.token),
    }))
}

fn validate_webhook(url: &str) -> NotificationResult<()> {
    if !secure_endpoint(url).unwrap_or(false) {
        return Err(delivery(
            "The Slack webhook must use HTTPS or loopback HTTP without URL credentials, query or fragment.",
        ));
    }
    Ok(())
}

async fn send_webhook(
    payload: Map<String, Value>,
    url: &str,
) -> NotificationResult<Option<ChannelResponse>> {
    validate_webhook(url)?;
    let response = Http::pendingRequest()
        .map_err(|_| delivery("The Slack webhook HTTP client is unavailable."))?
        .asJson()
        .withoutRedirecting()
        .timeout(Duration::from_secs(10))
        .maxResponseBytes(1024 * 1024)
        .post(url, &payload)
        .await
        .map_err(|_| {
            delivery("The Slack webhook request failed; delivery status may be unknown.")
        })?;
    if !response.successful() {
        return Err(delivery(format!(
            "The Slack webhook returned HTTP {}.",
            response.status().as_u16()
        )));
    }
    Ok(Some(ChannelResponse::Custom(Value::String(
        String::from_utf8_lossy(response.body()).into_owned(),
    ))))
}

fn decrypt(value: &str, message: &str) -> NotificationResult<String> {
    Crypt::decryptString(value)
        .ok()
        .and_then(|value| String::from_utf8(value).ok())
        .ok_or_else(|| delivery(message))
}

async fn send_message(
    payload: Map<String, Value>,
    token: Option<String>,
) -> NotificationResult<Option<ChannelResponse>> {
    let config = Config::get(CONFIG)
        .map_err(|_| delivery("Slack notification configuration is unavailable."))?
        .unwrap_or_default();
    let token = token
        .as_deref()
        .or_else(|| config["bot_user_oauth_token"].as_str())
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| delivery("The Slack Bot API token is not set."))?;
    let endpoint = config["endpoint"].as_str().unwrap_or(ENDPOINT);
    if !secure_endpoint(endpoint).unwrap_or(false) {
        return Err(delivery(
            "The Slack Bot API endpoint must use HTTPS or loopback HTTP.",
        ));
    }
    let response = Http::withToken(token)
        .map_err(|_| delivery("The Slack Bot API HTTP client is unavailable."))?
        .asJson()
        .acceptJson()
        .withoutRedirecting()
        .timeout(Duration::from_secs(10))
        .maxResponseBytes(1024 * 1024)
        .post(endpoint, &payload)
        .await
        .map_err(|_| {
            delivery("The Slack Bot API request failed; delivery status may be unknown.")
        })?;
    if !response.successful() {
        return Err(delivery(format!(
            "The Slack Bot API returned HTTP {}.",
            response.status().as_u16()
        )));
    }
    let payload: Value = response
        .json()
        .map_err(|_| delivery("The Slack Bot API returned an invalid response."))?;
    match payload["ok"].as_bool() {
        Some(true) => Ok(Some(ChannelResponse::Custom(payload))),
        Some(false) => Err(delivery("The Slack Bot API rejected the message.")),
        None => Err(delivery("The Slack Bot API response is missing a result.")),
    }
}

impl Channel for SlackChannel {
    fn send<'a>(
        &'a self,
        recipient: &'a dyn Notifiable,
        notification: &'a dyn Notification,
    ) -> NotificationFuture<'a, NotificationResult<Option<ChannelResponse>>> {
        Box::pin(async move {
            let Some(prepared) = prepare(recipient, notification)? else {
                return Ok(None);
            };
            prepared.send().await
        })
    }

    fn queue_payload<'a>(
        &'a self,
        recipient: &'a dyn Notifiable,
        notification: &'a dyn Notification,
    ) -> NotificationFuture<'a, NotificationResult<Option<Value>>> {
        Box::pin(async move {
            let Some(PreparedSlack {
                payload,
                destination,
            }) = prepare(recipient, notification)?
            else {
                return Ok(None);
            };
            let (encrypted_token, encrypted_webhook) = match destination {
                Destination::Bot(token) => (
                    token.map(Crypt::encryptString).transpose().map_err(|_| {
                        delivery("An application key is required to queue a Slack workspace token.")
                    })?,
                    None,
                ),
                Destination::Webhook(url) => (
                    None,
                    Some(Crypt::encryptString(url).map_err(|_| {
                        delivery("An application key is required to queue a Slack webhook URL.")
                    })?),
                ),
            };
            serde_json::to_value(QueuedSlackMessage {
                payload,
                encrypted_token,
                encrypted_webhook,
            })
            .map(Some)
            .map_err(delivery)
        })
    }

    fn send_queued(
        &self,
        payload: Value,
    ) -> NotificationFuture<'_, NotificationResult<Option<ChannelResponse>>> {
        Box::pin(async move {
            let queued: QueuedSlackMessage = serde_json::from_value(payload)
                .map_err(|_| delivery("The queued Slack message is invalid."))?;
            let destination = match (queued.encrypted_token, queued.encrypted_webhook) {
                (None, Some(url)) => Destination::Webhook(decrypt(
                    &url,
                    "The queued Slack webhook URL is invalid or could not be decrypted.",
                )?),
                (token, None) => Destination::Bot(
                    token.as_deref().map(|token| {
                        decrypt(token, "The queued Slack workspace token is invalid or could not be decrypted.")
                    }).transpose()?,
                ),
                (Some(_), Some(_)) => return Err(delivery("The queued Slack message is invalid.")),
            };
            PreparedSlack {
                payload: queued.payload,
                destination,
            }
            .send()
            .await
        })
    }
}
