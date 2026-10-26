#![doc = include_str!("README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::{fmt, sync::Arc};

use crate::config::ConfigError;
pub use crate::config::mailer::MailConfig;
use crate::core::{App, AppBuilder, BoxFuture, Error, Plugin, Result};
use crate::i18n::{I18n, Locale, MessageArgs};
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
        i18n: &I18n,
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

/// 安装配置好的 [`Mailer`] 服务；默认 driver 仅记录投递状态，不记录邮件内容。
#[derive(Debug, Default, Clone, Copy)]
pub struct MailerPlugin;

impl Plugin for MailerPlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        let config = app.sensitive_section::<MailConfig>("mail")?;
        if app.config().app.env == crate::config::Environment::Production && config.driver == "log"
        {
            return Err(Error::Config(ConfigError::Invalid(
                "mail.driver=log is not allowed in production; configure a real mail provider"
                    .into(),
            )));
        }
        let mailer = configured_mailer(&config)?;
        app.insert(config);
        app.insert(mailer);
        Ok(())
    }

    fn finish<'a>(&'a self, _app: &'a App) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

fn configured_mailer(config: &MailConfig) -> Result<Mailer> {
    match config.driver.as_str() {
        "log" => Ok(Mailer::new(LogMailer)),
        "smtp" => configured_smtp(config),
        other => Err(Error::Config(ConfigError::Invalid(format!(
            "unsupported mail.driver `{other}`; expected `log` or `smtp`"
        )))),
    }
}

#[cfg(feature = "mail-smtp")]
fn configured_smtp(config: &MailConfig) -> Result<Mailer> {
    let host = config.smtp_host.as_deref().ok_or_else(|| {
        Error::Config(ConfigError::Invalid(
            "missing mail.smtp_host for the SMTP driver".into(),
        ))
    })?;
    let credentials = match (&config.smtp_username, &config.smtp_password) {
        (None, None) => None,
        (Some(username), Some(password)) => {
            Some(lettre::transport::smtp::authentication::Credentials::new(
                username.clone(),
                password.clone(),
            ))
        }
        _ => {
            return Err(Error::Config(ConfigError::Invalid(
                "mail.smtp_username and mail.smtp_password must be configured together".into(),
            )));
        }
    };
    if !config.smtp_starttls {
        if !is_loopback_host(host) {
            return Err(Error::Config(ConfigError::Invalid(
                "mail.smtp_starttls=false is limited to localhost or a loopback IP".into(),
            )));
        }
        if credentials.is_some() {
            return Err(Error::Config(ConfigError::Invalid(
                "mail.smtp_username/password require mail.smtp_starttls=true".into(),
            )));
        }
    }
    let mut builder = if config.smtp_starttls {
        lettre::AsyncSmtpTransport::<lettre::Tokio1Executor>::starttls_relay(host)
            .map_err(Error::internal)?
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
    Err(Error::Config(ConfigError::Invalid(
        "mail.driver `smtp` requires the `mail-smtp` feature".into(),
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
        let mut catalog = crate::i18n::MessageCatalog::new();
        catalog.insert("en", "mail.subject", "Welcome {name}")?;
        catalog.insert("en", "mail.body", "Hello {name}")?;
        catalog.insert("zh-CN", "mail.subject", "欢迎 {name}")?;
        catalog.insert("zh-CN", "mail.body", "你好 {name}")?;
        let i18n = I18n::new(crate::i18n::I18nConfig::default(), catalog).map_err(Error::Config)?;
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

    #[test]
    fn production_rejects_log_driver() {
        let mut framework = crate::config::FrameworkConfig::default();
        framework.app.env = crate::config::Environment::Production;
        let figment = figment::Figment::new().merge(("mail.driver", "log"));
        let mut builder = AppBuilder::new(framework, figment);
        assert!(MailerPlugin.build(&mut builder).is_err());
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
