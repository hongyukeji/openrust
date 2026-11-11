use std::time::Duration;

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
    transport::smtp::{
        authentication::Credentials,
        client::{Tls, TlsParameters},
        extension::ClientId,
    },
};
use percent_encoding::percent_decode_str;
use serde::Deserialize;

use crate::mail::{MailFuture, MailResult, Message, SentMessage, transport};

use super::Transport;

/// An asynchronous SMTP transport using TLS when configured or advertised.
pub struct SmtpTransport(AsyncSmtpTransport<Tokio1Executor>);

#[derive(Deserialize)]
pub(crate) struct Settings {
    scheme: Option<String>,
    url: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    username: Option<String>,
    password: Option<String>,
    timeout: Option<u64>,
    local_domain: Option<String>,
}

impl SmtpTransport {
    pub(crate) fn new(mut settings: Settings) -> MailResult<Self> {
        if let Some(url) = settings.url.as_deref().filter(|url| !url.is_empty()) {
            let url = url::Url::parse(url).map_err(|_| transport("Invalid SMTP URL."))?;
            settings.scheme = Some(url.scheme().into());
            settings.host = url.host_str().map(str::to_owned);
            if let Some(port) = url.port() {
                settings.port = Some(port);
            }
            if !url.username().is_empty() {
                settings.username = Some(decode(url.username())?);
            }
            if let Some(password) = url.password() {
                settings.password = Some(decode(password)?);
            }
        }
        let host = settings
            .host
            .filter(|host| !host.is_empty())
            .ok_or_else(|| transport("SMTP host is required."))?;
        let port = settings.port.unwrap_or(2525);
        if port == 0 {
            return Err(transport("SMTP port must be positive."));
        }
        let credentials = match (settings.username, settings.password) {
            (Some(username), Some(password)) => Some(Credentials::new(username, password)),
            (None, None) => None,
            _ => {
                return Err(transport(
                    "SMTP username and password must be set together.",
                ));
            }
        };
        let tls = TlsParameters::new(host.clone())
            .map_err(|_| transport("Invalid SMTP TLS configuration."))?;
        let scheme =
            settings
                .scheme
                .as_deref()
                .unwrap_or(if port == 465 { "smtps" } else { "smtp" });
        let tls = match scheme {
            "smtps" => Tls::Wrapper(tls),
            "smtp" if credentials.is_some() => Tls::Required(tls),
            "smtp" => Tls::Opportunistic(tls),
            _ => return Err(transport("Unsupported SMTP scheme.")),
        };
        let mut builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
            .port(port)
            .tls(tls);
        if let Some(seconds) = settings.timeout {
            builder = builder.timeout(Some(Duration::from_secs(seconds)));
        }
        if let Some(domain) = settings.local_domain {
            if domain.is_empty() || domain.chars().any(|c| c.is_control() || c.is_whitespace()) {
                return Err(transport("Invalid SMTP EHLO domain."));
            }
            builder = builder.hello_name(ClientId::Domain(domain));
        }
        if let Some(credentials) = credentials {
            builder = builder.credentials(credentials);
        }
        Ok(Self(builder.build()))
    }
}

impl Transport for SmtpTransport {
    fn send<'a>(&'a self, message: &'a Message) -> MailFuture<'a, MailResult<SentMessage>> {
        Box::pin(async move {
            self.0.send(message.0.clone()).await.map_err(|source| {
                source.status().map_or_else(
                    || transport("SMTP connection or protocol failed."),
                    |status| transport(format!("SMTP server rejected the message ({status}).")),
                )
            })?;
            Ok(SentMessage {
                message: message.clone(),
            })
        })
    }
}

fn decode(value: &str) -> MailResult<String> {
    percent_decode_str(value)
        .decode_utf8()
        .map(|value| value.into_owned())
        .map_err(|_| transport("Invalid SMTP URL credentials."))
}
