use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::support::facades::Facade;

use super::{Channel, ChannelResponse};
use crate::notifications::{
    Notifiable, Notification, NotificationFuture, NotificationResult, NotificationRoute, delivery,
    sms::{Sms, SmsMessage},
};

/// Sends notifications through configured SMS profiles.
#[derive(Default)]
pub struct SmsChannel {
    profile: Option<String>,
}

impl SmsChannel {
    /// Pins this channel to a named profile instead of using the application default.
    pub fn using(profile: impl Into<String>) -> Self {
        Self {
            profile: Some(profile.into()),
        }
    }

    fn prepare(
        &self,
        recipient: &dyn Notifiable,
        notification: &dyn Notification,
    ) -> NotificationResult<Option<PreparedSms>> {
        let Some(NotificationRoute::Sms(to)) =
            recipient.route_notification_for("sms", notification)?
        else {
            return Ok(None);
        };
        let message = match notification.to_sms_template(recipient)? {
            Some(template) => SmsMessage::template(to, template)?,
            None => {
                let Some(content) = notification.to_sms(recipient)? else {
                    return Ok(None);
                };
                SmsMessage::new(to, content)?
            }
        };
        let profile = Sms::getFacadeRoot()
            .map_err(delivery)?
            .select_profile(self.profile.as_deref())?;
        Ok(Some(PreparedSms { profile, message }))
    }
}

#[derive(Deserialize, Serialize)]
struct PreparedSms {
    profile: String,
    message: SmsMessage,
}

impl PreparedSms {
    async fn send(self) -> NotificationResult<Option<ChannelResponse>> {
        self.message.validate()?;
        let response = Sms::driver(Some(&self.profile))?.send(self.message).await?;
        Ok(Some(ChannelResponse::Custom(response)))
    }
}

impl Channel for SmsChannel {
    fn send<'a>(
        &'a self,
        recipient: &'a dyn Notifiable,
        notification: &'a dyn Notification,
    ) -> NotificationFuture<'a, NotificationResult<Option<ChannelResponse>>> {
        Box::pin(async move {
            match self.prepare(recipient, notification)? {
                Some(message) => message.send().await,
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
            self.prepare(recipient, notification)?
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
                .map_err(|_| delivery("The queued SMS is invalid."))?;
            message.send().await
        })
    }
}
