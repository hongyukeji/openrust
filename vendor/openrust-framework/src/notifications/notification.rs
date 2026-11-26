use serde::{Deserialize, Serialize};

use crate::{
    broadcasting::{BroadcastChannel, BroadcastPayload},
    queue::{DispatchOptions, JobOptions},
};

use super::{
    Notifiable, NotificationResult,
    messages::{BroadcastMessage, DatabaseMessage, MailDelivery},
    slack::SlackMessage,
};

/// The registered worker decision and its serializable context.
#[derive(Deserialize, Serialize)]
pub struct NotificationQueueGuard {
    pub name: String,
    pub data: serde_json::Value,
}

/// Describes the channels and content for a recipient.
pub trait Notification: Send + Sync {
    /// 返回通知的具体类型，用于队列观测。
    fn queue_subject_type(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    fn via(&self, recipient: &dyn Notifiable) -> NotificationResult<Vec<String>>;

    fn to_mail(&self, _recipient: &dyn Notifiable) -> NotificationResult<Option<MailDelivery>> {
        Ok(None)
    }

    /// Builds the SMS delivered by the configured channel to the routed number.
    fn to_sms(
        &self,
        _recipient: &dyn Notifiable,
        _to: String,
    ) -> NotificationResult<Option<super::sms::SmsMessage>> {
        Ok(None)
    }

    fn to_slack(&self, _recipient: &dyn Notifiable) -> NotificationResult<Option<SlackMessage>> {
        Ok(None)
    }

    /// Provides a legacy attachment message for a Slack webhook route.
    fn to_legacy_slack(
        &self,
        _recipient: &dyn Notifiable,
    ) -> NotificationResult<Option<super::messages::SlackMessage>> {
        Ok(None)
    }

    fn to_database(
        &self,
        _recipient: &dyn Notifiable,
    ) -> NotificationResult<Option<DatabaseMessage>> {
        Ok(None)
    }

    fn to_array(&self, _recipient: &dyn Notifiable) -> NotificationResult<Option<DatabaseMessage>> {
        Ok(None)
    }

    fn to_broadcast(
        &self,
        _recipient: &dyn Notifiable,
    ) -> NotificationResult<Option<BroadcastMessage>> {
        Ok(None)
    }
    fn broadcast_on(&self) -> Vec<BroadcastChannel> {
        Vec::new()
    }
    fn broadcast_with(&self) -> Option<BroadcastPayload> {
        None
    }
    fn broadcast_as(&self) -> Option<&str> {
        None
    }
    fn broadcast_type(&self) -> &str {
        self.kind()
    }

    /// Supplies application-specific data for notification fake assertions.
    fn fake_data(&self) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn locale(&self) -> Option<&str> {
        None
    }
    fn id(&self) -> Option<&str> {
        None
    }
    fn kind(&self) -> &str {
        std::any::type_name::<Self>()
    }

    fn should_send(&self, _recipient: &dyn Notifiable, _channel: &str) -> bool {
        true
    }

    /// Defers the delivery decision to a registered guard on the worker.
    fn queue_guard(
        &self,
        _recipient: &dyn Notifiable,
        _channel: &str,
    ) -> NotificationResult<Option<NotificationQueueGuard>> {
        Ok(None)
    }

    /// Chooses an application-registered middleware chain for this queued channel.
    fn queue_middleware(&self, _recipient: &dyn Notifiable, _channel: &str) -> Option<&str> {
        None
    }

    /// Configures worker limits for the queued notification job.
    fn queue_job_options(&self, _recipient: &dyn Notifiable, _channel: &str) -> JobOptions {
        JobOptions::default()
    }

    /// Encrypts this channel's payload and guard before writing it to the queue.
    fn should_encrypt(&self, _recipient: &dyn Notifiable, _channel: &str) -> bool {
        false
    }

    /// Selects queue delivery and options for this recipient and channel.
    fn queue_options(
        &self,
        _recipient: &dyn Notifiable,
        _channel: &str,
    ) -> Option<DispatchOptions> {
        None
    }
}
