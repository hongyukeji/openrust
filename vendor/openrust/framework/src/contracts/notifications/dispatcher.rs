use crate::notifications::{Notifiable, Notification, NotificationFuture, NotificationResult};

/// Delivers notifications to one or more recipients.
pub trait Dispatcher: Send + Sync {
    fn send<'a>(
        &'a self,
        recipients: &'a [&'a dyn Notifiable],
        notification: &'a dyn Notification,
    ) -> NotificationFuture<'a, NotificationResult<()>>;

    fn send_now<'a>(
        &'a self,
        recipients: &'a [&'a dyn Notifiable],
        notification: &'a dyn Notification,
        channels: Option<&'a [&'a str]>,
    ) -> NotificationFuture<'a, NotificationResult<()>>;
}
