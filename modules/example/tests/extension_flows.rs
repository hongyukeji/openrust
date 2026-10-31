//! 模块业务通过公开缓存/通信扩展及生产 `TestApp` 的调用证据；不向供应商发送。

use std::sync::Arc;
use std::time::Duration;

use figment::{
    Figment,
    providers::{Format, Serialized, Toml},
};
use openrust::cache::{Cache, CacheScope};
use openrust::config::{Environment, FrameworkConfig};
use openrust::mail::{MailDriver, Mailer, lettre::Message};
use openrust::sms::{SmsDriver, SmsMessage, SmsReceipt, SmsSender};
use openrust::testing::TestApp;
use openrust::{BoxFuture, Error, Result};

use super::extensions::CapturedDeliveries;
use super::services::welcome::{WelcomeRecipient, WelcomeService};

type ServerApp = TestApp<crate::bootstrap::Server>;

fn sources() -> Figment {
    let mut config = FrameworkConfig::default();
    config.app.env = Environment::Test;
    Figment::from(Serialized::defaults(config)).merge(Toml::string(
        r#"
[cache]
default = "default"
[cache.stores.default]
driver = "memory"
namespace = "default"
[cache.stores.workshop]
driver = "example-prefix"
namespace = "workshop"
key_prefix = "lesson"
[security.encryption]
active_key_id = "test"
[security.encryption.keys]
test = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="
"#,
    ))
}

#[tokio::test]
async fn module_named_cache_driver_preserves_default_and_rejects_unsupported_tags() -> Result<()> {
    let mut app = ServerApp::builder(sources()).build().await?;
    let cache = app.app()?.state().resolve::<Cache>().await?;
    let workshop = cache.store("workshop").await?;
    assert_eq!(workshop.scope(), CacheScope::Process);
    assert!(workshop.tagged(&["lessons"]).is_err());
    cache.set("lesson:1", &"default", None).await?;
    assert_eq!(workshop.get::<String>("lesson:1").await?, None);
    workshop.set("lesson:1", &"draft", None).await?;
    workshop.set("lesson:1", &"published", None).await?;
    assert_eq!(
        workshop.get::<String>("lesson:1").await?.as_deref(),
        Some("published")
    );
    assert_eq!(
        cache.get::<String>("lesson:1").await?.as_deref(),
        Some("default")
    );
    workshop.remove("lesson:1").await?;
    workshop.remove("lesson:1").await?;
    assert_eq!(workshop.get::<String>("lesson:1").await?, None);
    workshop
        .set("expires", &42, Some(Duration::from_millis(1)))
        .await?;
    // 等待明确的 TTL 时间，不以调度次数推测进程缓存是否过期。
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(workshop.get::<u32>("expires").await?, None);
    assert!(
        workshop
            .set("invalid", &42, Some(Duration::ZERO))
            .await
            .is_err()
    );
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
}

#[tokio::test]
async fn module_named_driver_validates_options_and_is_not_registered_when_disabled() -> Result<()> {
    for config in [
        sources().merge(Serialized::defaults(
            serde_json::json!({"cache":{"stores":{"workshop":{"unknown_option":true}}}}),
        )),
        sources().merge(Serialized::defaults(
            serde_json::json!({"cache":{"stores":{"workshop":{"key_prefix":" "}}}}),
        )),
        sources().merge(Serialized::defaults(
            serde_json::json!({"cache":{"stores":{"workshop":{"key_prefix":42}}}}),
        )),
    ] {
        assert!(ServerApp::builder(config).build().await.is_err());
    }
    let mut builder = ServerApp::builder(sources());
    builder.remove_module("example");
    // 配置仍选择该业务 driver，但未启用其模块时必须失败，不回退到 memory/Redis。
    assert!(builder.build().await.is_err());
    Ok(())
}

#[tokio::test]
async fn welcome_service_captures_real_business_content_only_after_explicit_call() -> Result<()> {
    let captured = CapturedDeliveries::default();
    let capture = captured.clone();
    let mut builder = ServerApp::builder(sources());
    builder.configure_after_register(move |bindings| {
        capture.install(bindings);
        Ok(())
    });
    let mut app = builder.build().await?;
    let welcome = app.app()?.state().resolve::<WelcomeService>().await?;
    assert_eq!(captured.take_mail().await.len(), 0);
    assert_eq!(captured.take_sms().await.len(), 0);
    welcome
        .send(WelcomeRecipient::new(
            "learner@example.test",
            "+15555550123",
        )?)
        .await?;
    let mail = captured.take_mail().await;
    assert_eq!(mail.len(), 1);
    let message = String::from_utf8_lossy(&mail[0].formatted()).into_owned();
    assert!(message.contains("learner@example.test"));
    assert!(message.contains("Welcome to OpenRust."));
    let sms = captured.take_sms().await;
    assert_eq!(sms.len(), 1);
    assert_eq!(sms[0].recipient(), "+15555550123");
    assert_eq!(
        sms[0],
        SmsMessage::text("+15555550123", "欢迎加入 OpenRust 示例业务。")?
    );
    assert_eq!(captured.take_mail().await.len(), 0);
    assert_eq!(captured.take_sms().await.len(), 0);
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
}

#[tokio::test]
async fn invalid_destinations_are_rejected_before_any_channel_sends() -> Result<()> {
    let captured = CapturedDeliveries::default();
    let capture = captured.clone();
    let mut builder = ServerApp::builder(sources());
    builder.configure_after_register(move |bindings| {
        capture.install(bindings);
        Ok(())
    });
    let mut app = builder.build().await?;
    for (email, phone) in [
        ("secret-sentinel", "+15555550123"),
        ("learner@example.test", "secret-sentinel"),
    ] {
        let failure = WelcomeRecipient::new(email, phone)
            .err()
            .ok_or_else(|| Error::custom("test_expected_error", "无效目的地未被拒绝"))?;
        assert!(!format!("{failure} {failure:?}").contains("secret-sentinel"));
    }
    assert_eq!(captured.take_mail().await.len(), 0);
    assert_eq!(captured.take_sms().await.len(), 0);
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
}

struct FailingMail;
impl MailDriver for FailingMail {
    fn send(&self, _: Message) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Err(Error::custom("example_mail_failed", "测试邮件失败")) })
    }
}
struct FailingSms;
impl SmsDriver for FailingSms {
    fn send(&self, _: SmsMessage) -> BoxFuture<'_, Result<SmsReceipt>> {
        Box::pin(async { Err(Error::custom("example_sms_failed", "测试 SMS 失败")) })
    }
}

#[tokio::test]
async fn welcome_transport_failures_propagate_without_claiming_atomic_delivery() -> Result<()> {
    for mail_fails in [false, true] {
        let captured = CapturedDeliveries::default();
        let capture = captured.clone();
        let mut builder = ServerApp::builder(sources());
        builder.configure_after_register(move |bindings| {
            capture.install(bindings);
            if mail_fails {
                bindings.instance(Arc::new(Mailer::new(FailingMail)));
            } else {
                bindings.instance(Arc::new(SmsSender::new(FailingSms)));
            }
            Ok(())
        });
        let mut app = builder.build().await?;
        let error = app
            .app()?
            .state()
            .resolve::<WelcomeService>()
            .await?
            .send(WelcomeRecipient::new(
                "learner@example.test",
                "+15555550123",
            )?)
            .await
            .err()
            .ok_or_else(|| Error::custom("test_expected_error", "传输失败未传播"))?;
        assert_eq!(
            error.code(),
            if mail_fails {
                "example_mail_failed"
            } else {
                "example_sms_failed"
            }
        );
        assert_eq!(captured.take_mail().await.len(), usize::from(!mail_fails));
        assert_eq!(captured.take_sms().await.len(), 0);
        app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await?;
    }
    Ok(())
}

#[tokio::test]
async fn capture_installation_is_explicit_and_rejects_production() -> Result<()> {
    let capture = CapturedDeliveries::default();
    let mut builder = ServerApp::builder(sources().merge(Serialized::defaults(
        serde_json::json!({"app":{"env":"production"}}),
    )));
    builder.configure_after_register(move |bindings| {
        capture.install(bindings);
        Ok(())
    });
    assert!(builder.build().await.is_err());
    // 不调用 install 时，标准 Provider 保持自身 driver；服务装配本身不会捕获或发送欢迎消息。
    let mut unmodified = ServerApp::builder(sources()).build().await?;
    unmodified
        .app()?
        .state()
        .resolve::<WelcomeService>()
        .await?;
    unmodified
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
}

struct LateProduction;
impl openrust::core::ServiceProvider for LateProduction {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.resolve::<WelcomeService>().await?;
            app.overlay("app", serde_json::json!({"env":"production"}))
        })
    }
}

#[tokio::test]
async fn capture_rechecks_environment_after_a_later_boot_overlay() -> Result<()> {
    let captured = CapturedDeliveries::default();
    let capture = captured.clone();
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(super::services::welcome::WelcomeServiceProvider)
        .provider(LateProduction);
    builder.configure_after_register(move |bindings| {
        capture.install(bindings);
        Ok(())
    });
    let runtime = builder.start(sources()).await?;
    let error = runtime
        .container()
        .resolve::<WelcomeService>()
        .await?
        .send(WelcomeRecipient::new(
            "learner@example.test",
            "+15555550123",
        )?)
        .await
        .err()
        .ok_or_else(|| Error::custom("test_expected_error", "后续生产环境覆盖未被捕获驱动拒绝"))?;
    assert_eq!(error.code(), "example_capture_production_forbidden");
    assert_eq!(captured.take_mail().await.len(), 0);
    assert_eq!(captured.take_sms().await.len(), 0);
    runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
}
