use crate::{Error, Repository, Result};
use openrust::{
    container::ServiceKey,
    contracts::{
        container::BindingResolutionError,
        events::{Event, Listener},
    },
    events::{EventContext, EventError, EventFuture, EventResult, ListenerResponse},
    notifications::{
        AnonymousNotifiable, Notifiable, Notification, NotificationResult, NotificationRoute,
        messages::{MailDelivery, MailMessage, SlackMessage},
        slack::{SlackMessage as BlockKitMessage, SlackRoute},
        sms::SmsMessage,
    },
    support::facades::{App, Config, Facade, Notification as Notifications},
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
pub trait LongWaitDetectedNotification: Notification {
    fn signature(&self) -> String;
}

pub trait LongWaitNotificationFactory: Send + Sync {
    fn make(&self, event: &LongWaitDetected) -> Box<dyn LongWaitDetectedNotification>;
}

pub(crate) struct DefaultLongWaitNotificationFactory;
impl LongWaitNotificationFactory for DefaultLongWaitNotificationFactory {
    fn make(&self, event: &LongWaitDetected) -> Box<dyn LongWaitDetectedNotification> {
        Box::new(event.clone())
    }
}

impl LongWaitDetectedNotification for LongWaitDetected {
    fn signature(&self) -> String {
        format!("{}:{}", self.connection, self.queue)
    }
}

impl Event for LongWaitDetected {
    fn name(&self) -> &str {
        Self::NAME
    }
}
impl LongWaitDetected {
    pub const NAME: &str = "LongWaitDetected";

    pub async fn to_notification(
        &self,
    ) -> std::result::Result<Box<dyn LongWaitDetectedNotification>, BindingResolutionError> {
        Ok(App::make::<dyn LongWaitNotificationFactory>()
            .await?
            .make(self))
    }

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
        Self::register_slack_route(route.into().into())
    }
    pub fn route_slack_webhook_notifications_to(
        url: impl Into<String>,
        channel: impl Into<String>,
    ) -> std::result::Result<(), BindingResolutionError> {
        Self::register_slack_route(SlackRoute::for_webhook(url, Some(channel.into())))
    }
    fn register_slack_route(route: SlackRoute) -> std::result::Result<(), BindingResolutionError> {
        Routes::get_facade_root()?
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert("slack".into(), NotificationRoute::Slack(route));
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
        openrust::support::facades::Event::dispatch(event)
            .await
            .map_err(|_| Error::Unavailable)?;
    }
    Ok(())
}

pub struct SendNotification;

impl Listener for SendNotification {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let Some(event) = event.downcast_ref::<LongWaitDetected>() else {
                return Ok(None);
            };
            let notification = event.to_notification().await?;
            let repository = crate::dashboard::Store::get_facade_root()?;
            if !repository
                .notification_lock(&notification.signature())
                .await
                .map_err(listener_error)?
            {
                return Ok(None);
            }
            let routes = Routes::get_facade_root()?
                .0
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            if routes.is_empty() {
                return Ok(None);
            }
            let mut recipient = AnonymousNotifiable::default();
            for (channel, route) in routes {
                recipient = recipient.route(channel, route).map_err(listener_error)?;
            }
            Notifications::send(&[&recipient], notification.as_ref())
                .await
                .map_err(listener_error)?;
            Ok(None)
        })
    }
}

fn listener_error(source: impl std::error::Error + Send + Sync + 'static) -> EventError {
    EventError::Listener {
        source: Box::new(source),
    }
}
