#![doc = include_str!("README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::{fmt, sync::Arc};

use crate::config::ConfigError;
pub use crate::config::mailer::MailConfig;
use crate::core::Error;
use crate::core::Result;
use crate::support::BoxFuture;
use crate::translation::Locale;
use crate::translation::MessageArgs;
use crate::translation::Translator;
#[cfg(feature = "mail-smtp")]
use lettre::AsyncTransport;
use lettre::Message;

#[cfg(feature = "mail-templates")]
mod template;
#[cfg(feature = "mail-templates")]
pub use template::{EmailTemplateBody, EmailTemplateCatalog, EmailTemplateError, RenderedEmail};

/// 重新导出 [`Email`] 实现使用的消息构建器与 SMTP 类型。
pub use lettre;

/// 创建用于投递的传输消息。
pub trait Email {
    /// 将此邮件构建为 lettre 消息。
    fn message(&self) -> Result<Message>;
}

/// 使用两个明确指定的语言目录 key 组合纯文本邮件。
///
/// 主题和正文在构造时只翻译一次。后台任务应显式传入 [`Locale`]，不要依赖请求任务的局部状态。
pub struct LocalizedEmail {
    message: Message,
}

impl LocalizedEmail {
    /// 从共享目录渲染主题和正文，并构建纯文本邮件。
    pub fn new(
        i18n: &Translator,
        locale: &Locale,
        args: &MessageArgs,
        from: &str,
        to: &str,
        subject_key: &str,
        body_key: &str,
    ) -> Result<Self> {
        let from = from
            .parse::<lettre::message::Mailbox>()
            .map_err(Error::internal)?;
        let to = to
            .parse::<lettre::message::Mailbox>()
            .map_err(Error::internal)?;
        let subject = i18n.translate(locale, subject_key, args);
        let body = i18n.translate(locale, body_key, args);
        let message = Message::builder()
            .from(from)
            .to(to)
            .subject(subject)
            .body(body)
            .map_err(Error::internal)?;
        Ok(Self { message })
    }
}

impl fmt::Debug for LocalizedEmail {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LocalizedEmail([REDACTED])")
    }
}

impl Email for LocalizedEmail {
    fn message(&self) -> Result<Message> {
        Ok(self.message.clone())
    }
}

/// 可插拔的邮件投递后端。
pub trait MailDriver: Send + Sync + 'static {
    /// 发送一封已构建的邮件。
    fn send(&self, message: Message) -> BoxFuture<'_, Result<()>>;
}

/// 用于发送 [`Email`] 的应用服务。
#[derive(Clone)]
pub struct Mailer {
    driver: Arc<dyn MailDriver>,
}

impl Mailer {
    /// 创建由指定 driver 支持的邮件服务。
    pub fn new(driver: impl MailDriver) -> Self {
        Self {
            driver: Arc::new(driver),
        }
    }

    /// 构建并发送一封邮件。
    pub async fn send<E: Email>(&self, email: &E) -> Result<()> {
        self.driver.send(email.message()?).await
    }
}

/// 经统一 Provider 从当前配置构造邮件服务；log 不受应用环境限制。
#[derive(Debug, Default, Clone, Copy)]
pub struct MailerServiceProvider;

impl crate::support::ServiceProvider for MailerServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::foundation::RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use crate::config::repository::SectionSchema;
            app.config_section(
                "mail",
                MailConfig::default(),
                SectionSchema::object([
                    ("driver", SectionSchema::String),
                    ("smtp_host", SectionSchema::String.nullable()),
                    ("smtp_port", SectionSchema::Integer),
                    ("smtp_starttls", SectionSchema::Boolean),
                    (
                        "smtp_username",
                        SectionSchema::String.nullable().sensitive(),
                    ),
                    (
                        "smtp_password",
                        SectionSchema::String.nullable().sensitive(),
                    ),
                ]),
                |_| Ok(()),
            )?;
            app.singleton::<MailConfig, _>(|resolver| {
                Box::pin(async move { Ok(Arc::new(resolver.section::<MailConfig>("mail")?)) })
            })
            .await?;
            app.singleton::<Mailer, _>(|resolver| {
                Box::pin(async move {
                    let config = resolver.section::<MailConfig>("mail").map_err(|source| {
                        Error::Config(ConfigError::diagnostic_with_source(
                            "mail",
                            "invalid_section",
                            source,
                        ))
                    })?;
                    Ok(Arc::new(configured_mailer(&config)?))
                })
            })
            .await?;
            app.require::<Mailer>();
            Ok(())
        })
    }

    fn boot<'a>(
        &'a self,
        app: &'a mut crate::foundation::BootContext<'_>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let _ = app.resolve::<Mailer>().await?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::foundation::BootContext;
    use crate::foundation::ProviderBuilder;
    use crate::support::ServiceProvider;
    use figment::{Figment, providers::Serialized};
    use serde_json::json;

    struct ChangeDriver;
    impl ServiceProvider for ChangeDriver {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(
                async move { app.overlay("mail", json!({"driver": "secret-driver-sentinel"})) },
            )
        }
    }

    fn sources(env: &str) -> Figment {
        Figment::from(Serialized::defaults(json!({"app": {"env": env}})))
    }

    #[tokio::test]
    async fn provider_constructs_in_test_environment() -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder.provider(MailerServiceProvider);
        let runtime = builder.start(sources("test")).await?;
        let _ = runtime.container().resolve::<Mailer>().await?;
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await
    }

    #[tokio::test]
    async fn provider_constructs_log_in_production_or_without_environment() -> Result<()> {
        for source in [sources("production"), Figment::new()] {
            let mut builder = ProviderBuilder::new();
            builder.provider(MailerServiceProvider);
            let runtime = builder.start(source).await?;
            let _ = runtime.container().resolve::<Mailer>().await?;
            runtime
                .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
                .await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn provider_reads_prior_overlay_and_redacts_invalid_driver() {
        let mut builder = ProviderBuilder::new();
        builder
            .provider(ChangeDriver)
            .provider(MailerServiceProvider);
        let result = builder.start(sources("test")).await;
        assert!(result.is_err());
        if let Err(error) = result {
            assert!(!format!("{error:?} {error}").contains("secret-driver-sentinel"));
        }
    }

    #[tokio::test]
    async fn provider_rejects_malformed_config_without_echoing_value() {
        let mut builder = ProviderBuilder::new();
        builder.provider(MailerServiceProvider);
        let source = Figment::from(Serialized::defaults(
            json!({"app":{"env":"test"},"mail":{"driver":{"secret":"private-sentinel"}}}),
        ));
        let result = builder.start(source).await;
        assert!(result.is_err());
        if let Err(error) = result {
            assert!(!format!("{error:?} {error}").contains("private-sentinel"));
        }
    }

    #[cfg(not(feature = "mail-smtp"))]
    #[tokio::test]
    async fn provider_rejects_driver_without_feature() {
        let mut builder = ProviderBuilder::new();
        builder.provider(MailerServiceProvider);
        let sources = Figment::from(Serialized::defaults(
            json!({"app":{"env":"test"},"mail":{"driver":"smtp"}}),
        ));
        assert!(builder.start(sources).await.is_err());
    }
}

fn configured_mailer(config: &MailConfig) -> Result<Mailer> {
    match config.driver.as_str() {
        "log" => Ok(Mailer::new(LogMailer)),
        "smtp" => configured_smtp(config),
        _ => Err(Error::Config(ConfigError::diagnostic(
            "mail.driver",
            "unsupported_driver",
        ))),
    }
}

#[cfg(feature = "mail-smtp")]
fn configured_smtp(config: &MailConfig) -> Result<Mailer> {
    let host = config
        .smtp_host
        .as_deref()
        .ok_or_else(|| Error::Config(ConfigError::diagnostic("mail.smtp_host", "required")))?;
    let credentials = match (&config.smtp_username, &config.smtp_password) {
        (None, None) => None,
        (Some(username), Some(password)) => {
            Some(lettre::transport::smtp::authentication::Credentials::new(
                username.clone(),
                password.clone(),
            ))
        }
        _ => {
            return Err(Error::Config(ConfigError::diagnostic(
                "mail.smtp_username",
                "credentials_pair_required",
            )));
        }
    };
    if !config.smtp_starttls {
        if !is_loopback_host(host) {
            return Err(Error::Config(ConfigError::diagnostic(
                "mail.smtp_starttls",
                "remote_tls_required",
            )));
        }
        if credentials.is_some() {
            return Err(Error::Config(ConfigError::diagnostic(
                "mail.smtp_starttls",
                "credentials_tls_required",
            )));
        }
    }
    let mut builder = if config.smtp_starttls {
        lettre::AsyncSmtpTransport::<lettre::Tokio1Executor>::starttls_relay(host).map_err(
            |source| {
                Error::Config(ConfigError::diagnostic_with_source(
                    "mail.smtp_host",
                    "invalid_host",
                    source,
                ))
            },
        )?
    } else {
        lettre::AsyncSmtpTransport::<lettre::Tokio1Executor>::builder_dangerous(host)
    }
    .port(config.smtp_port);
    if let Some(credentials) = credentials {
        builder = builder.credentials(credentials);
    }
    Ok(Mailer::new(SmtpMailer {
        transport: builder.build(),
    }))
}

#[cfg(feature = "mail-smtp")]
fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

#[cfg(not(feature = "mail-smtp"))]
fn configured_smtp(_config: &MailConfig) -> Result<Mailer> {
    Err(Error::Config(ConfigError::diagnostic(
        "mail.driver",
        "missing_feature:mail-smtp",
    )))
}

/// 记录邮件已被接受，但不连接 SMTP 服务器，也不记录邮件内容。
#[derive(Debug, Default, Clone, Copy)]
pub struct LogMailer;

impl MailDriver for LogMailer {
    fn send(&self, _message: Message) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            tracing::info!("email accepted by the log mail driver");
            Ok(())
        })
    }
}

/// 通过 SMTP 服务器发送邮件；远程主机必须启用 STARTTLS，本机 loopback 可按配置使用明文连接。
#[cfg(feature = "mail-smtp")]
struct SmtpMailer {
    transport: lettre::AsyncSmtpTransport<lettre::Tokio1Executor>,
}

#[cfg(feature = "mail-smtp")]
impl MailDriver for SmtpMailer {
    fn send(&self, message: Message) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.transport
                .send(message)
                .await
                .map_err(Error::internal)?;
            Ok(())
        })
    }
}

/// 在测试中捕获已构建的邮件，不会实际发送。
#[cfg(feature = "test-util")]
#[derive(Clone, Default)]
pub struct FakeMailer {
    messages: Arc<std::sync::Mutex<Vec<Message>>>,
}

#[cfg(feature = "test-util")]
impl FakeMailer {
    /// 返回邮件服务和用于检查已捕获邮件的句柄。
    pub fn new() -> (Mailer, Self) {
        let fake = Self::default();
        (Mailer::new(fake.clone()), fake)
    }

    /// 返回此 fake driver 接受的邮件数量。
    pub fn sent_count(&self) -> usize {
        self.messages.lock().map_or(0, |messages| messages.len())
    }

    /// 如果没有捕获到发给 `recipient` 的邮件，则返回错误。
    pub fn assert_sent_to(&self, recipient: &str) -> Result<()> {
        let messages = self
            .messages
            .lock()
            .map_err(|error| Error::internal(std::io::Error::other(error.to_string())))?;
        if messages.iter().any(|message| {
            message
                .envelope()
                .to()
                .iter()
                .any(|address| address.to_string() == recipient)
        }) {
            Ok(())
        } else {
            Err(Error::custom(
                "email_not_sent",
                format!("no captured email was addressed to `{recipient}`"),
            ))
        }
    }
}

#[cfg(feature = "test-util")]
impl MailDriver for FakeMailer {
    fn send(&self, message: Message) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.messages
                .lock()
                .map_err(|error| Error::internal(std::io::Error::other(error.to_string())))?
                .push(message);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(any(feature = "test-util", feature = "mail-smtp"))]
    use lettre::message::Mailbox;

    #[cfg(any(feature = "test-util", feature = "mail-smtp"))]
    struct WelcomeEmail;

    #[cfg(any(feature = "test-util", feature = "mail-smtp"))]
    impl Email for WelcomeEmail {
        fn message(&self) -> Result<Message> {
            Message::builder()
                .from(
                    "OpenRust <noreply@example.test>"
                        .parse::<Mailbox>()
                        .map_err(Error::internal)?,
                )
                .to("user@example.test"
                    .parse::<Mailbox>()
                    .map_err(Error::internal)?)
                .subject("Welcome")
                .body(String::from("Welcome to OpenRust"))
                .map_err(Error::internal)
        }
    }

    #[test]
    fn localized_email_renders_subject_and_body_from_explicit_locale() -> Result<()> {
        let mut catalog = crate::translation::MessageCatalog::new();
        catalog.insert("en", "mail.subject", "Welcome {name}")?;
        catalog.insert("en", "mail.body", "Hello {name}")?;
        catalog.insert("zh-CN", "mail.subject", "欢迎 {name}")?;
        catalog.insert("zh-CN", "mail.body", "你好 {name}")?;
        let i18n = Translator::new(crate::translation::TranslationConfig::default(), catalog)
            .map_err(Error::Config)?;
        let locale = Locale::new("zh-CN").map_err(Error::Config)?;
        let args = MessageArgs::from([("name".to_owned(), "Mina".to_owned())]);
        let email = LocalizedEmail::new(
            &i18n,
            &locale,
            &args,
            "OpenRust <noreply@example.test>",
            "Mina <mina@example.test>",
            "mail.subject",
            "mail.body",
        )?;
        let formatted = String::from_utf8(email.message()?.formatted()).map_err(Error::internal)?;
        assert!(formatted.contains("=?utf-8?b?5qyi6L+O?= Mina"));
        assert!(formatted.contains("5L2g5aW9IE1pbmE="));
        assert!(!format!("{email:?}").contains("Mina"));
        Ok(())
    }

    #[tokio::test]
    async fn production_accepts_log_without_external_credentials() -> Result<()> {
        let mut builder = crate::foundation::ProviderBuilder::new();
        builder.provider(MailerServiceProvider);
        let figment = figment::Figment::from(figment::providers::Serialized::defaults(
            serde_json::json!({"app":{"env":"production"},"mail":{"driver":"log"}}),
        ));
        let runtime = builder.start(figment).await?;
        let _ = runtime.container().resolve::<Mailer>().await?;
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await
    }

    #[cfg(feature = "mail-smtp")]
    #[tokio::test]
    #[ignore = "requires Mailpit; run explicitly with OPENRUST_TEST_MAILPIT_SMTP_PORT"]
    async fn smtp_submits_to_local_mailpit() -> Result<()> {
        let config = MailConfig {
            driver: "smtp".into(),
            smtp_host: Some("127.0.0.1".into()),
            smtp_port: std::env::var("OPENRUST_TEST_MAILPIT_SMTP_PORT")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(1025),
            smtp_starttls: false,
            ..MailConfig::default()
        };
        configured_mailer(&config)?.send(&WelcomeEmail).await
    }

    #[tokio::test]
    #[cfg(feature = "test-util")]
    async fn fake_driver_captures_composed_messages() -> Result<(), Box<dyn std::error::Error>> {
        let (mailer, fake) = FakeMailer::new();
        mailer.send(&WelcomeEmail).await?;
        assert_eq!(fake.sent_count(), 1);
        fake.assert_sent_to("user@example.test")?;
        assert!(fake.assert_sent_to("missing@example.test").is_err());
        Ok(())
    }

    #[test]
    fn mail_config_debug_redacts_password() {
        let config = MailConfig {
            smtp_password: Some("secret-value".into()),
            ..MailConfig::default()
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("secret-value"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn unknown_mail_driver_is_rejected() {
        let config = MailConfig {
            driver: "unknown".into(),
            ..MailConfig::default()
        };
        assert!(configured_mailer(&config).is_err());
    }

    #[cfg(feature = "mail-smtp")]
    #[test]
    fn smtp_driver_builds_tls_transport_and_requires_complete_credentials() {
        let config = MailConfig {
            driver: "smtp".into(),
            smtp_host: Some("smtp.example.test".into()),
            ..MailConfig::default()
        };
        assert!(configured_mailer(&config).is_ok());

        let invalid = MailConfig {
            smtp_username: Some("mailer".into()),
            ..config
        };
        assert!(configured_mailer(&invalid).is_err());
    }

    #[cfg(feature = "mail-smtp")]
    #[test]
    fn plaintext_smtp_is_limited_to_unauthenticated_loopback() {
        let local = MailConfig {
            driver: "smtp".into(),
            smtp_host: Some("localhost".into()),
            smtp_starttls: false,
            ..MailConfig::default()
        };
        assert!(configured_mailer(&local).is_ok());
        let remote = MailConfig {
            smtp_host: Some("mail.example.test".into()),
            ..local.clone()
        };
        assert!(configured_mailer(&remote).is_err());
        let authenticated = MailConfig {
            smtp_username: Some("user".into()),
            smtp_password: Some("pass".into()),
            ..local
        };
        assert!(configured_mailer(&authenticated).is_err());
    }
}

#[cfg(test)]
mod log_diagnostic_tests;
