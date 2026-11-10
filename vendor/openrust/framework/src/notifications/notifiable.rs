use super::{Notification, NotificationResult, NotificationRoute};

/// Resolves delivery routes and the preferred locale for a recipient.
pub trait Notifiable: Send + Sync {
    fn route_notification_for(
        &self,
        channel: &str,
        notification: &dyn Notification,
    ) -> NotificationResult<Option<NotificationRoute>>;

    fn preferred_locale(&self) -> Option<&str>;
}
