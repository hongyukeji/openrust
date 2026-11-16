use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    broadcasting::{BroadcastChannel as Subscription, BroadcastPayload, PrivateChannel},
    contracts::{broadcasting::ShouldBroadcast, events::Event},
    support::facades::Event as Events,
};

use super::{Channel, ChannelResponse};
use crate::notifications::{
    Notifiable, Notification, NotificationFuture, NotificationResult, NotificationRoute, delivery,
    messages::BroadcastMessage, new_id,
};

/// Queues an event on the application's configured broadcast connection.
pub struct BroadcastChannel;

/// The event dispatched when a notification enters the broadcast channel.
#[derive(Deserialize, Serialize)]
pub struct BroadcastNotificationCreated {
    pub channels: Vec<Subscription>,
    pub data: BroadcastPayload,
    pub name: Option<String>,
    pub connection: Option<String>,
    pub queue: Option<String>,
}

impl Event for BroadcastNotificationCreated {
    fn name(&self) -> &str {
        "notifications.broadcast_notification_created"
    }
}

impl ShouldBroadcast for BroadcastNotificationCreated {
    fn broadcastOn(&self) -> Vec<Subscription> {
        self.channels.clone()
    }

    fn broadcastAs(&self) -> Option<&str> {
        Some(
            self.name
                .as_deref()
                .unwrap_or(r"Illuminate\Notifications\Events\BroadcastNotificationCreated"),
        )
    }

    fn broadcastData(&self) -> BroadcastPayload {
        self.data.clone()
    }

    fn broadcastQueueConnection(&self) -> Option<&str> {
        self.connection.as_deref()
    }

    fn broadcastQueue(&self) -> Option<&str> {
        self.queue.as_deref()
    }
}

impl BroadcastChannel {
    fn prepare(
        recipient: &dyn Notifiable,
        notification: &dyn Notification,
    ) -> NotificationResult<Option<BroadcastNotificationCreated>> {
        let routed = recipient.route_notification_for("broadcast", notification)?;
        let mut channels = match routed {
            Some(NotificationRoute::Broadcast(channels)) if recipient.as_anonymous().is_some() => {
                channels
            }
            _ => notification.broadcast_on(),
        };
        if channels.is_empty() {
            channels = recipient
                .receives_broadcast_notifications_on(notification)?
                .into_iter()
                .map(|name| Subscription::Private(PrivateChannel { name }))
                .collect();
        }
        if channels.is_empty() {
            return Ok(None);
        }
        let message = match notification.to_broadcast(recipient)? {
            Some(message) => message,
            None => {
                let fallback = notification.to_array(recipient)?.ok_or_else(|| {
                    delivery("Broadcast notifications require to_broadcast or to_array.")
                })?;
                let data = fallback.data.as_object().cloned().ok_or_else(|| {
                    delivery("Broadcast notification data must be a JSON object.")
                })?;
                BroadcastMessage::new(data)
            }
        };
        let data = match notification.broadcast_with() {
            Some(data) => data,
            None => {
                let mut data = message.data;
                let id = match notification.id() {
                    Some(id) => id.to_owned(),
                    None => new_id()?,
                };
                data.insert("id".into(), Value::String(id));
                data.insert(
                    "type".into(),
                    Value::String(notification.broadcast_type().into()),
                );
                data
            }
        };
        Ok(Some(BroadcastNotificationCreated {
            channels,
            data,
            name: notification.broadcast_as().map(str::to_owned),
            connection: message.connection,
            queue: message.queue,
        }))
    }

    async fn dispatch(
        event: BroadcastNotificationCreated,
    ) -> NotificationResult<Option<ChannelResponse>> {
        Events::dispatch(event).await.map_err(delivery)?;
        Ok(None)
    }
}

impl Channel for BroadcastChannel {
    fn send<'a>(
        &'a self,
        recipient: &'a dyn Notifiable,
        notification: &'a dyn Notification,
    ) -> NotificationFuture<'a, NotificationResult<Option<ChannelResponse>>> {
        Box::pin(async move {
            match Self::prepare(recipient, notification)? {
                Some(event) => Self::dispatch(event).await,
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
        Box::pin(
            async move { Self::dispatch(serde_json::from_value(payload).map_err(delivery)?).await },
        )
    }
}
