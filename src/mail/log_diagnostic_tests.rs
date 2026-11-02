use std::io::Write;
use std::sync::{Arc, Mutex};

use futures_util::FutureExt;

use super::*;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[test]
fn log_transport_records_acceptance_without_message_data() -> Result<()> {
    let message = Message::builder()
        .from(
            "sender-sentinel@example.test"
                .parse()
                .map_err(Error::internal)?,
        )
        .to("recipient-sentinel@example.test"
            .parse()
            .map_err(Error::internal)?)
        .subject("subject-sentinel")
        .body("body-sentinel token-sentinel".to_owned())
        .map_err(Error::internal)?;
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(capture.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        assert!(matches!(
            LogMailer.send(message).now_or_never(),
            Some(Ok(()))
        ));
    });
    let bytes = capture
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let output = String::from_utf8(bytes).map_err(Error::internal)?;
    assert!(output.contains("email accepted by the log mail driver"));
    for secret in [
        "sender-sentinel",
        "recipient-sentinel",
        "subject-sentinel",
        "body-sentinel",
        "token-sentinel",
    ] {
        assert!(!output.contains(secret));
    }
    Ok(())
}

fn initialized_failure(config: &MailConfig) -> Result<Error> {
    let source = configured_mailer(config)
        .err()
        .ok_or_else(|| Error::custom("mail_config_accepted", "应拒绝无效配置"))?;
    Ok(Error::service_initialization::<Mailer>(source))
}

#[test]
fn unknown_driver_diagnostic_identifies_key_without_echoing_selection() -> Result<()> {
    let config = MailConfig {
        driver: "smtp://user:password-sentinel@private.example".into(),
        ..MailConfig::default()
    };
    let error = initialized_failure(&config)?;
    assert!(
        error
            .to_string()
            .contains("mail.driver [unsupported_driver]")
    );
    assert!(!format!("{error} {error:?}").contains("password-sentinel"));
    Ok(())
}

#[cfg(not(feature = "mail-smtp"))]
#[test]
fn missing_smtp_feature_has_distinct_safe_diagnostic() -> Result<()> {
    let config = MailConfig {
        driver: "smtp".into(),
        ..MailConfig::default()
    };
    let error = initialized_failure(&config)?;
    assert!(
        error
            .to_string()
            .contains("mail.driver [missing_feature:mail-smtp]")
    );
    Ok(())
}

#[cfg(feature = "mail-smtp")]
#[test]
fn smtp_configuration_failures_have_distinct_safe_keys_and_causes() -> Result<()> {
    let missing_host = MailConfig {
        driver: "smtp".into(),
        ..MailConfig::default()
    };
    let credentials = MailConfig {
        smtp_host: Some("smtp.example.test".into()),
        smtp_username: Some("credential-sentinel".into()),
        ..missing_host.clone()
    };
    let remote_plaintext = MailConfig {
        smtp_starttls: false,
        smtp_username: None,
        ..credentials.clone()
    };
    let authenticated_plaintext = MailConfig {
        smtp_host: Some("localhost".into()),
        smtp_username: Some("credential-sentinel".into()),
        smtp_password: Some("password-sentinel".into()),
        ..remote_plaintext.clone()
    };
    for (config, expected) in [
        (missing_host, "mail.smtp_host [required]"),
        (
            credentials,
            "mail.smtp_username [credentials_pair_required]",
        ),
        (remote_plaintext, "mail.smtp_starttls [remote_tls_required]"),
        (
            authenticated_plaintext,
            "mail.smtp_starttls [credentials_tls_required]",
        ),
    ] {
        let error = initialized_failure(&config)?;
        assert!(error.to_string().contains(expected));
        for secret in ["credential-sentinel", "password-sentinel"] {
            assert!(!format!("{error} {error:?}").contains(secret));
        }
    }
    Ok(())
}
