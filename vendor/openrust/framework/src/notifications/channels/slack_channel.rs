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

/// Posts Slack notifications through the Bot API.
pub struct SlackChannel;

#[derive(Deserialize, Serialize)]
struct QueuedSlackMessage {
    payload: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    encrypted_token: Option<String>,
}

struct PreparedSlack {
    payload: Map<String, Value>,
    token: Option<String>,
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
        token: route.token,
    }))
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
            let Some(PreparedSlack { payload, token }) = prepare(recipient, notification)? else {
                return Ok(None);
            };
            send_message(payload, token).await
        })
    }

    fn queue_payload<'a>(
        &'a self,
        recipient: &'a dyn Notifiable,
        notification: &'a dyn Notification,
    ) -> NotificationFuture<'a, NotificationResult<Option<Value>>> {
        Box::pin(async move {
            let Some(PreparedSlack { payload, token }) = prepare(recipient, notification)? else {
                return Ok(None);
            };
            let encrypted_token = token.map(Crypt::encryptString).transpose().map_err(|_| {
                delivery("An application key is required to queue a Slack workspace token.")
            })?;
            serde_json::to_value(QueuedSlackMessage {
                payload,
                encrypted_token,
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
            let token = queued
                .encrypted_token
                .as_deref()
                .map(|token| {
                    Crypt::decryptString(token)
                        .map_err(|_| {
                            delivery("The queued Slack workspace token could not be decrypted.")
                        })
                        .and_then(|token| {
                            String::from_utf8(token).map_err(|_| {
                                delivery("The queued Slack workspace token is invalid.")
                            })
                        })
                })
                .transpose()?;
            send_message(queued.payload, token).await
        })
    }
}
