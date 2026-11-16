use std::collections::BTreeMap;

use super::{Notifiable, Notification, NotificationResult, NotificationRoute, delivery};

/// A recipient assembled from explicit delivery routes.
#[derive(Clone, Default)]
pub struct AnonymousNotifiable {
    routes: BTreeMap<String, NotificationRoute>,
}

impl AnonymousNotifiable {
    pub fn route(
        mut self,
        channel: impl Into<String>,
        route: NotificationRoute,
    ) -> NotificationResult<Self> {
        let channel = channel.into();
        if channel == "database" || matches!(route, NotificationRoute::Database(_)) {
            return Err(delivery(
                "The database channel does not support on-demand notifications.",
            ));
        }
        self.routes.insert(channel, route);
        Ok(self)
    }

    pub fn routes(&self) -> &BTreeMap<String, NotificationRoute> {
        &self.routes
    }
}

impl Notifiable for AnonymousNotifiable {
    fn route_notification_for(
        &self,
        channel: &str,
        _: &dyn Notification,
    ) -> NotificationResult<Option<NotificationRoute>> {
        Ok(self.routes.get(channel).cloned())
    }

    fn as_anonymous(&self) -> Option<&AnonymousNotifiable> {
        Some(self)
    }
}
