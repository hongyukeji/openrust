use serde_json::Value;

use crate::{
    mail::{Mail, PreparedMail, Recipients},
    notifications::{
        Notifiable, Notification, NotificationError, NotificationFuture, NotificationResult,
        NotificationRoute, delivery, messages::MailDelivery,
    },
};

use super::{Channel, ChannelResponse};

/// Delivers notifications using the current application's mail manager.
pub struct MailChannel;

impl MailChannel {
    async fn prepare(
        recipient: &dyn Notifiable,
        notification: &dyn Notification,
    ) -> NotificationResult<Option<PreparedMail>> {
        let Some(message) = notification.to_mail(recipient)? else {
            return Ok(None);
        };
        let (mailable, recipients): (Box<dyn crate::mail::Mailable>, _) = match message {
            MailDelivery::Message(message) => {
                let Some(NotificationRoute::Mail(addresses)) =
                    recipient.route_notification_for("mail", notification)?
                else {
                    return Ok(None);
                };
                if addresses.is_empty() {
                    return Ok(None);
                }
                (message, Recipients::to(addresses))
            }
            MailDelivery::Mailable(mailable) => (mailable, Recipients::default()),
        };
        let mailer = Mail::mailer(mailable.mailer()).map_err(NotificationError::Mail)?;
        PreparedMail::prepare(&mailer.name, mailable.as_ref(), recipients)
            .await
            .map(Some)
            .map_err(NotificationError::Mail)
    }
}

impl Channel for MailChannel {
    fn send<'a>(
        &'a self,
        recipient: &'a dyn Notifiable,
        notification: &'a dyn Notification,
    ) -> NotificationFuture<'a, NotificationResult<Option<ChannelResponse>>> {
        Box::pin(async move {
            let Some(message) = Self::prepare(recipient, notification).await? else {
                return Ok(None);
            };
            let sent = message.send().await.map_err(NotificationError::Mail)?;
            Ok(Some(ChannelResponse::Mail(sent)))
        })
    }

    fn queue_payload<'a>(
        &'a self,
        recipient: &'a dyn Notifiable,
        notification: &'a dyn Notification,
    ) -> NotificationFuture<'a, NotificationResult<Option<Value>>> {
        Box::pin(async move {
            Self::prepare(recipient, notification)
                .await?
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
            let message: PreparedMail = serde_json::from_value(payload).map_err(delivery)?;
            let sent = message.send().await.map_err(NotificationError::Mail)?;
            Ok(Some(ChannelResponse::Mail(sent)))
        })
    }
}
