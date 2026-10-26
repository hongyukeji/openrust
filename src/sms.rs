//! SMS delivery through an explicit, replaceable provider driver.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

pub use crate::config::SmsConfig;
use crate::config::{ConfigError, Environment};
use crate::core::{App, AppBuilder, BoxFuture, Error, Plugin, Result};
use crate::i18n::{I18n, Locale};
#[cfg(feature = "sms-twilio")]
use http::header::AUTHORIZATION;
#[cfg(feature = "sms-twilio")]
use http::{HeaderMap, HeaderValue, StatusCode};

#[cfg(feature = "sms-twilio")]
use crate::http::{HttpClient, HttpClientConfig};

/// SMS content, either literal text or a provider template with string parameters.
#[derive(Clone, PartialEq, Eq)]
pub enum SmsContent {
    /// Plain text content.
    Text(String),
    /// Provider-managed template SID and its named string values.
    Template {
        /// Provider-specific template identifier.
        sid: String,
        /// Named template values serialized as JSON for providers that support them.
        variables: BTreeMap<String, String>,
    },
}

impl fmt::Debug for SmsContent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SmsContent([REDACTED])")
    }
}

/// One SMS submission. Recipient and content are redacted from debug output.
#[derive(Clone, PartialEq, Eq)]
pub struct SmsMessage {
    to: String,
    content: SmsContent,
}

impl SmsMessage {
    /// Creates a text message for an E.164 recipient.
    pub fn text(to: impl Into<String>, body: impl Into<String>) -> Result<Self> {
        Self::new(to.into(), SmsContent::Text(body.into()))
    }

    /// Creates a provider-template message for an E.164 recipient.
    pub fn template(
        to: impl Into<String>,
        sid: impl Into<String>,
        variables: BTreeMap<String, String>,
    ) -> Result<Self> {
        Self::new(
            to.into(),
            SmsContent::Template {
                sid: sid.into(),
                variables,
            },
        )
    }

    fn new(to: String, content: SmsContent) -> Result<Self> {
        if !valid_e164(&to) {
            return Err(Error::custom(
                "sms_invalid_recipient",
                "SMS recipient must be an E.164 phone number",
            ));
        }
        match &content {
            SmsContent::Text(body) if body.trim().is_empty() => {
                return Err(Error::custom(
                    "sms_empty_content",
                    "SMS content cannot be empty",
                ));
            }
            SmsContent::Template { sid, .. } if sid.trim().is_empty() => {
                return Err(Error::custom(
                    "sms_invalid_template",
                    "SMS template SID cannot be empty",
                ));
            }
            _ => {}
        }
        Ok(Self { to, content })
    }

    /// Returns the selected content variant.
    pub fn content(&self) -> &SmsContent {
        &self.content
    }

    /// Returns the recipient address.
    pub fn recipient(&self) -> &str {
        &self.to
    }

    /// Translates text content with the supplied locale; provider templates stay provider-owned.
    pub fn localized_text(
        to: impl Into<String>,
        i18n: &I18n,
        locale: &Locale,
        key: &str,
        args: &crate::i18n::MessageArgs,
    ) -> Result<Self> {
        Self::text(to, i18n.translate(locale, key, args))
    }
}

impl fmt::Debug for SmsMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SmsMessage")
            .field("to", &"[REDACTED]")
            .field("content", &self.content)
            .finish()
    }
}

/// Provider submission acknowledgement. It does not claim final delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmsReceipt {
    /// Driver name that accepted the submission.
    pub provider: String,
    /// Provider message identifier, when available.
    pub message_id: Option<String>,
}

/// Pluggable SMS submission backend. Implementations must not log message contents or credentials.
pub trait SmsDriver: Send + Sync + 'static {
    /// Submits one message without automatic retries.
    fn send(&self, message: SmsMessage) -> BoxFuture<'_, Result<SmsReceipt>>;
}

/// Application service for submitting SMS messages.
#[derive(Clone)]
pub struct SmsSender {
    driver: Arc<dyn SmsDriver>,
}

impl SmsSender {
    /// Creates a sender backed by the supplied driver.
    pub fn new(driver: impl SmsDriver) -> Self {
        Self {
            driver: Arc::new(driver),
        }
    }

    /// Submits one message and returns the provider's acceptance receipt.
    pub async fn send(&self, message: SmsMessage) -> Result<SmsReceipt> {
        self.driver.send(message).await
    }
}

/// Installs the configured [`SmsSender`] service.
#[derive(Debug, Default, Clone, Copy)]
pub struct SmsPlugin;

impl Plugin for SmsPlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        let config = app.sensitive_section::<SmsConfig>("sms")?;
        if app.config().app.env == Environment::Production && config.driver == "log" {
            return Err(Error::Config(ConfigError::Invalid(
                "sms.driver=log is not allowed in production; configure a real SMS provider".into(),
            )));
        }
        let sender = configured_sender(&config)?;
        app.insert(config);
        app.insert(sender);
        Ok(())
    }

    fn finish<'a>(&'a self, _app: &'a App) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

fn configured_sender(config: &SmsConfig) -> Result<SmsSender> {
    match config.driver.as_str() {
        "log" => Ok(SmsSender::new(LogSmsDriver)),
        "twilio" => configured_twilio(config),
        other => Err(Error::Config(ConfigError::Invalid(format!(
            "unsupported sms.driver `{other}`; expected `log` or `twilio`"
        )))),
    }
}

/// Logs SMS submission metadata without disclosing recipient or content.
#[derive(Debug, Default, Clone, Copy)]
pub struct LogSmsDriver;

impl SmsDriver for LogSmsDriver {
    fn send(&self, _message: SmsMessage) -> BoxFuture<'_, Result<SmsReceipt>> {
        Box::pin(async {
            tracing::info!(channel = "sms", "message accepted by the log driver");
            Ok(SmsReceipt {
                provider: "log".into(),
                message_id: None,
            })
        })
    }
}

#[cfg(feature = "test-util")]
/// Captures SMS submissions for tests without contacting a provider.
#[derive(Clone, Default)]
pub struct FakeSmsDriver {
    messages: Arc<std::sync::Mutex<Vec<SmsMessage>>>,
}

#[cfg(feature = "test-util")]
impl FakeSmsDriver {
    /// Returns a sender and a handle for inspecting captured messages.
    pub fn new() -> (SmsSender, Self) {
        let fake = Self::default();
        (SmsSender::new(fake.clone()), fake)
    }

    /// Returns the number of captured SMS submissions.
    pub fn sent_count(&self) -> usize {
        self.messages.lock().map_or(0, |messages| messages.len())
    }

    /// Returns captured messages for assertions.
    pub fn messages(&self) -> Result<Vec<SmsMessage>> {
        self.messages
            .lock()
            .map(|messages| messages.clone())
            .map_err(|error| Error::internal(std::io::Error::other(error.to_string())))
    }
}

#[cfg(feature = "test-util")]
impl SmsDriver for FakeSmsDriver {
    fn send(&self, message: SmsMessage) -> BoxFuture<'_, Result<SmsReceipt>> {
        Box::pin(async move {
            self.messages
                .lock()
                .map_err(|error| Error::internal(std::io::Error::other(error.to_string())))?
                .push(message);
            Ok(SmsReceipt {
                provider: "fake".into(),
                message_id: None,
            })
        })
    }
}

#[cfg(feature = "sms-twilio")]
/// Twilio Messaging REST driver using the shared bounded outbound HTTP client.
pub struct TwilioSmsDriver {
    client: HttpClient,
    account_sid: String,
    api_key_sid: String,
    api_key_secret: String,
    from: Option<String>,
    messaging_service_sid: Option<String>,
}

#[cfg(feature = "sms-twilio")]
impl fmt::Debug for TwilioSmsDriver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TwilioSmsDriver")
            .field("account_sid", &"[REDACTED]")
            .field("api_key_sid", &"[REDACTED]")
            .field("api_key_secret", &"[REDACTED]")
            .field("from", &self.from.as_ref().map(|_| "[REDACTED]"))
            .field(
                "messaging_service_sid",
                &self.messaging_service_sid.as_ref().map(|_| "[REDACTED]"),
            )
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "sms-twilio")]
impl TwilioSmsDriver {
    /// Creates a driver using a custom HTTP client, useful for proxies and transport contract tests.
    pub fn new(config: SmsConfig, client: HttpClient) -> Result<Self> {
        let account_sid = required(config.account_sid.as_deref(), "sms.account_sid")?;
        let api_key_sid = required(config.api_key_sid.as_deref(), "sms.api_key_sid")?;
        let api_key_secret = required(config.api_key_secret.as_deref(), "sms.api_key_secret")?;
        let has_from = config
            .from
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
        let has_service = config
            .messaging_service_sid
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
        if has_from == has_service {
            return Err(config_error(
                "configure exactly one of sms.from or sms.messaging_service_sid",
            ));
        }
        Ok(Self {
            client,
            account_sid,
            api_key_sid,
            api_key_secret,
            from: config.from.filter(|value| !value.trim().is_empty()),
            messaging_service_sid: config
                .messaging_service_sid
                .filter(|value| !value.trim().is_empty()),
        })
    }
}

#[cfg(feature = "sms-twilio")]
impl SmsDriver for TwilioSmsDriver {
    fn send(&self, message: SmsMessage) -> BoxFuture<'_, Result<SmsReceipt>> {
        Box::pin(async move {
            use crate::http::client::HttpClientError;
            use base64::Engine as _;

            let mut fields = vec![("To", message.to.clone())];
            if let Some(from) = &self.from {
                fields.push(("From", from.clone()));
            }
            if let Some(service) = &self.messaging_service_sid {
                fields.push(("MessagingServiceSid", service.clone()));
            }
            match message.content {
                SmsContent::Text(body) => fields.push(("Body", body)),
                SmsContent::Template { sid, variables } => {
                    fields.push(("ContentSid", sid));
                    fields.push((
                        "ContentVariables",
                        serde_json::to_string(&variables).map_err(Error::internal)?,
                    ));
                }
            }
            let auth = base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", self.api_key_sid, self.api_key_secret));
            let mut headers = HeaderMap::new();
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Basic {auth}")).map_err(|_| {
                    Error::custom("sms_provider_config", "invalid Twilio credentials")
                })?,
            );
            let url = format!(
                "https://api.twilio.com/2010-04-01/Accounts/{}/Messages.json",
                self.account_sid
            );
            let response = self
                .client
                .post(&url)
                .headers(headers)
                .form(&fields)
                .send()
                .await
                .map_err(|error| match error {
                    HttpClientError::Timeout => Error::custom_with_status(
                        StatusCode::GATEWAY_TIMEOUT,
                        "sms_provider_timeout",
                        "SMS provider timed out; submission status may be unknown",
                    ),
                    _ => Error::custom_with_status(
                        StatusCode::BAD_GATEWAY,
                        "sms_provider_unavailable",
                        "SMS provider request failed",
                    ),
                })?;
            if !response.status().is_success() {
                return Err(Error::custom_with_status(
                    StatusCode::BAD_GATEWAY,
                    "sms_provider_rejected",
                    "SMS provider rejected the submission",
                ));
            }
            let payload: serde_json::Value = response.json().await.map_err(|_| {
                Error::custom_with_status(
                    StatusCode::BAD_GATEWAY,
                    "sms_provider_invalid_response",
                    "SMS provider returned an invalid response",
                )
            })?;
            let message_id = payload
                .get("sid")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            Ok(SmsReceipt {
                provider: "twilio".into(),
                message_id,
            })
        })
    }
}

#[cfg(feature = "sms-twilio")]
fn configured_twilio(config: &SmsConfig) -> Result<SmsSender> {
    let client = HttpClient::new(HttpClientConfig::default()).map_err(|_| {
        config_error("unable to initialize outbound HTTP client for sms.driver=twilio")
    })?;
    Ok(SmsSender::new(TwilioSmsDriver::new(
        config.clone(),
        client,
    )?))
}

#[cfg(not(feature = "sms-twilio"))]
fn configured_twilio(_config: &SmsConfig) -> Result<SmsSender> {
    Err(config_error(
        "sms.driver=twilio requires the sms-twilio feature",
    ))
}

#[cfg(feature = "sms-twilio")]
fn required(value: Option<&str>, key: &str) -> Result<String> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| config_error(&format!("missing {key}")))
}

fn config_error(message: &str) -> Error {
    Error::Config(ConfigError::Invalid(message.to_owned()))
}

fn valid_e164(value: &str) -> bool {
    let Some(number) = value.strip_prefix('+') else {
        return false;
    };
    (2..=15).contains(&number.len())
        && number
            .as_bytes()
            .first()
            .is_some_and(|digit| (b'1'..=b'9').contains(digit))
        && number.bytes().all(|digit| digit.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_rejects_log_driver() {
        use crate::core::Plugin as _;
        let mut framework = crate::config::FrameworkConfig::default();
        framework.app.env = Environment::Production;
        let figment = figment::Figment::new().merge(("sms.driver", "log"));
        let mut builder = AppBuilder::new(framework, figment);
        assert!(SmsPlugin.build(&mut builder).is_err());
    }

    #[test]
    fn recipient_and_content_are_validated() {
        assert!(SmsMessage::text("+14155552671", "hello").is_ok());
        assert!(SmsMessage::text("14155552671", "hello").is_err());
        assert!(SmsMessage::text("+0123", "hello").is_err());
        assert!(SmsMessage::text("+14155552671", " ").is_err());
    }

    #[test]
    fn debug_does_not_disclose_message_data() -> Result<()> {
        let message = SmsMessage::text("+14155552671", "secret-code-123")?;
        let debug = format!("{message:?}");
        assert!(!debug.contains("14155552671"));
        assert!(!debug.contains("secret-code-123"));
        Ok(())
    }

    #[cfg(feature = "sms-twilio")]
    #[test]
    fn twilio_config_requires_credentials_and_exactly_one_sender() {
        let config = SmsConfig::default();
        let client = HttpClient::new(HttpClientConfig::default());
        assert!(client.is_ok_and(|client| TwilioSmsDriver::new(config, client).is_err()));
    }

    #[cfg(feature = "test-util")]
    #[tokio::test]
    async fn fake_captures_messages_without_provider() -> Result<()> {
        let (sender, fake) = FakeSmsDriver::new();
        sender
            .send(SmsMessage::text("+14155552671", "code")?)
            .await?;
        assert_eq!(fake.sent_count(), 1);
        assert_eq!(fake.messages()?[0].recipient(), "+14155552671");
        Ok(())
    }
}

#[cfg(all(test, feature = "sms-twilio"))]
mod twilio_tests {
    use super::*;
    use crate::http::{HttpTransport, HttpTransportRequest, TransportResponse};
    use async_trait::async_trait;
    use std::sync::Mutex;

    type RequestSnapshot = (String, String, Vec<u8>);

    #[derive(Clone)]
    struct RecordingTransport {
        status: StatusCode,
        delay: std::time::Duration,
        seen: Arc<Mutex<Vec<RequestSnapshot>>>,
    }

    #[async_trait]
    impl HttpTransport for RecordingTransport {
        async fn send(
            &self,
            request: HttpTransportRequest,
        ) -> std::result::Result<TransportResponse, crate::http::HttpClientError> {
            let authorization = request
                .headers()
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            let url = request.url().as_str().to_owned();
            let body = request
                .body()
                .and_then(reqwest::Body::as_bytes)
                .map_or_else(Vec::new, <[u8]>::to_vec);
            self.seen
                .lock()
                .map_err(|_| crate::http::HttpClientError::Transport)?
                .push((url, authorization, body));
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            Ok(TransportResponse::from_bytes(
                self.status,
                HeaderMap::new(),
                br#"{"sid":"SM123"}"#.to_vec(),
            ))
        }
    }

    fn twilio_config() -> SmsConfig {
        SmsConfig {
            driver: "twilio".into(),
            account_sid: Some("AC00000000000000000000000000000000".into()),
            api_key_sid: Some("SK00000000000000000000000000000000".into()),
            api_key_secret: Some("test-secret".into()),
            from: Some("+14155550100".into()),
            messaging_service_sid: None,
        }
    }

    #[tokio::test]
    async fn twilio_posts_form_with_basic_auth_and_returns_acceptance_id() -> Result<()> {
        use base64::Engine as _;

        let seen = Arc::new(Mutex::new(Vec::new()));
        let client = HttpClient::with_transport(
            HttpClientConfig::default(),
            RecordingTransport {
                status: StatusCode::CREATED,
                delay: std::time::Duration::ZERO,
                seen: seen.clone(),
            },
        )
        .map_err(Error::internal)?;
        let driver = TwilioSmsDriver::new(twilio_config(), client)?;
        let receipt =
            SmsDriver::send(&driver, SmsMessage::text("+14155552671", "hello world")?).await?;
        assert_eq!(receipt.provider, "twilio");
        assert_eq!(receipt.message_id.as_deref(), Some("SM123"));
        let sent = seen
            .lock()
            .map_err(|error| Error::internal(std::io::Error::other(error.to_string())))?;
        assert_eq!(sent.len(), 1);
        assert!(
            sent[0]
                .0
                .ends_with("/Accounts/AC00000000000000000000000000000000/Messages.json")
        );
        let expected = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD
                .encode("SK00000000000000000000000000000000:test-secret")
        );
        assert_eq!(sent[0].1, expected);
        let body = String::from_utf8_lossy(&sent[0].2);
        assert!(body.contains("To=%2B14155552671"));
        assert!(body.contains("Body=hello+world"));
        Ok(())
    }

    #[tokio::test]
    async fn provider_rejection_is_generic_and_does_not_leak_message_or_credentials() -> Result<()>
    {
        let client = HttpClient::with_transport(
            HttpClientConfig::default(),
            RecordingTransport {
                status: StatusCode::UNAUTHORIZED,
                delay: std::time::Duration::ZERO,
                seen: Arc::new(Mutex::new(Vec::new())),
            },
        )
        .map_err(Error::internal)?;
        let driver = TwilioSmsDriver::new(twilio_config(), client)?;
        let error =
            SmsDriver::send(&driver, SmsMessage::text("+14155552671", "private-code")?).await;
        let rendered = error
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(rendered.contains("SMS provider rejected"));
        assert!(!rendered.contains("private-code"));
        assert!(!rendered.contains("test-secret"));
        Ok(())
    }

    #[tokio::test]
    async fn provider_timeout_is_bounded_and_never_retries_sms() -> Result<()> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let client = HttpClient::with_transport(
            HttpClientConfig {
                timeout: std::time::Duration::from_millis(10),
                ..HttpClientConfig::default()
            },
            RecordingTransport {
                status: StatusCode::CREATED,
                delay: std::time::Duration::from_millis(100),
                seen: seen.clone(),
            },
        )
        .map_err(Error::internal)?;
        let driver = TwilioSmsDriver::new(twilio_config(), client)?;
        let error = SmsDriver::send(&driver, SmsMessage::text("+14155552671", "private")?).await;
        assert!(error.is_err());
        let calls = seen
            .lock()
            .map_err(|error| Error::internal(std::io::Error::other(error.to_string())))?;
        assert_eq!(calls.len(), 1);
        Ok(())
    }

    #[test]
    fn twilio_configuration_debug_redacts_secrets_and_addresses() {
        let debug = format!("{:?}", twilio_config());
        assert!(!debug.contains("test-secret"));
        assert!(!debug.contains("+14155550100"));
    }
}
