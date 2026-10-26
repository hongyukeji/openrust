#![doc = include_str!("README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::sync::Arc;

use crate::config::ConfigError;
pub use crate::config::mailer::MailConfig;
use crate::core::{App, AppBuilder, BoxFuture, Error, Plugin, Result};
#[cfg(feature = "mail-smtp")]
use lettre::AsyncTransport;
use lettre::Message;

/// Re-exported message builder and SMTP types used by [`Email`] implementations.
pub use lettre;

/// Creates a transport message for delivery.
pub trait Email {
    /// Builds this email as a lettre message.
    fn message(&self) -> Result<Message>;
}

/// A pluggable email delivery backend.
pub trait MailDriver: Send + Sync + 'static {
    /// Sends one composed message.
    fn send(&self, message: Message) -> BoxFuture<'_, Result<()>>;
}

/// Application service for sending [`Email`] values.
#[derive(Clone)]
pub struct Mailer {
    driver: Arc<dyn MailDriver>,
}

impl Mailer {
    /// Creates a mailer backed by the supplied driver.
    pub fn new(driver: impl MailDriver) -> Self {
        Self {
            driver: Arc::new(driver),
        }
    }

    /// Composes and sends one email.
    pub async fn send<E: Email>(&self, email: &E) -> Result<()> {
        self.driver.send(email.message()?).await
    }
}

/// Installs the configured [`Mailer`] service; the default driver logs delivery without content.
#[derive(Debug, Default, Clone, Copy)]
pub struct MailerPlugin;

impl Plugin for MailerPlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        let config = app.section::<MailConfig>("mail")?;
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
    let mut builder = lettre::AsyncSmtpTransport::<lettre::Tokio1Executor>::starttls_relay(host)
        .map_err(Error::internal)?
        .port(config.smtp_port);
    if let Some(credentials) = credentials {
        builder = builder.credentials(credentials);
    }
    Ok(Mailer::new(SmtpMailer {
        transport: builder.build(),
    }))
}

#[cfg(not(feature = "mail-smtp"))]
fn configured_smtp(_config: &MailConfig) -> Result<Mailer> {
    Err(Error::Config(ConfigError::Invalid(
        "mail.driver `smtp` requires the `mail-smtp` feature".into(),
    )))
}

/// Sends email without contacting an SMTP server or logging message contents.
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

/// Sends email through an SMTP server using mandatory TLS.
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

/// Captures composed messages for tests without sending them.
#[cfg(feature = "test-util")]
#[derive(Clone, Default)]
pub struct FakeMailer {
    messages: Arc<std::sync::Mutex<Vec<Message>>>,
}

#[cfg(feature = "test-util")]
impl FakeMailer {
    /// Returns a mailer service and a handle for inspecting captured messages.
    pub fn new() -> (Mailer, Self) {
        let fake = Self::default();
        (Mailer::new(fake.clone()), fake)
    }

    /// Returns the number of messages accepted by this fake driver.
    pub fn sent_count(&self) -> usize {
        self.messages.lock().map_or(0, |messages| messages.len())
    }

    /// Returns an error unless a captured message was addressed to `recipient`.
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
    #[cfg(feature = "test-util")]
    use lettre::message::Mailbox;

    #[cfg(feature = "test-util")]
    struct WelcomeEmail;

    #[cfg(feature = "test-util")]
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
}
