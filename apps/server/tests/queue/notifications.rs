//! 验证通知通过真实数据库队列派发，并由 worker 中的应用服务处理。
#![cfg(all(feature = "queue", any(feature = "db-postgres", feature = "db-mysql")))]
#![allow(clippy::expect_used, reason = "测试夹具配置错误时应立即终止")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use figment::{
    Figment,
    providers::{Format, Toml},
};
#[cfg(feature = "mail-smtp")]
use openrust::mail::MailerPlugin;
use openrust::mail::lettre::Message;
use openrust::{
    config::FrameworkConfig,
    core::{App, AppBuilder, BoxFuture, Error, Result as FrameworkResult, lifecycle},
    database::{
        DatabasePlugin, Db, sea_orm_migration::MigrationTrait, sea_orm_migration::MigratorTrait,
    },
    mail::{MailDriver, Mailer},
    notifications::{EmailDelivery, Notification, NotificationsPlugin, Notifier, SmsDelivery},
    queue::{Queue, QueueMigration, QueuePlugin},
    sms::{SmsDriver, SmsMessage, SmsReceipt, SmsSender},
};
use testing::TestDatabase;

struct QueueMigrator;

impl MigratorTrait for QueueMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(QueueMigration)]
    }
}

#[derive(Clone, Default)]
struct CountingMailer;

impl MailDriver for CountingMailer {
    fn send(&self, _message: Message) -> BoxFuture<'_, FrameworkResult<()>> {
        Box::pin(async move {
            let accepted_count = EMAIL_SENT.fetch_add(1, Ordering::SeqCst);
            if FAIL_AFTER_ACCEPT.swap(false, Ordering::SeqCst) && accepted_count == 1 {
                Err(Error::custom(
                    "test_ambiguous_mail_failure",
                    "simulated acknowledgement loss after provider acceptance",
                ))
            } else {
                Ok(())
            }
        })
    }
}

#[derive(Clone, Default)]
struct CountingSms;

impl SmsDriver for CountingSms {
    fn send(&self, _message: SmsMessage) -> BoxFuture<'_, FrameworkResult<SmsReceipt>> {
        Box::pin(async move {
            SMS_SENT.fetch_add(1, Ordering::SeqCst);
            Ok(SmsReceipt {
                provider: "test".into(),
                message_id: None,
            })
        })
    }
}

async fn build_fake_app(admin_url: &str) -> FrameworkResult<(App, TestDatabase)> {
    let test_database = TestDatabase::create::<QueueMigrator>(admin_url).await?;
    EMAIL_SENT.store(0, Ordering::SeqCst);
    SMS_SENT.store(0, Ordering::SeqCst);
    FAIL_AFTER_ACCEPT.store(false, Ordering::SeqCst);
    let mut builder = AppBuilder::new(
        FrameworkConfig::default(),
        Figment::new().merge(Toml::string(&format!(
            "[database]\nurl = {:?}\n",
            test_database.url()
        ))),
    );
    builder
        .insert(Mailer::new(CountingMailer))
        .insert(SmsSender::new(CountingSms))
        .plugin(DatabasePlugin)
        .plugin(QueuePlugin::default())
        .plugin(NotificationsPlugin);
    let app = lifecycle::finalize(builder).await?;
    Ok((app, test_database))
}

fn notification() -> FrameworkResult<Notification> {
    Ok(Notification::new()
        .email(EmailDelivery::new(
            "noreply@example.test",
            "person@example.test",
            "Account update",
            "Your account was updated.",
            None,
        )?)
        .sms(SmsDelivery::text("+15555550123", "Account updated")?))
}

#[tokio::test]
async fn notification_channels_share_transaction_but_run_as_independent_jobs()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping queued notifications: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let (app, test_database) = build_fake_app(&admin_url).await?;
    let queue = app.state().try_get::<Queue>()?;
    let notifier = app.state().try_get::<Notifier>()?;
    let db = app.state().try_get::<Db>()?;
    let queue_name = format!("notify{}", std::process::id());
    let notifier = (*notifier).clone().with_queue(queue_name.clone());
    let message = notification()?;
    assert!(notifier.dispatch_unique("", &message).await.is_err());

    let rollback = db
        .transaction(|transaction| {
            let notifier = notifier.clone();
            let message = message.clone();
            Box::pin(async move {
                notifier
                    .dispatch_unique_in_transaction(transaction, "signup-42", &message)
                    .await?;
                Err::<(), _>(Error::custom(
                    "rollback_probe",
                    "rollback notification dispatch",
                ))
            })
        })
        .await;
    assert!(rollback.is_err());
    assert!(!queue.run_next_with_state(&queue_name, app.state()).await?);

    let first = db
        .transaction(|transaction| {
            let notifier = notifier.clone();
            let message = message.clone();
            Box::pin(async move {
                notifier
                    .dispatch_unique_in_transaction(transaction, "signup-42", &message)
                    .await
            })
        })
        .await?;
    let duplicate = notifier.dispatch_unique("signup-42", &message).await?;
    assert_eq!(first.email_job_id, duplicate.email_job_id);
    assert_eq!(first.sms_job_id, duplicate.sms_job_id);
    assert_ne!(first.email_job_id, first.sms_job_id);
    assert!(queue.run_next_with_state(&queue_name, app.state()).await?);
    assert!(queue.run_next_with_state(&queue_name, app.state()).await?);
    assert!(!queue.run_next_with_state(&queue_name, app.state()).await?);

    assert_eq!(EMAIL_SENT.load(Ordering::SeqCst), 1);
    assert_eq!(SMS_SENT.load(Ordering::SeqCst), 1);

    FAIL_AFTER_ACCEPT.store(true, Ordering::SeqCst);
    notifier
        .dispatch(&Notification::new().email(EmailDelivery::new(
            "noreply@example.test",
            "person@example.test",
            "Retry boundary",
            "The first provider acceptance has an ambiguous result.",
            None,
        )?))
        .await?;
    assert!(queue.run_next_with_state(&queue_name, app.state()).await?);
    assert_eq!(EMAIL_SENT.load(Ordering::SeqCst), 2);
    assert!(!queue.run_next_with_state(&queue_name, app.state()).await?);
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    assert!(queue.run_next_with_state(&queue_name, app.state()).await?);
    assert_eq!(EMAIL_SENT.load(Ordering::SeqCst), 3);
    db.connection()?.close().await?;
    drop(notifier);
    drop(queue);
    drop(db);
    drop(app);
    test_database.close().await?;
    Ok(())
}

static EMAIL_SENT: AtomicUsize = AtomicUsize::new(0);
static SMS_SENT: AtomicUsize = AtomicUsize::new(0);
static FAIL_AFTER_ACCEPT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[tokio::test]
#[cfg(feature = "mail-smtp")]
async fn queued_notification_is_accepted_by_configured_smtp_sink()
-> Result<(), Box<dyn std::error::Error>> {
    let (Ok(admin_url), Ok(host), Ok(port)) = (
        std::env::var("OPENRUST_TEST_DATABASE_URL"),
        std::env::var("OPENRUST_TEST_SMTP_HOST"),
        std::env::var("OPENRUST_TEST_SMTP_PORT"),
    ) else {
        eprintln!("skipping queued SMTP delivery: database URL or SMTP sink is unset");
        return Ok(());
    };
    let test_database = TestDatabase::create::<QueueMigrator>(&admin_url).await?;
    let config = Figment::new().merge(Toml::string(&format!(
        "[database]\nurl = {:?}\n[mail]\ndriver = \"smtp\"\nsmtp_host = {:?}\nsmtp_port = {}\nsmtp_starttls = false\n",
        test_database.url(), host, port.parse::<u16>()?
    )));
    let mut builder = AppBuilder::new(FrameworkConfig::default(), config);
    builder
        .plugin(DatabasePlugin)
        .plugin(QueuePlugin::default())
        .plugin(MailerPlugin)
        .plugin(NotificationsPlugin);
    let app = lifecycle::finalize(builder).await?;
    let queue = app.state().try_get::<Queue>()?;
    let notifier = app.state().try_get::<Notifier>()?;
    let queue_name = format!("smtp{}", std::process::id());
    let notifier = (*notifier).clone().with_queue(queue_name.clone());
    notifier
        .dispatch(&Notification::new().email(EmailDelivery::new(
            "noreply@example.test",
            "recipient@example.test",
            "Queued SMTP acceptance",
            "OpenRust queued SMTP delivery",
            Some("<p>OpenRust queued SMTP delivery</p>".into()),
        )?))
        .await?;
    assert!(queue.run_next_with_state(&queue_name, app.state()).await?);
    app.state().try_get::<Db>()?.connection()?.close().await?;
    drop(notifier);
    drop(queue);
    drop(app);
    test_database.close().await?;
    Ok(())
}
