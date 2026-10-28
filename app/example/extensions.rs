//! 缓存与通信扩展的参考源码，尚未声明模块、接入生产或通过 Cargo 验收。
//!
//! Provider 入口仍需由应用显式挂载。
//! 捕获驱动仅供本地演示和测试。

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use openrust::{
    BoxFuture, Error, Result,
    cache::{Cache, CacheBackend, CacheDriver, CacheDriverConfig, CacheScope},
    config::Environment,
    core::{BootContext, RegisterContext, ServiceProvider},
    mail::{Email, MailDriver, Mailer, lettre::Message},
    sms::{SmsDriver, SmsMessage, SmsReceipt, SmsSender},
};
use serde::Deserialize;
use tokio::sync::Mutex;

/// 注册业务缓存 driver；仅配置选择 `example-prefix` 的 store 才会实例化它。
pub struct PrefixCacheServiceProvider;
impl ServiceProvider for PrefixCacheServiceProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { app.resolve::<Cache>().await?.extend(PrefixDriver) })
    }
}

struct PrefixDriver;

impl CacheDriver for PrefixDriver {
    fn name(&self) -> &'static str {
        "example-prefix"
    }

    fn validate(&self, config: &CacheDriverConfig) -> Result<()> {
        for name in config.option_names() {
            if name != "key_prefix" {
                return Err(Error::custom(
                    "example_cache_option_invalid",
                    format!("cache.stores.{}.{name} 不受支持", config.store_name()),
                ));
            }
        }
        let prefix = config
            .option::<String>("key_prefix")?
            .unwrap_or_else(|| config.store_name().to_owned());
        if prefix.trim().is_empty() {
            return Err(Error::custom(
                "example_cache_prefix_invalid",
                format!("cache.stores.{}.key_prefix 不能为空", config.store_name()),
            ));
        }
        Ok(())
    }

    fn build<'a>(
        &'a self,
        config: &'a CacheDriverConfig,
    ) -> BoxFuture<'a, Result<Arc<dyn CacheBackend>>> {
        Box::pin(async move {
            self.validate(config)?;
            let prefix = config
                .option::<String>("key_prefix")?
                .unwrap_or_else(|| config.store_name().to_owned());
            Ok(Arc::new(PrefixBackend {
                prefix,
                values: Mutex::new(HashMap::new()),
            }) as Arc<dyn CacheBackend>)
        })
    }
}

struct CacheEntry {
    value: Vec<u8>,
    expires_at: Option<Instant>,
}

struct PrefixBackend {
    prefix: String,
    values: Mutex<HashMap<String, CacheEntry>>,
}

impl PrefixBackend {
    fn key(&self, key: &str) -> String {
        format!("{}:{key}", self.prefix)
    }
}

#[async_trait]
impl CacheBackend for PrefixBackend {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let mut values = self.values.lock().await;
        // 惰性清理所有过期条目，避免一次性业务键在后续访问时持续占用内存。
        let now = Instant::now();
        values.retain(|_, entry| entry.expires_at.is_none_or(|expires| expires > now));
        Ok(values.get(&self.key(key)).map(|entry| entry.value.clone()))
    }

    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()> {
        let expires_at = ttl
            .map(|duration| {
                Instant::now().checked_add(duration).ok_or_else(|| {
                    Error::custom("example_cache_ttl_invalid", "缓存 TTL 超出时钟范围")
                })
            })
            .transpose()?;
        self.values
            .lock()
            .await
            .insert(self.key(key), CacheEntry { value, expires_at });
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<()> {
        self.values.lock().await.remove(&self.key(key));
        Ok(())
    }

    fn scope(&self) -> CacheScope {
        CacheScope::Process
    }
}

/// 本地捕获结果，不实现 Debug，避免日志意外输出收件人和正文。
#[derive(Clone, Default)]
pub struct CapturedDeliveries {
    mail: Arc<Mutex<Vec<Message>>>,
    sms: Arc<Mutex<Vec<SmsMessage>>>,
}

impl CapturedDeliveries {
    /// 取走已捕获邮件；内容仅留在当前进程，不会真实发送。
    pub async fn take_mail(&self) -> Vec<Message> {
        std::mem::take(&mut *self.mail.lock().await)
    }

    /// 取走已捕获短信；内容仅留在当前进程，不会真实发送。
    pub async fn take_sms(&self) -> Vec<SmsMessage> {
        std::mem::take(&mut *self.sms.lock().await)
    }
}

/// 显式替换通信服务的本地测试 Provider。
pub struct LocalCaptureServiceProvider {
    captured: CapturedDeliveries,
}
impl LocalCaptureServiceProvider {
    /// 由调用方持有捕获句柄，用于断言业务实际提交的消息。
    pub fn new(captured: CapturedDeliveries) -> Self {
        Self { captured }
    }
}
#[derive(Deserialize)]
struct DriverSelection {
    driver: String,
}
fn validate_capture(
    app: &openrust::config::AppConfig,
    mail: &DriverSelection,
    sms: &DriverSelection,
) -> Result<()> {
    if app.env == Environment::Production {
        return Err(Error::custom(
            "example_capture_production_forbidden",
            "production 禁止本地捕获通信驱动",
        ));
    }
    if mail.driver != "example-capture" || sms.driver != "example-capture" {
        return Err(Error::custom(
            "example_capture_driver_invalid",
            "本地捕获需要 mail.driver 与 sms.driver 均选择 example-capture",
        ));
    }
    Ok(())
}
impl ServiceProvider for LocalCaptureServiceProvider {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        use openrust::config::repository::SectionSchema as S;
        for section in ["mail", "sms"] {
            app.config_section(
                section,
                serde_json::json!({}),
                S::object([("driver", S::String)]),
                |value: &serde_json::Value| {
                    if value.get("driver").and_then(serde_json::Value::as_str)
                        != Some("example-capture")
                    {
                        return Err(Error::custom(
                            "example_capture_driver_invalid",
                            "本地捕获需要选择 example-capture",
                        ));
                    }
                    Ok(())
                },
            )?;
        }
        let captured = self.captured.clone();
        app.singleton::<Mailer, _>(move |_| {
            let captured = captured.clone();
            Box::pin(async move { Ok(Arc::new(Mailer::new(CaptureMailDriver(captured)))) })
        });
        let captured = self.captured.clone();
        app.singleton::<SmsSender, _>(move |_| {
            let captured = captured.clone();
            Box::pin(async move { Ok(Arc::new(SmsSender::new(CaptureSmsDriver(captured)))) })
        });
        Ok(())
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            validate_capture(
                &app.section("app")?,
                &app.section("mail")?,
                &app.section("sms")?,
            )?;
            app.resolve::<Mailer>().await?;
            app.resolve::<SmsSender>().await?;
            let config = app.config_repository();
            app.validate_at_finalize(move || {
                Box::pin(async move {
                    validate_capture(
                        &config.section("app")?,
                        &config.section("mail")?,
                        &config.section("sms")?,
                    )
                })
            });
            Ok(())
        })
    }
}

struct CaptureMailDriver(CapturedDeliveries);

impl MailDriver for CaptureMailDriver {
    fn send(&self, message: Message) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.0.mail.lock().await.push(message);
            Ok(())
        })
    }
}

struct CaptureSmsDriver(CapturedDeliveries);

impl SmsDriver for CaptureSmsDriver {
    fn send(&self, message: SmsMessage) -> BoxFuture<'_, Result<SmsReceipt>> {
        Box::pin(async move {
            self.0.sms.lock().await.push(message);
            Ok(SmsReceipt {
                provider: "example-capture".into(),
                message_id: None,
            })
        })
    }
}

/// 业务邮件只负责构造内容；真实传输与本地捕获均复用同一 `Mailer`。
pub struct WelcomeEmail {
    recipient: String,
}

impl WelcomeEmail {
    /// 指定收件人，邮件地址格式在构造发送消息时校验。
    pub fn new(recipient: impl Into<String>) -> Self {
        Self {
            recipient: recipient.into(),
        }
    }
}

impl Email for WelcomeEmail {
    fn message(&self) -> Result<Message> {
        Message::builder()
            .from(
                "OpenRust <hello@example.test>"
                    .parse()
                    .map_err(Error::internal)?,
            )
            .to(self.recipient.parse().map_err(Error::internal)?)
            .subject("欢迎加入")
            .body("Welcome to OpenRust.".to_owned())
            .map_err(Error::internal)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        CapturedDeliveries, LocalCaptureServiceProvider, PrefixCacheServiceProvider, WelcomeEmail,
    };
    use figment::{
        Figment,
        providers::{Format, Toml},
    };
    use openrust::{
        Result,
        cache::{Cache, CacheScope, CacheServiceProvider},
        config::{Environment, FrameworkConfig},
        core::ProviderBuilder,
        mail::Mailer,
        sms::{SmsContent, SmsMessage, SmsSender},
    };

    fn sources(config: &str) -> Figment {
        Figment::from(figment::providers::Serialized::defaults(
            FrameworkConfig::default(),
        ))
        .merge(Toml::string(config))
    }

    const CACHE_CONFIG: &str = r#"
[cache]
default_store = "default"
[cache.stores.default]
driver = "memory"
namespace = "default"
[cache.stores.workshop]
driver = "example-prefix"
namespace = "workshop"
key_prefix = "lesson"
"#;

    const CAPTURE_CONFIG: &str = r#"
[mail]
driver = "example-capture"
[sms]
driver = "example-capture"
"#;

    #[tokio::test]
    async fn custom_named_store_preserves_crud_ttl_and_default_store() -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder
            .provider(CacheServiceProvider::new())
            .provider(PrefixCacheServiceProvider);
        let app = builder.start(sources(CACHE_CONFIG)).await?;
        let cache = app.container().resolve::<Cache>().await?;
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
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(workshop.get::<u32>("expires").await?, None);
        assert!(
            workshop
                .set("invalid", &42, Some(Duration::ZERO))
                .await
                .is_err()
        );
        Ok(())
    }

    #[tokio::test]
    async fn custom_store_rejects_bad_options_and_missing_registration() {
        for config in [
            CACHE_CONFIG.replace("key_prefix = \"lesson\"", "unknown_option = true"),
            CACHE_CONFIG.replace("key_prefix = \"lesson\"", "key_prefix = \" \""),
            CACHE_CONFIG.replace("key_prefix = \"lesson\"", "key_prefix = 42"),
        ] {
            let mut builder = ProviderBuilder::new();
            builder
                .provider(CacheServiceProvider::new())
                .provider(PrefixCacheServiceProvider);
            assert!(builder.start(sources(&config)).await.is_err());
        }
        let mut builder = ProviderBuilder::new();
        builder.provider(CacheServiceProvider::new());
        assert!(builder.start(sources(CACHE_CONFIG)).await.is_err());
    }

    #[tokio::test]
    async fn public_communication_services_capture_business_content_without_delivery() -> Result<()>
    {
        let captured = CapturedDeliveries::default();
        let mut builder = ProviderBuilder::new();
        builder.provider(LocalCaptureServiceProvider::new(captured.clone()));
        let app = builder.start(sources(CAPTURE_CONFIG)).await?;
        let mailer = app.container().resolve::<Mailer>().await?;
        let sms = app.container().resolve::<SmsSender>().await?;

        mailer
            .send(&WelcomeEmail::new("learner@example.test"))
            .await?;
        let message = SmsMessage::text("+15555550123", "课程已发布")?;
        let receipt = sms.send(message.clone()).await?;
        assert_eq!(receipt.provider, "example-capture");
        assert_eq!(receipt.message_id, None);

        let emails = captured.take_mail().await;
        assert_eq!(emails.len(), 1);
        let formatted = String::from_utf8_lossy(&emails[0].formatted()).into_owned();
        assert!(formatted.contains("learner@example.test"));
        assert!(formatted.contains("Welcome to OpenRust."));
        let messages = captured.take_sms().await;
        assert_eq!(messages, vec![message]);
        assert_eq!(messages[0].recipient(), "+15555550123");
        assert_eq!(
            messages[0].content(),
            &SmsContent::Text("课程已发布".into())
        );
        assert!(matches!(captured.take_mail().await.as_slice(), []));
        assert!(matches!(captured.take_sms().await.as_slice(), []));

        assert!(
            mailer
                .send(&WelcomeEmail::new("invalid-address"))
                .await
                .is_err()
        );
        assert!(captured.take_mail().await.is_empty());
        Ok(())
    }

    struct InvalidCaptureOverlay;
    impl openrust::core::ServiceProvider for InvalidCaptureOverlay {
        fn boot<'a>(
            &'a self,
            app: &'a mut openrust::core::BootContext<'_>,
        ) -> openrust::BoxFuture<'a, Result<()>> {
            Box::pin(async move { app.overlay("mail", serde_json::json!({"driver": "log"})) })
        }
    }
    #[tokio::test]
    async fn capture_rejects_later_driver_overlay() {
        let mut builder = ProviderBuilder::new();
        builder
            .provider(LocalCaptureServiceProvider::new(
                CapturedDeliveries::default(),
            ))
            .provider(InvalidCaptureOverlay);
        assert!(builder.start(sources(CAPTURE_CONFIG)).await.is_err());
    }

    #[tokio::test]
    async fn capture_provider_rejects_production_and_unselected_drivers() {
        let mut production = FrameworkConfig::default();
        production.app.env = Environment::Production;
        let config = Figment::from(figment::providers::Serialized::defaults(production))
            .merge(Toml::string(CAPTURE_CONFIG));
        let mut builder = ProviderBuilder::new();
        builder.provider(LocalCaptureServiceProvider::new(
            CapturedDeliveries::default(),
        ));
        assert!(builder.start(config).await.is_err());

        for config in [
            CAPTURE_CONFIG.replace(
                "[mail]\ndriver = \"example-capture\"",
                "[mail]\ndriver = \"log\"",
            ),
            CAPTURE_CONFIG.replace(
                "[sms]\ndriver = \"example-capture\"",
                "[sms]\ndriver = \"log\"",
            ),
        ] {
            let mut builder = ProviderBuilder::new();
            builder.provider(LocalCaptureServiceProvider::new(
                CapturedDeliveries::default(),
            ));
            assert!(builder.start(sources(&config)).await.is_err());
        }
    }
}
