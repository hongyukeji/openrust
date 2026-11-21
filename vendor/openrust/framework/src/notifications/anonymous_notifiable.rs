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
    #[allow(non_snake_case, reason = "Matches the notifiable diagnostic API")]
    fn notificationTag(&self) -> String {
        let routes = self
            .routes
            .values()
            .flat_map(|route| match route {
                NotificationRoute::Mail(addresses) => addresses
                    .iter()
                    .map(|address| address.address.clone())
                    .collect(),
                NotificationRoute::Sms(number) => vec![number.clone()],
                NotificationRoute::Custom(value) => vec![
                    value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string()),
                ],
                _ => Vec::new(),
            })
            .collect::<Vec<_>>();
        format!("Anonymous:{}", routes.join(","))
    }
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
