use super::{
    Notifiable, NotificationResult,
    messages::{DatabaseMessage, MailDelivery},
};

/// Describes the channels and content for a recipient.
pub trait Notification: Send + Sync {
    fn via(&self, recipient: &dyn Notifiable) -> NotificationResult<Vec<String>>;
    fn to_mail(&self, recipient: &dyn Notifiable) -> NotificationResult<Option<MailDelivery>>;
    fn to_database(
        &self,
        recipient: &dyn Notifiable,
    ) -> NotificationResult<Option<DatabaseMessage>>;
    fn to_array(&self, recipient: &dyn Notifiable) -> NotificationResult<Option<DatabaseMessage>>;
    fn locale(&self) -> Option<&str>;
}
