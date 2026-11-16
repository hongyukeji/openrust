use super::{
    AnonymousNotifiable, DatabaseKey, Notification, NotificationFacade, NotificationFuture,
    NotificationResult, NotificationRoute,
};

/// Resolves delivery routes and the preferred locale for a recipient.
#[allow(non_snake_case, reason = "Matches Laravel's Notifiable API")]
pub trait Notifiable: Send + Sync {
    fn route_notification_for(
        &self,
        channel: &str,
        notification: &dyn Notification,
    ) -> NotificationResult<Option<NotificationRoute>>;

    fn preferred_locale(&self) -> Option<&str> {
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

    fn notifyNow<'a>(
        &'a self,
        notification: &'a dyn Notification,
        channels: Option<&'a [&'a str]>,
    ) -> NotificationFuture<'a, NotificationResult<()>>
    where
        Self: Sized,
    {
        Box::pin(async move { NotificationFacade::sendNow(&[self], notification, channels).await })
    }
}
