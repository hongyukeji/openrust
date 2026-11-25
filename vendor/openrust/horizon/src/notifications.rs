use crate::{Error, Repository, Result};
use openrust::{
    container::ServiceKey,
    contracts::{container::BindingResolutionError, events::Event},
    notifications::{
        AnonymousNotifiable, Notifiable, Notification, NotificationResult, NotificationRoute,
        messages::{MailDelivery, MailMessage, SlackMessage},
        slack::SlackMessage as BlockKitMessage,
        sms::SmsMessage,
    },
    support::facades::{Config, Facade, Notification as Notifications},
};
use std::{collections::BTreeMap, sync::RwLock, time::Duration};
use tokio::time::Instant;

const WAIT_GREETING: &str = "Oh no! Something needs your attention.";
const WAIT_TITLE: &str = "Long Wait Detected";

#[derive(Default)]
pub(crate) struct NotificationRoutes(RwLock<BTreeMap<String, NotificationRoute>>);
struct Routes;
impl Facade for Routes {
    type Service = NotificationRoutes;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<NotificationRoutes>()
    }
}
#[derive(Clone, Debug)]
pub struct LongWaitDetected {
    pub connection: String,
    pub queue: String,
    pub seconds: u64,
}
impl Event for LongWaitDetected {
    fn name(&self) -> &str {
        "LongWaitDetected"
    }
}
impl LongWaitDetected {
    fn deployment_name() -> String {
        Config::get("horizon.name")
            .ok()
            .flatten()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    fn named_content(&self) -> String {
        format!("[{}] {}", Self::deployment_name(), self.content())
    }

    fn content(&self) -> String {
        format!(
            "The \"{}\" queue on the \"{}\" connection has a wait time of {} seconds.",
            self.queue, self.connection, self.seconds
        )
    }
}
impl Notification for LongWaitDetected {
    fn via(&self, recipient: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(recipient
            .as_anonymous()
            .map(|recipient| recipient.routes().keys().cloned().collect())
            .unwrap_or_default())
    }
    fn to_mail(&self, _: &dyn Notifiable) -> NotificationResult<Option<MailDelivery>> {
        Ok(Some(MailDelivery::Message(Box::new(
            MailMessage::new()
                .error()
                .subject(format!(
                    "{}: Long Queue Wait Detected",
                    Self::deployment_name()
                ))
                .greeting(WAIT_GREETING)
                .line(self.content()),
        ))))
    }
    fn to_legacy_slack(&self, _: &dyn Notifiable) -> NotificationResult<Option<SlackMessage>> {
        Ok(Some(
            SlackMessage::new()
                .from("OpenRust Horizon", None)
                .error()
                .content(WAIT_GREETING)
                .attachment(|attachment| {
                    attachment
                        .title(WAIT_TITLE, None)
                        .content(self.named_content());
                }),
        ))
    }
    fn to_slack(&self, _: &dyn Notifiable) -> NotificationResult<Option<BlockKitMessage>> {
        Ok(Some(
            BlockKitMessage::new()
                .username("OpenRust Horizon")
                .text(WAIT_GREETING)
                .header_block(WAIT_TITLE)
                .section_block(|section| {
                    section.text(self.named_content());
                }),
        ))
    }
    fn to_sms(&self, _: &dyn Notifiable, to: String) -> NotificationResult<Option<SmsMessage>> {
        SmsMessage::new(to, self.named_content()).map(Some)
    }
    fn fake_data(&self) -> serde_json::Value {
        serde_json::json!({"connection":self.connection,"queue":self.queue,"seconds":self.seconds})
    }
}
impl crate::Horizon {
    pub fn route_mail_notifications_to(
        email: impl Into<String>,
    ) -> std::result::Result<(), BindingResolutionError> {
        Routes::get_facade_root()?
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                "mail".into(),
                NotificationRoute::Mail(vec![openrust::mail::mailables::Address::from(
                    email.into().as_str(),
                )]),
            );
        Ok(())
    }
    pub fn route_slack_notifications_to(
        route: impl Into<String>,
    ) -> std::result::Result<(), BindingResolutionError> {
        Routes::get_facade_root()?
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                "slack".into(),
                NotificationRoute::Slack(route.into().into()),
            );
        Ok(())
    }
    pub fn route_sms_notifications_to(
        number: impl Into<String>,
    ) -> std::result::Result<(), BindingResolutionError> {
        Routes::get_facade_root()?
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert("sms".into(), NotificationRoute::Sms(number.into()));
        Ok(())
    }
}
pub(crate) async fn monitor(
    repository: &Repository,
    last_monitored: &mut Option<Instant>,
) -> Result<()> {
    if last_monitored.is_some_and(|last| last.elapsed() < Duration::from_secs(60))
        || !repository
            .metrics_repository()
            .acquire_wait_time_monitor_lock()
            .await?
    {
        return Ok(());
    }
    *last_monitored = Some(Instant::now());
    let config =
        openrust::support::facades::Config::get_facade_root().map_err(|_| Error::Unavailable)?;
    for workload in repository.workload().get().await? {
        let Some(queue) = workload["name"].as_str() else {
            continue;
        };
        let Some(connection) = workload["connection"].as_str() else {
            continue;
        };
        let name = format!("{connection}:{queue}");
        let limit = config
            .get(&format!("horizon.waits.{name}"))
            .and_then(|value| value.as_u64())
            .unwrap_or(60);
        let seconds = workload["wait"].as_f64().unwrap_or(0.).ceil() as u64;
        if limit == 0 || seconds <= limit {
            continue;
        }
        let event = LongWaitDetected {
            connection: connection.into(),
            queue: queue.into(),
            seconds,
        };
        openrust::support::facades::Event::dispatch(event.clone())
            .await
            .map_err(|_| Error::Unavailable)?;
        if !repository.notification_lock(&name).await? {
            continue;
        }
        let routes = Routes::get_facade_root()
            .map_err(|_| Error::Unavailable)?
            .0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if routes.is_empty() {
            continue;
        }
        let mut recipient = AnonymousNotifiable::default();
        for (channel, route) in routes {
            recipient = recipient
                .route(channel, route)
                .map_err(|_| Error::Unavailable)?;
        }
        Notifications::send(&[&recipient], &event)
            .await
            .map_err(|_| Error::Unavailable)?;
    }
    Ok(())
}
