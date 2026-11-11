use serde_json::Value;

use super::{Channel, ChannelResponse};
use crate::notifications::{
    DatabaseNotification, Notifiable, Notification, NotificationFuture, NotificationResult,
    NotificationRoute, delivery, new_id,
};

/// Stores database notifications through the recipient's model relation.
pub struct DatabaseChannel;

impl DatabaseChannel {
    fn prepare(
        recipient: &dyn Notifiable,
        notification: &dyn Notification,
    ) -> NotificationResult<Option<DatabaseNotification>> {
        let Some(NotificationRoute::Database(route)) =
            recipient.route_notification_for("database", notification)?
        else {
            return Ok(None);
        };
        let message = match notification.to_database(recipient)? {
            Some(message) => Some(message),
            None => notification.to_array(recipient)?,
        }
        .ok_or_else(|| delivery("Database notifications require to_database or to_array."))?;
        Ok(Some(DatabaseNotification {
            id: match notification.id() {
                Some(id) => id.into(),
                None => new_id()?,
            },
            kind: message.kind.unwrap_or_else(|| notification.kind().into()),
            route,
            data: message.data,
            read_at: message.read_at,
        }))
    }
}

impl Channel for DatabaseChannel {
    fn send<'a>(
        &'a self,
        recipient: &'a dyn Notifiable,
        notification: &'a dyn Notification,
    ) -> NotificationFuture<'a, NotificationResult<Option<ChannelResponse>>> {
        Box::pin(async move {
            let Some(message) = Self::prepare(recipient, notification)? else {
                return Ok(None);
            };
            message.insert().await?;
            Ok(Some(ChannelResponse::Database(message)))
        })
    }

    fn queue_payload<'a>(
        &'a self,
        recipient: &'a dyn Notifiable,
        notification: &'a dyn Notification,
    ) -> NotificationFuture<'a, NotificationResult<Option<Value>>> {
        Box::pin(async move {
            Self::prepare(recipient, notification)?
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
            let message: DatabaseNotification =
                serde_json::from_value(payload).map_err(delivery)?;
            message.insert().await?;
            Ok(Some(ChannelResponse::Database(message)))
        })
    }
}
