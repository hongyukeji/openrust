use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
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
    transport::{ArrayTransport, LogTransport, SmtpTransport, Transport},
};

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
        let config = self
            .config
            .get(&format!("mail.mailers.{name}"))
            .or_else(|| (name == "log").then(|| serde_json::json!({"transport":"log"})))
            .ok_or_else(|| MailError::UnknownMailer { name: name.into() })?;
        let container = self
            .owner
            .upgrade()
            .ok_or_else(|| super::transport("Mail application is unavailable."))?;
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
            _ => return Err(super::transport("Unsupported mail transport.")),
        };
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
