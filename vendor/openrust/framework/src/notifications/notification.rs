use crate::queue::DispatchOptions;

use super::{
    Notifiable, NotificationResult,
    messages::{DatabaseMessage, MailDelivery},
};

/// Describes the channels and content for a recipient.
pub trait Notification: Send + Sync {
    fn via(&self, recipient: &dyn Notifiable) -> NotificationResult<Vec<String>>;

    fn to_mail(&self, _recipient: &dyn Notifiable) -> NotificationResult<Option<MailDelivery>> {
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

    /// Selects queue delivery and options for this recipient and channel.
    fn queue_options(
        &self,
        _recipient: &dyn Notifiable,
        _channel: &str,
    ) -> Option<DispatchOptions> {
        None
    }
}
