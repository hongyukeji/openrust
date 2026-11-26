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
    events::Dispatcher,
    log::LogManager,
    support::facades::Lang,
    view,
};

use super::{
    MailError, MailFake, MailResult, Mailable, Mailer, Markdown, MessageDraft, PendingMail,
    mail_fake::FakeState,
    mailables::Address,
    message::{Preview, render_bodies},
    message_customization::MessageCustomizers,
    transport::{
        ApiProvider, ApiTransport, ArrayTransport, CompositeTransport, LogTransport,
        SendmailTransport, SesTransport, SmtpTransport, Transport,
    },
};

type ResolvedTransport = (Arc<dyn Transport>, Option<Arc<ArrayTransport>>);

type TransportCreator = dyn Fn(&serde_json::Value) -> MailResult<Arc<dyn Transport>> + Send + Sync;

/// Resolves application-local mailers by name.
pub struct MailManager {
    base: std::path::PathBuf,
    config: Arc<dyn Repository>,
    owner: Weak<Container>,
    mailers: Mutex<HashMap<String, Arc<Mailer>>>,
    fake: FakeState,
    creators: Mutex<HashMap<String, Arc<TransportCreator>>>,
    customizers: Arc<MessageCustomizers>,
}

impl MailManager {
    pub(super) async fn preview(&self, mailable: &dyn Mailable) -> MailResult<Preview> {
        self.render_mailable(mailable, |views, markdown| {
            Preview::render(mailable.content()?, views, markdown, mailable)
        })
        .await
    }

    pub(super) async fn render_for_assertions(
        &self,
        mailable: &dyn Mailable,
    ) -> MailResult<(String, String)> {
        self.render_mailable(mailable, |views, markdown| {
            let (html, text) = render_bodies(mailable.content()?, views, markdown)?;
            let text = text.unwrap_or_default();
            let html = match html {
                Some(html) => {
                    super::inline_attachment::preview(html, &mailable.inline_attachments()?)?
                }
                None => text.clone(),
            };
            Ok((html, text))
        })
        .await
    }

    async fn render_mailable<T>(
        &self,
        mailable: &dyn Mailable,
        render: impl FnOnce(&view::Factory, &Markdown) -> MailResult<T> + Send,
    ) -> MailResult<T> {
        let container = self
            .owner
            .upgrade()
            .ok_or_else(|| super::composition("Mail application is unavailable."))?;
        let views = container
            .make::<view::Factory>()
            .map_err(super::composition)?;
        let markdown = Markdown::from_config(self.config.as_ref(), &self.base)?;
        let render = || render(&views, &markdown);
        match mailable.locale() {
            Some(locale) => Lang::with_locale(locale, async { render() })
                .await
                .map_err(super::composition)?,
            None => render(),
        }
    }

    pub(crate) fn new(
        config: Arc<dyn Repository>,
        owner: Weak<Container>,
        base: std::path::PathBuf,
    ) -> Self {
        Self {
            base,
            config,
            owner,
            mailers: Mutex::default(),
            fake: FakeState::default(),
            creators: Mutex::default(),
            customizers: Arc::default(),
        }
    }

    pub(crate) fn is_faking(&self) -> bool {
        self.fake.read().expect("mail fake lock poisoned").is_some()
    }

    pub fn fake(&self) -> Arc<MailFake> {
        let fake = Arc::new(MailFake::default());
        *self.fake.write().expect("mail fake lock poisoned") = Some(fake.clone());
        fake
    }

    /// Registers a transport factory for the corresponding `transport` config value.
    pub fn extend(
        &self,
        name: impl Into<String>,
        creator: impl Fn(&serde_json::Value) -> MailResult<Arc<dyn Transport>> + Send + Sync + 'static,
    ) {
        self.creators
            .lock()
            .expect("mail transport factories lock poisoned")
            .insert(name.into(), Arc::new(creator));
    }

    fn global_address(&self, key: &str) -> Option<Address> {
        let value = self.config.get(&format!("mail.{key}"))?;
        Some(Address {
            address: value["address"].as_str()?.to_owned(),
            name: value["name"].as_str().map(str::to_owned),
        })
    }

    pub fn mailer(&self, name: Option<&str>) -> MailResult<Arc<Mailer>> {
        let default = self.config.get("mail.default");
        let name = name
            .filter(|name| !name.is_empty())
            .or_else(|| default.as_ref().and_then(serde_json::Value::as_str))
            .unwrap_or("log");
        if let Some(mailer) = self
            .mailers
            .lock()
            .expect("mail manager lock poisoned")
            .get(name)
        {
            return Ok(mailer.clone());
        }
        let container = self
            .owner
            .upgrade()
            .ok_or_else(|| super::transport("Mail application is unavailable."))?;
        let faking = self.fake.read().expect("mail fake lock poisoned").is_some();
        let (transport, captured): ResolvedTransport = if faking {
            (Arc::new(ArrayTransport::default()), None)
        } else {
            self.resolve_transport(&container, name, &mut Vec::new())?
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
            defaults: Mutex::new(super::address_defaults::Defaults {
                from: None,
                to: self.global_address("to"),
                reply_to: self.global_address("reply_to"),
                return_path: self
                    .global_address("return_path")
                    .map(|address| address.address),
            }),
            owner: self.owner.clone(),
            views,
            markdown: Markdown::from_config(self.config.as_ref(), &self.base)?,
            events: container.make::<Dispatcher>().map_err(super::composition)?,
            transport,
            captured,
            fake: self.fake.clone(),
            customizers: self.customizers.clone(),
        });
        if faking {
            return Ok(mailer);
        }
        Ok(self
            .mailers
            .lock()
            .expect("mail manager lock poisoned")
            .entry(name.into())
            .or_insert(mailer)
            .clone())
    }

    /// Registers a named message callback in this application's mail manager.
    pub fn register_message_customizer(
        &self,
        name: impl Into<String>,
        callback: impl Fn(&mut MessageDraft, &serde_json::Value) -> MailResult<()>
        + Send
        + Sync
        + 'static,
    ) -> MailResult<()> {
        self.customizers.register(name.into(), Arc::new(callback))
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
        let kind = config["transport"].as_str();
        let creator = kind.and_then(|kind| {
            self.creators
                .lock()
                .expect("mail transport factories lock poisoned")
                .get(kind)
                .cloned()
        });
        if let Some(creator) = creator {
            return Ok((creator(&config)?, None));
        }
        let mut captured = None;
        let transport: Arc<dyn Transport> = match kind {
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
            Some(kind @ ("ses" | "ses-v2")) => {
                let service = self.config.get("services.ses").unwrap_or_default();
                let mut settings = service.as_object().cloned().unwrap_or_default();
                settings.extend(config.as_object().expect("mailer transport object").clone());
                Arc::new(SesTransport::new(
                    serde_json::from_value(serde_json::Value::Object(settings))
                        .map_err(|_| super::transport("Invalid SES configuration."))?,
                    kind == "ses-v2",
                )?)
            }
            Some("cloudflare") => {
                let service = self.config.get("services.cloudflare").unwrap_or_default();
                let account = config["account_id"]
                    .as_str()
                    .or_else(|| service["account_id"].as_str())
                    .filter(|account| {
                        !account.is_empty()
                            && account
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                    })
                    .ok_or_else(|| super::transport("Cloudflare account ID is required."))?;
                let key = config["token"]
                    .as_str()
                    .or_else(|| config["key"].as_str())
                    .or_else(|| service["token"].as_str())
                    .or_else(|| service["key"].as_str())
                    .filter(|key| !key.trim().is_empty())
                    .ok_or_else(|| super::transport("Cloudflare API credential is required."))?;
                let default_endpoint = format!(
                    "https://api.cloudflare.com/client/v4/accounts/{account}/email/sending/send"
                );
                Arc::new(ApiTransport::new(
                    ApiProvider::Cloudflare,
                    key,
                    Some(config["endpoint"].as_str().unwrap_or(&default_endpoint)),
                )?)
            }
            Some("mailgun") => self.mailgun_transport(&config)?,
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

    fn mailgun_transport(&self, config: &serde_json::Value) -> MailResult<Arc<dyn Transport>> {
        let service = self.config.get("services.mailgun").unwrap_or_default();
        let settings = if config["secret"].is_string() {
            config
        } else {
            &service
        };
        let secret = settings["secret"]
            .as_str()
            .filter(|secret| !secret.trim().is_empty())
            .ok_or_else(|| super::transport("Mailgun credential is required."))?;
        let domain = settings["domain"]
            .as_str()
            .filter(|domain| !domain.is_empty())
            .ok_or_else(|| super::transport("Mailgun domain is required."))?;
        let scheme = settings["scheme"].as_str().unwrap_or("https");
        let region = match settings["region"].as_str() {
            Some("eu") => ".eu",
            Some("us") | None => "",
            _ => return Err(super::transport("Unsupported Mailgun region.")),
        };
        match scheme {
            "smtp" | "smtps" => {
                let config = serde_json::json!({
                    "scheme": scheme, "host": format!("smtp{region}.mailgun.org"),
                    "port": if scheme == "smtps" { 465 } else { 587 },
                    "username": secret, "password": domain,
                });
                Ok(Arc::new(SmtpTransport::new(
                    serde_json::from_value(config)
                        .map_err(|_| super::transport("Invalid Mailgun SMTP configuration."))?,
                )?))
            }
            "https" | "api" => {
                if !domain
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
                {
                    return Err(super::transport("Invalid Mailgun domain."));
                }
                let host = format!("https://api{region}.mailgun.net");
                let endpoint = settings["endpoint"]
                    .as_str()
                    .filter(|endpoint| *endpoint != "default");
                let endpoint = match endpoint {
                    Some(endpoint) if endpoint.contains("://") => endpoint.into(),
                    Some(endpoint) => format!("https://{endpoint}"),
                    None => host,
                };
                let raw = scheme == "https";
                let suffix = if raw { "messages.mime" } else { "messages" };
                let mut url = reqwest::Url::parse(&endpoint)
                    .map_err(|_| super::transport("Invalid Mailgun endpoint."))?;
                if url.query().is_some() || url.fragment().is_some() {
                    return Err(super::transport("Invalid Mailgun endpoint."));
                }
                url.path_segments_mut()
                    .map_err(|_| super::transport("Invalid Mailgun endpoint."))?
                    .pop_if_empty()
                    .extend(["v3", domain, suffix]);
                Ok(Arc::new(ApiTransport::new(
                    ApiProvider::Mailgun { raw },
                    secret,
                    Some(url.as_str()),
                )?))
            }
            _ => Err(super::transport("Unsupported Mailgun scheme.")),
        }
    }

    pub fn to(self: &Arc<Self>, address: impl Into<Address>) -> PendingMail {
        PendingMail::new(self.clone(), address.into())
    }

    /// 清除所有已解析邮件实例，下一次访问按配置重新解析。
    pub fn forget_mailers(&self) -> &Self {
        self.mailers.lock().expect("邮件实例缓存锁已损坏").clear();
        self
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
