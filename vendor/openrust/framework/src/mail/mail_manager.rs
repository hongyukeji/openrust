use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use crate::{
    container::Container,
    contracts::{
        config::Repository,
        container::Container as _,
        mail::{Factory, Mailer as Contract},
    },
    log::LogManager,
    view,
};

use super::{
    MailError, MailResult, Mailer, PendingMail,
    mailables::Address,
    transport::{
        ApiProvider, ApiTransport, ArrayTransport, CompositeTransport, LogTransport,
        SendmailTransport, SesTransport, SmtpTransport, Transport,
    },
};

type ResolvedTransport = (Arc<dyn Transport>, Option<Arc<ArrayTransport>>);

/// Resolves application-local mailers by name.
pub struct MailManager {
    config: Arc<dyn Repository>,
    owner: Weak<Container>,
    mailers: Mutex<HashMap<String, Arc<Mailer>>>,
}

impl MailManager {
    pub(crate) fn new(config: Arc<dyn Repository>, owner: Weak<Container>) -> Self {
        Self {
            config,
            owner,
            mailers: Mutex::default(),
        }
    }

    pub fn mailer(&self, name: Option<&str>) -> MailResult<Arc<Mailer>> {
        let default = self.config.get("mail.default");
        let name = name
            .filter(|name| !name.is_empty())
            .or_else(|| default.as_ref().and_then(serde_json::Value::as_str))
            .unwrap_or("log");
        let mut mailers = self.mailers.lock().expect("mail manager lock poisoned");
        if let Some(mailer) = mailers.get(name) {
            return Ok(mailer.clone());
        }
        let container = self
            .owner
            .upgrade()
            .ok_or_else(|| super::transport("Mail application is unavailable."))?;
        let (transport, captured) = self.resolve_transport(&container, name, &mut Vec::new())?;
        let from = self.config.get("mail.from").and_then(|from| {
            Some(Address {
                address: from["address"].as_str()?.into(),
                name: from["name"].as_str().map(str::to_owned),
            })
        });
        let views = container
            .make::<view::Factory>()
            .map_err(super::composition)?;
        let mailer = Arc::new(Mailer {
            name: name.into(),
            from,
            views,
            transport,
            captured,
        });
        mailers.insert(name.into(), mailer.clone());
        Ok(mailer)
    }

    fn resolve_transport(
        &self,
        container: &Container,
        name: &str,
        ancestors: &mut Vec<String>,
    ) -> MailResult<ResolvedTransport> {
        if ancestors.iter().any(|ancestor| ancestor == name) {
            return Err(super::transport("Circular mail transport configuration."));
        }
        let config = self
            .config
            .get(&format!("mail.mailers.{name}"))
            .or_else(|| (name == "log").then(|| serde_json::json!({"transport":"log"})))
            .ok_or_else(|| MailError::UnknownMailer { name: name.into() })?;
        let mut captured = None;
        let transport: Arc<dyn Transport> = match config["transport"].as_str() {
            Some("smtp") => Arc::new(SmtpTransport::new(
                serde_json::from_value(config)
                    .map_err(|_| super::transport("Invalid SMTP configuration."))?,
            )?),
            Some("log") => {
                let logs = container.make::<LogManager>().map_err(super::transport)?;
                Arc::new(LogTransport(
                    logs.channel(config["channel"].as_str())
                        .map_err(super::transport)?,
                ))
            }
            Some("array") => {
                let array = Arc::new(ArrayTransport::default());
                captured = Some(array.clone());
                array
            }
            Some("sendmail") => Arc::new(SendmailTransport::new(
                config["path"]
                    .as_str()
                    .unwrap_or("/usr/sbin/sendmail -bs -i"),
            )?),
            Some("ses") => {
                let service = self.config.get("services.ses").unwrap_or_default();
                let mut settings = service.as_object().cloned().unwrap_or_default();
                settings.extend(config.as_object().expect("mailer transport object").clone());
                Arc::new(SesTransport::new(
                    serde_json::from_value(serde_json::Value::Object(settings))
                        .map_err(|_| super::transport("Invalid SES configuration."))?,
                )?)
            }
            Some(kind @ ("postmark" | "resend")) => {
                let service = self
                    .config
                    .get(&format!("services.{kind}"))
                    .unwrap_or_default();
                let (provider, key) = if kind == "postmark" {
                    (
                        ApiProvider::Postmark {
                            stream: config["message_stream_id"].as_str().map(str::to_owned),
                        },
                        config["token"]
                            .as_str()
                            .or_else(|| config["key"].as_str())
                            .or_else(|| service["token"].as_str())
                            .or_else(|| service["key"].as_str()),
                    )
                } else {
                    (
                        ApiProvider::Resend,
                        config["key"].as_str().or_else(|| service["key"].as_str()),
                    )
                };
                let key = key
                    .filter(|key| !key.trim().is_empty())
                    .ok_or_else(|| super::transport("Mail API credential is required."))?;
                Arc::new(ApiTransport::new(
                    provider,
                    key,
                    config["endpoint"].as_str(),
                )?)
            }
            Some(kind @ ("failover" | "roundrobin")) => {
                let children: Vec<String> = serde_json::from_value(config["mailers"].clone())
                    .map_err(|_| super::transport("Invalid composite mail transport list."))?;
                if children.is_empty() {
                    return Err(super::transport(
                        "Composite mail transport requires at least one mailer.",
                    ));
                }
                let retry_after = if config["retry_after"].is_null() {
                    60
                } else {
                    config["retry_after"].as_u64().ok_or_else(|| {
                        super::transport("Invalid composite mail transport retry interval.")
                    })?
                };
                ancestors.push(name.into());
                let transports = children
                    .iter()
                    .map(|child| {
                        self.resolve_transport(container, child, ancestors)
                            .map(|(transport, _)| transport)
                    })
                    .collect::<MailResult<Vec<_>>>();
                ancestors.pop();
                Arc::new(CompositeTransport::new(
                    transports?,
                    Duration::from_secs(retry_after),
                    kind == "roundrobin",
                ))
            }
            _ => return Err(super::transport("Unsupported mail transport.")),
        };
        Ok((transport, captured))
    }

    pub fn to(self: &Arc<Self>, address: impl Into<Address>) -> PendingMail {
        PendingMail::new(self.clone(), address.into())
    }

    pub fn purge(&self, name: &str) {
        self.mailers
            .lock()
            .expect("mail manager lock poisoned")
            .remove(name);
    }
}

impl Factory for MailManager {
    fn mailer(&self, name: Option<&str>) -> MailResult<Arc<dyn Contract>> {
        Ok(self.mailer(name)?)
    }
}
