use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Map, Value, json};

use crate::mail::{
    MailFuture, MailResult, Message, SentMessage, mailables::Address, message::mailbox, transport,
};

use super::Transport;

pub(in crate::mail) enum Provider {
    Postmark { stream: Option<String> },
    Resend,
    Cloudflare,
    Mailgun { raw: bool },
}

pub(in crate::mail) struct ApiTransport {
    provider: Provider,
    endpoint: String,
    key: reqwest::header::HeaderValue,
    client: reqwest::Client,
}

impl ApiTransport {
    pub(in crate::mail) fn new(
        provider: Provider,
        key: &str,
        endpoint: Option<&str>,
    ) -> MailResult<Self> {
        let endpoint = match (endpoint, &provider) {
            (Some(endpoint), _) => endpoint,
            (None, Provider::Postmark { .. }) => "https://api.postmarkapp.com/email",
            (None, Provider::Resend) => "https://api.resend.com/emails",
            _ => return Err(transport("Mail API endpoint is required.")),
        };
        let endpoint_url =
            reqwest::Url::parse(endpoint).map_err(|_| transport("Invalid mail API endpoint."))?;
        let loopback = endpoint_url.host_str().is_some_and(|host| {
            host.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
        });
        if endpoint_url.host().is_none()
            || !endpoint_url.username().is_empty()
            || endpoint_url.password().is_some()
            || !(endpoint_url.scheme() == "https" || endpoint_url.scheme() == "http" && loopback)
        {
            return Err(transport("Mail API endpoints require HTTPS."));
        }
        let mut key = reqwest::header::HeaderValue::from_str(&match provider {
            Provider::Postmark { .. } => key.to_owned(),
            Provider::Resend | Provider::Cloudflare => format!("Bearer {key}"),
            Provider::Mailgun { .. } => format!("Basic {}", STANDARD.encode(format!("api:{key}"))),
        })
        .map_err(|_| transport("Invalid mail API credential."))?;
        key.set_sensitive(true);
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| transport("Could not configure the mail API client."))?;
        Ok(Self {
            provider,
            endpoint: endpoint.into(),
            key,
            client,
        })
    }

    fn request(&self, message: &Message) -> MailResult<reqwest::RequestBuilder> {
        let data = &message.1;
        if data.has_inline_attachments && !matches!(self.provider, Provider::Mailgun { raw: true })
        {
            return Err(transport(
                "This mail transport requires raw MIME support to send inline attachments.",
            ));
        }
        let envelope = &data.envelope;
        let from_address = envelope
            .from
            .as_ref()
            .ok_or_else(|| transport("Mail API sender is missing."))?;
        let (from, to, cc, bcc, reply_to) = (
            mailbox(from_address.clone())?.to_string(),
            addresses(&envelope.to)?,
            addresses(&envelope.cc)?,
            addresses(&envelope.bcc)?,
            addresses(&envelope.reply_to)?,
        );
        let mut headers = data.headers.text.clone();
        if let Some(id) = &data.headers.message_id {
            headers.insert("Message-ID".into(), id.clone());
        }
        if !data.headers.references.is_empty() {
            headers.insert("References".into(), data.headers.references.join(" "));
        }
        let request = self
            .client
            .post(&self.endpoint)
            .header("Accept", "application/json");
        let (mut payload, request) = match &self.provider {
            Provider::Postmark { stream } => {
                let mut payload = json!({
                    "From": from, "To": to.join(", "), "Cc": cc.join(", "), "Bcc": bcc.join(", "),
                    "ReplyTo": reply_to.join(", "), "Subject": envelope.subject,
                    "Headers": headers.iter().map(|(name, value)| json!({"Name": name, "Value": value})).collect::<Vec<_>>(),
                    "Attachments": data.attachments.iter().map(|attachment| json!({
                        "Name": attachment.name, "ContentType": attachment.mime, "Content": STANDARD.encode(&attachment.bytes),
                    })).collect::<Vec<_>>(),
                });
                let fields = payload.as_object_mut().expect("object payload");
                insert_text(fields, "TextBody", data.text.as_deref());
                insert_text(fields, "HtmlBody", data.html.as_deref());
                insert_text(fields, "MessageStream", stream.as_deref());
                for optional in ["To", "Cc", "Bcc", "ReplyTo"] {
                    if fields[optional].as_str() == Some("") {
                        fields.remove(optional);
                    }
                }
                (
                    payload,
                    request.header("X-Postmark-Server-Token", self.key.clone()),
                )
            }
            Provider::Resend => {
                let mut payload = json!({
                    "from": from, "to": to, "cc": cc, "bcc": bcc, "reply_to": reply_to,
                    "subject": envelope.subject, "headers": headers,
                    "attachments": data.attachments.iter().map(|attachment| json!({
                        "filename": attachment.name, "content_type": attachment.mime, "content": STANDARD.encode(&attachment.bytes),
                    })).collect::<Vec<_>>(),
                });
                let fields = payload.as_object_mut().expect("object payload");
                insert_text(fields, "text", data.text.as_deref());
                insert_text(fields, "html", data.html.as_deref());
                (
                    payload,
                    request.header(reqwest::header::AUTHORIZATION, self.key.clone()),
                )
            }
            Provider::Cloudflare => {
                let mut payload = json!({
                    "from": cloudflare_address(from_address),
                    "to": addresses_only(&envelope.to),
                    "cc": addresses_only(&envelope.cc),
                    "bcc": addresses_only(&envelope.bcc),
                    "subject": envelope.subject,
                    "headers": headers,
                    "attachments": data.attachments.iter().map(|attachment| json!({
                        "filename": attachment.name, "type": attachment.mime,
                        "content": STANDARD.encode(&attachment.bytes), "disposition": "attachment",
                    })).collect::<Vec<_>>(),
                });
                let fields = payload.as_object_mut().expect("object payload");
                if let Some(reply) = envelope.reply_to.first() {
                    fields.insert("reply_to".into(), cloudflare_address(reply));
                }
                insert_text(fields, "text", data.text.as_deref());
                insert_text(fields, "html", data.html.as_deref());
                (
                    payload,
                    request.header(reqwest::header::AUTHORIZATION, self.key.clone()),
                )
            }
            Provider::Mailgun { raw } => {
                use reqwest::multipart::{Form, Part};

                let form = if *raw {
                    Form::new().text("to", message.recipients().join(",")).part(
                        "message",
                        Part::bytes(message.formatted()).file_name("message.mime"),
                    )
                } else {
                    let mut form = Form::new()
                        .text("from", from)
                        .text("subject", envelope.subject.clone());
                    for (field, addresses) in [
                        ("to", to),
                        ("cc", cc),
                        ("bcc", bcc),
                        ("h:Reply-To", reply_to),
                    ] {
                        if !addresses.is_empty() {
                            form = form.text(field, addresses.join(","));
                        }
                    }
                    for (field, value) in [("text", &data.text), ("html", &data.html)] {
                        if let Some(value) = value {
                            form = form.text(field, value.clone());
                        }
                    }
                    for (name, value) in headers {
                        form = form.text(format!("h:{name}"), value);
                    }
                    for attachment in &data.attachments {
                        let part = Part::bytes(attachment.bytes.clone())
                            .file_name(attachment.name.clone())
                            .mime_str(&attachment.mime)
                            .map_err(|_| transport("Invalid Mailgun attachment type."))?;
                        form = form.part("attachment", part);
                    }
                    form
                };
                return Ok(request
                    .header(reqwest::header::AUTHORIZATION, self.key.clone())
                    .multipart(form));
            }
        };
        payload
            .as_object_mut()
            .expect("object payload")
            .retain(|_, value| {
                !value.as_array().is_some_and(Vec::is_empty)
                    && !value.as_object().is_some_and(Map::is_empty)
            });
        Ok(request.json(&payload))
    }
}

impl Transport for ApiTransport {
    fn send<'a>(&'a self, message: &'a Message) -> MailFuture<'a, MailResult<SentMessage>> {
        Box::pin(async move {
            let response = self
                .request(message)?
                .send()
                .await
                .map_err(|_| transport("Mail API connection or request failed."))?;
            let status = response.status();
            let accepted = match self.provider {
                Provider::Cloudflare | Provider::Mailgun { .. } => {
                    status == reqwest::StatusCode::OK
                }
                _ => status.is_success(),
            };
            if !accepted {
                return Err(transport(format!(
                    "Mail API rejected the request ({status})."
                )));
            }
            let result: Value = response
                .json()
                .await
                .map_err(|_| transport("Mail API returned an invalid receipt."))?;
            let (id, header) = match self.provider {
                Provider::Postmark { .. } if result["ErrorCode"].as_i64() == Some(0) => {
                    (&result["MessageID"], "X-Postmark-Message-ID")
                }
                Provider::Postmark { .. } => {
                    return Err(transport("Postmark rejected the request."));
                }
                Provider::Resend => (&result["id"], "X-Resend-Email-ID"),
                Provider::Mailgun { .. } => (&result["id"], "X-Message-ID"),
                Provider::Cloudflare if result["success"].as_bool() != Some(false) => {
                    return Ok(SentMessage {
                        message: message.clone(),
                    });
                }
                Provider::Cloudflare => return Err(transport("Cloudflare rejected the request.")),
            };
            let id = id
                .as_str()
                .filter(|id| !id.is_empty() && !id.chars().any(char::is_control))
                .ok_or_else(|| transport("Mail API receipt did not contain a valid message ID."))?;
            let mut sent = message.clone();
            sent.0
                .headers_mut()
                .insert_raw(lettre::message::header::HeaderValue::new(
                    lettre::message::header::HeaderName::new_from_ascii_str(header),
                    id.into(),
                ));
            Ok(SentMessage { message: sent })
        })
    }
}

fn addresses(addresses: &[Address]) -> MailResult<Vec<String>> {
    addresses
        .iter()
        .cloned()
        .map(|address| mailbox(address).map(|mailbox| mailbox.to_string()))
        .collect()
}

fn insert_text(fields: &mut Map<String, Value>, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        fields.insert(name.into(), json!(value));
    }
}

fn cloudflare_address(address: &Address) -> Value {
    match &address.name {
        Some(name) if !name.is_empty() => json!({"name": name, "address": address.address}),
        _ => json!(address.address),
    }
}

fn addresses_only(addresses: &[Address]) -> Vec<&str> {
    addresses
        .iter()
        .map(|address| address.address.as_str())
        .collect()
}
