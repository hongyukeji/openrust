//! 通过既有数据库队列调度邮件和短信。
//!
//! 每个渠道使用独立任务；使用事务派发时所有入队操作与业务变更共同提交或回滚。
//! 任务固定保存已准备的内容，worker 至少执行一次，供应商提交与队列确认之间故障可能导致重复发送。

use std::collections::BTreeMap;
use std::fmt;

use lettre::{
    Message,
    message::{MultiPart, SinglePart},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::core::{Error, Result};
use crate::database::DatabaseTransaction;
use crate::mail::{Email, Mailer};
use crate::queue::{Job, JobContext, Queue};
use crate::sms::{SmsMessage, SmsSender};
use crate::support::BoxFuture;

const DEFAULT_QUEUE: &str = "notifications";
const EMAIL_JOB_NAME: &str = "openrust.notification.email.v1";
const SMS_JOB_NAME: &str = "openrust.notification.sms.v1";
const MAX_EMAIL_PAYLOAD_BYTES: usize = 512 * 1024;

/// 固定邮件内容；它会以 JSON payload 写入队列数据库，worker 重试不会重新渲染模板。
#[derive(Clone, Serialize, Deserialize)]
pub struct EmailDelivery {
    from: String,
    to: String,
    subject: String,
    text: String,
    html: Option<String>,
}

impl EmailDelivery {
    /// 创建纯文本或 HTML alternative 邮件，并在入队前校验地址、主题和大小。
    pub fn new(
        from: impl Into<String>,
        to: impl Into<String>,
        subject: impl Into<String>,
        text: impl Into<String>,
        html: Option<String>,
    ) -> Result<Self> {
        let delivery = Self {
            from: from.into(),
            to: to.into(),
            subject: subject.into(),
            text: text.into(),
            html,
        };
        delivery.message()?;
        Ok(delivery)
    }

    fn validate(&self) -> Result<()> {
        if self.subject.len() > 998 || self.subject.contains(['\r', '\n']) {
            return Err(Error::custom(
                "notification_invalid_email",
                "email subject contains a newline or exceeds 998 bytes",
            ));
        }
        let content_bytes = self
            .text
            .len()
            .saturating_add(self.html.as_ref().map_or(0, String::len));
        if content_bytes > MAX_EMAIL_PAYLOAD_BYTES {
            return Err(Error::custom(
                "notification_email_too_large",
                "queued email content exceeds 512 KiB",
            ));
        }
        Ok(())
    }
}

impl fmt::Debug for EmailDelivery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("EmailDelivery([REDACTED])")
    }
}

impl Email for EmailDelivery {
    fn message(&self) -> Result<Message> {
        self.validate()?;
        let from = self.from.parse().map_err(Error::internal)?;
        let to = self.to.parse().map_err(Error::internal)?;
        let builder = Message::builder().from(from).to(to).subject(&self.subject);
        match &self.html {
            Some(html) => builder
                .multipart(
                    MultiPart::alternative()
                        .singlepart(SinglePart::plain(self.text.clone()))
                        .singlepart(SinglePart::html(html.clone())),
                )
                .map_err(Error::internal),
            None => builder.body(self.text.clone()).map_err(Error::internal),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
enum SmsDeliveryContent {
    Text(String),
    Template {
        sid: String,
        variables: BTreeMap<String, String>,
    },
}

/// 固定 SMS 内容；它会以 JSON payload 写入队列数据库，provider 模板不会在 worker 侧本地渲染。
#[derive(Clone, Serialize, Deserialize)]
pub struct SmsDelivery {
    to: String,
    content: SmsDeliveryContent,
}

impl SmsDelivery {
    /// 创建固定文本短信，并在入队前校验 E.164 收件人。
    pub fn text(to: impl Into<String>, body: impl Into<String>) -> Result<Self> {
        let message = SmsMessage::text(to, body)?;
        let crate::sms::SmsContent::Text(body) = message.content() else {
            return Err(Error::custom(
                "notification_invalid_sms",
                "text constructor received a non-text message",
            ));
        };
        Ok(Self {
            to: message.recipient().to_owned(),
            content: SmsDeliveryContent::Text(body.clone()),
        })
    }

    /// 创建供应商管理模板短信；SID 和变量保留至供应商请求时提交。
    pub fn template(
        to: impl Into<String>,
        sid: impl Into<String>,
        variables: BTreeMap<String, String>,
    ) -> Result<Self> {
        let message = SmsMessage::template(to, sid, variables)?;
        let crate::sms::SmsContent::Template { sid, variables } = message.content() else {
            return Err(Error::custom(
                "notification_invalid_sms",
                "template constructor received a non-template message",
            ));
        };
        Ok(Self {
            to: message.recipient().to_owned(),
            content: SmsDeliveryContent::Template {
                sid: sid.clone(),
                variables: variables.clone(),
            },
        })
    }

    fn message(&self) -> Result<SmsMessage> {
        match &self.content {
            SmsDeliveryContent::Text(body) => SmsMessage::text(&self.to, body),
            SmsDeliveryContent::Template { sid, variables } => {
                SmsMessage::template(&self.to, sid, variables.clone())
            }
        }
    }
}

impl fmt::Debug for SmsDelivery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SmsDelivery([REDACTED])")
    }
}

/// 一次通知可向邮件、短信两个独立渠道投递。
#[derive(Debug, Default, Clone)]
pub struct Notification {
    email: Option<EmailDelivery>,
    sms: Option<SmsDelivery>,
}

impl Notification {
    /// 创建尚未选择渠道的通知。
    pub fn new() -> Self {
        Self::default()
    }

    /// 添加一个邮件渠道。
    #[must_use]
    pub fn email(mut self, delivery: EmailDelivery) -> Self {
        self.email = Some(delivery);
        self
    }

    /// 添加一个短信渠道。
    #[must_use]
    pub fn sms(mut self, delivery: SmsDelivery) -> Self {
        self.sms = Some(delivery);
        self
    }

    fn has_channel(&self) -> bool {
        self.email.is_some() || self.sms.is_some()
    }
}

/// 通知渠道各自的持久化队列任务 ID。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotificationDispatch {
    /// 如果入队邮件则包含其任务 ID。
    pub email_job_id: Option<String>,
    /// 如果入队短信则包含其任务 ID。
    pub sms_job_id: Option<String>,
}

/// 复用数据库队列派发通知，不建立独立重试或投递系统。
#[derive(Clone)]
pub struct Notifier {
    queue: Queue,
    queue_name: String,
}

impl fmt::Debug for Notifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Notifier")
            .field("queue_name", &self.queue_name)
            .finish_non_exhaustive()
    }
}

impl Notifier {
    /// 使用默认 `notifications` 队列创建通知 facade。
    pub fn new(queue: Queue) -> Self {
        Self {
            queue,
            queue_name: DEFAULT_QUEUE.to_owned(),
        }
    }

    /// 设置通知专用队列名称；队列 driver 会按既有规则校验名称。
    #[must_use]
    pub fn with_queue(mut self, queue_name: impl Into<String>) -> Self {
        self.queue_name = queue_name.into();
        self
    }

    /// 将所选渠道分别排入队列；渠道间没有顺序保证。
    pub async fn dispatch(&self, notification: &Notification) -> Result<NotificationDispatch> {
        self.dispatch_to(None, None, notification).await
    }

    /// 在调用方业务事务中排入所有渠道，事务回滚会撤销全部任务。
    pub async fn dispatch_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        notification: &Notification,
    ) -> Result<NotificationDispatch> {
        self.dispatch_to(Some(transaction), None, notification)
            .await
    }

    /// 按活动期唯一键排入各渠道，唯一键不会在任务完成后永久保留。
    pub async fn dispatch_unique(
        &self,
        unique_key: &str,
        notification: &Notification,
    ) -> Result<NotificationDispatch> {
        self.dispatch_to(None, Some(unique_key), notification).await
    }

    /// 在调用方事务中以活动期唯一键排入各渠道。
    pub async fn dispatch_unique_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        unique_key: &str,
        notification: &Notification,
    ) -> Result<NotificationDispatch> {
        self.dispatch_to(Some(transaction), Some(unique_key), notification)
            .await
    }

    async fn dispatch_to(
        &self,
        transaction: Option<&DatabaseTransaction>,
        unique_key: Option<&str>,
        notification: &Notification,
    ) -> Result<NotificationDispatch> {
        if !notification.has_channel() {
            return Err(Error::custom(
                "empty_notification",
                "notification must contain at least one channel",
            ));
        }
        if unique_key.is_some_and(|key| {
            key.is_empty()
                || key.len() > 128
                || !key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
        }) {
            return Err(Error::custom(
                "invalid_queue_name",
                "unique key must be 1..=128 ASCII letters, digits, '.', '_', ':', or '-'",
            ));
        }
        let mut result = NotificationDispatch::default();
        if let Some(email) = &notification.email {
            result.email_job_id = Some(
                self.dispatch_channel::<EmailDeliveryJob, _>(
                    transaction,
                    unique_key,
                    "email",
                    email,
                )
                .await?,
            );
        }
        if let Some(sms) = &notification.sms {
            result.sms_job_id = Some(
                self.dispatch_channel::<SmsDeliveryJob, _>(transaction, unique_key, "sms", sms)
                    .await?,
            );
        }
        Ok(result)
    }

    async fn dispatch_channel<J: Job, T: Serialize>(
        &self,
        transaction: Option<&DatabaseTransaction>,
        unique_key: Option<&str>,
        channel: &str,
        payload: &T,
    ) -> Result<String> {
        let scoped_key = unique_key.map(|key| scoped_unique_key(key, channel));
        match (transaction, scoped_key.as_deref()) {
            (Some(transaction), Some(key)) => {
                self.queue
                    .dispatch_unique_in_transaction(
                        transaction,
                        &self.queue_name,
                        J::NAME,
                        key,
                        payload,
                    )
                    .await
            }
            (Some(transaction), None) => {
                self.queue
                    .dispatch_in_transaction(transaction, &self.queue_name, J::NAME, payload)
                    .await
            }
            (None, Some(key)) => {
                self.queue
                    .dispatch_unique(&self.queue_name, J::NAME, key, payload)
                    .await
            }
            (None, None) => {
                self.queue
                    .dispatch(&self.queue_name, J::NAME, payload)
                    .await
            }
        }
    }
}

fn scoped_unique_key(key: &str, channel: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"openrust.notification.v1:");
    digest.update(key.as_bytes());
    digest.update(b":");
    digest.update(channel.as_bytes());
    let bytes = digest.finalize();
    let mut encoded = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

/// 通过统一容器提供队列通知；队列目录内置邮件与短信任务。
#[derive(Debug, Default, Clone, Copy)]
pub struct NotificationsServiceProvider;

impl crate::support::ServiceProvider for NotificationsServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::foundation::RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.singleton::<Notifier, _>(|resolver| {
                Box::pin(async move {
                    let queue = resolver.resolve::<Queue>().await?;
                    Ok(std::sync::Arc::new(Notifier::new((*queue).clone())))
                })
            })
            .await?;
            Ok(())
        })
    }

    fn boot<'a>(
        &'a self,
        app: &'a mut crate::foundation::BootContext<'_>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.resolve::<Notifier>().await?;
            Ok(())
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct EmailDeliveryJob(EmailDelivery);

impl Job for EmailDeliveryJob {
    const NAME: &'static str = EMAIL_JOB_NAME;

    fn handle(&self, context: JobContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let mailer = context.service::<Mailer>().await?;
            mailer.send(&self.0).await
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct SmsDeliveryJob(SmsDelivery);

impl Job for SmsDeliveryJob {
    const NAME: &'static str = SMS_JOB_NAME;

    fn handle(&self, context: JobContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let sender = context.service::<SmsSender>().await?;
            sender.send(self.0.message()?).await?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod provider_service_tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct MailSink(Arc<AtomicUsize>);
    impl crate::mail::MailDriver for MailSink {
        fn send(&self, _: Message) -> BoxFuture<'_, Result<()>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
    }
    struct SmsSink(Arc<AtomicUsize>);
    impl crate::sms::SmsDriver for SmsSink {
        fn send(&self, _: SmsMessage) -> BoxFuture<'_, Result<crate::sms::SmsReceipt>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                Ok(crate::sms::SmsReceipt {
                    provider: "test".into(),
                    message_id: None,
                })
            })
        }
    }

    #[tokio::test]
    async fn notification_jobs_resolve_factory_services() -> Result<()> {
        struct DeliveryProvider {
            mail: Arc<AtomicUsize>,
            sms: Arc<AtomicUsize>,
        }
        impl crate::support::ServiceProvider for DeliveryProvider {
            fn register<'a>(
                &'a self,
                app: &'a mut crate::foundation::RegisterContext<'_>,
            ) -> crate::support::BoxFuture<'a, Result<()>> {
                Box::pin(async move {
                    let count = Arc::clone(&self.mail);
                    app.singleton::<Mailer, _>(move |_| {
                        let count = Arc::clone(&count);
                        Box::pin(async move { Ok(Arc::new(Mailer::new(MailSink(count)))) })
                    })
                    .await?;
                    let count = Arc::clone(&self.sms);
                    app.singleton::<SmsSender, _>(move |_| {
                        let count = Arc::clone(&count);
                        Box::pin(async move { Ok(Arc::new(SmsSender::new(SmsSink(count)))) })
                    })
                    .await?;
                    Ok(())
                })
            }
        }
        let mail_count = Arc::new(AtomicUsize::new(0));
        let sms_count = Arc::new(AtomicUsize::new(0));
        let mut builder = crate::foundation::ProviderBuilder::new();
        builder.provider(DeliveryProvider {
            mail: Arc::clone(&mail_count),
            sms: Arc::clone(&sms_count),
        });
        let app = crate::foundation::Application::from_provider_runtime(
            builder.start(figment::Figment::new()).await?,
        )?;
        let context = JobContext::new("test", 1, 1, crate::core::Cancellation::new())?
            .with_app_state(app.state().clone());
        let registry = crate::queue::QueueContributions::default().job_registry();
        registry
            .execute(
                EmailDeliveryJob::NAME,
                serde_json::to_value(EmailDeliveryJob(EmailDelivery::new(
                    "sender@example.test",
                    "user@example.test",
                    "subject",
                    "body",
                    None,
                )?))
                .map_err(Error::internal)?,
                context.clone(),
            )
            .await?;
        registry
            .execute(
                SmsDeliveryJob::NAME,
                serde_json::to_value(SmsDeliveryJob(SmsDelivery::text("+12025550123", "body")?))
                    .map_err(Error::internal)?,
                context,
            )
            .await?;
        assert_eq!(mail_count.load(Ordering::SeqCst), 1);
        assert_eq!(sms_count.load(Ordering::SeqCst), 1);
        Ok(())
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::foundation::ProviderBuilder;
    use crate::foundation::RegisterContext;
    use crate::queue::{DispatchOptions, QueueDriver};
    use crate::support::ServiceProvider;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct Driver(Arc<AtomicUsize>);
    impl QueueDriver for Driver {
        fn dispatch<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: serde_json::Value,
            _: DispatchOptions,
        ) -> BoxFuture<'a, Result<String>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok("job-id".into()) })
        }
        fn dispatch_in_transaction<'a>(
            &'a self,
            _: &'a DatabaseTransaction,
            _: &'a str,
            _: &'a str,
            _: serde_json::Value,
            _: DispatchOptions,
        ) -> BoxFuture<'a, Result<String>> {
            Box::pin(async { Err(Error::custom("test", "unsupported")) })
        }
        fn run_next<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<bool>> {
            Box::pin(async { Ok(false) })
        }
        fn retry_failed<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<String>> {
            Box::pin(async { Err(Error::custom("test", "unsupported")) })
        }
        fn reset_shutdown(&self) {}
        fn request_shutdown(&self) {}
        fn is_shutdown_requested(&self) -> bool {
            false
        }
    }
    struct QueueBinding(Arc<AtomicUsize>);
    impl ServiceProvider for QueueBinding {
        fn register<'a>(
            &'a self,
            app: &'a mut RegisterContext<'_>,
        ) -> crate::support::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let calls = Arc::clone(&self.0);
                app.singleton::<Queue, _>(move |_| {
                    let calls = Arc::clone(&calls);
                    Box::pin(async move {
                        Ok(Arc::new(Queue::with_driver(
                            Driver(calls),
                            &crate::config::QueueConfig::default(),
                        )?))
                    })
                })
                .await?;
                Ok(())
            })
        }
    }
    #[tokio::test]
    async fn later_queue_binding_and_shared_notifier_are_used() -> Result<()> {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut builder = ProviderBuilder::new();
        builder
            .provider(NotificationsServiceProvider)
            .provider(QueueBinding(Arc::clone(&calls)));
        let runtime = builder.start(figment::Figment::new()).await?;
        let a = runtime.container().resolve::<Notifier>().await?;
        let b = runtime.container().resolve::<Notifier>().await?;
        assert!(Arc::ptr_eq(&a, &b));
        let notification = Notification::new().sms(SmsDelivery::text("+12025550123", "message")?);
        a.dispatch(&notification).await?;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await
    }
    struct BrokenQueue;
    impl ServiceProvider for BrokenQueue {
        fn register<'a>(
            &'a self,
            app: &'a mut RegisterContext<'_>,
        ) -> crate::support::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.singleton::<Queue, _>(|_| {
                    Box::pin(async { Err(Error::custom("broken_driver", "private-queue-secret")) })
                })
                .await?;
                Ok(())
            })
        }
    }
    #[tokio::test]
    async fn queue_factory_failure_is_not_ignored_or_exposed() -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder
            .provider(NotificationsServiceProvider)
            .provider(BrokenQueue);
        let error = builder
            .start(figment::Figment::new())
            .await
            .err()
            .ok_or_else(|| Error::custom("test", "broken queue must fail startup"))?;
        assert_eq!(error.code(), "service_initialization_failed");
        assert!(!error.to_string().contains("private-queue-secret"));
        Ok(())
    }
    #[tokio::test]
    async fn missing_queue_fails_startup() {
        let mut builder = ProviderBuilder::new();
        builder.provider(NotificationsServiceProvider);
        assert!(builder.start(figment::Figment::new()).await.is_err());
    }
}
