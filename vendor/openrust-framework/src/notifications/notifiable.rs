use super::{
    AnonymousNotifiable, DatabaseKey, Notification, NotificationFacade, NotificationFuture,
    NotificationResult, NotificationRoute,
};

/// Resolves delivery routes and the preferred locale for a recipient.
pub trait Notifiable: Send + Sync {
    /// Stable diagnostic identity for this recipient without resolving delivery routes.
    fn notification_tag(&self) -> String {
        let kind = std::any::type_name::<Self>();
        match self.notification_key() {
            Some(key) => format!("{kind}:{key}"),
            None => kind.into(),
        }
    }
    fn route_notification_for(
        &self,
        channel: &str,
        notification: &dyn Notification,
    ) -> NotificationResult<Option<NotificationRoute>>;

    fn preferred_locale(&self) -> Option<&str> {
        None
    }

    /// Stable identity for assertions when the recipient has no database route.
    fn notification_key(&self) -> Option<String> {
        None
    }

    fn as_anonymous(&self) -> Option<&AnonymousNotifiable> {
        None
    }

    fn receives_broadcast_notifications_on(
        &self,
        notification: &dyn Notification,
    ) -> NotificationResult<Vec<String>> {
        let route = self.route_notification_for("database", notification)?;
        let Some(NotificationRoute::Database(route)) = route else {
            return Ok(Vec::new());
        };
        let key = match route.key {
            DatabaseKey::Unsigned(key) => key.to_string(),
            DatabaseKey::Integer(key) => key.to_string(),
            DatabaseKey::String(key) => key,
        };
        Ok(vec![format!(
            "{}.{key}",
            route.model_type.replace("::", ".").replace('\\', ".")
        )])
    }

    fn notify<'a>(
        &'a self,
        notification: &'a dyn Notification,
    ) -> NotificationFuture<'a, NotificationResult<()>>
    where
        Self: Sized,
    {
        Box::pin(async move { NotificationFacade::send(&[self], notification).await })
    }

    fn notify_now<'a>(
        &'a self,
        notification: &'a dyn Notification,
        channels: Option<&'a [&'a str]>,
    ) -> NotificationFuture<'a, NotificationResult<()>>
    where
        Self: Sized,
    {
        Box::pin(async move { NotificationFacade::send_now(&[self], notification, channels).await })
    }
}
