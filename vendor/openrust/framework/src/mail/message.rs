use std::{fmt::Write as _, sync::Arc};

use lettre::message::{Mailbox, MultiPart, SinglePart, header};

use crate::view::Factory;

use super::{
    MailResult, Markdown, MessageDraft, composition, inline_attachment,
    mailables::{Address, Attachment, Content, Envelope, Headers, InlineAttachment},
};

/// A composed email ready for transport.
#[derive(Clone)]
pub struct Message(pub(super) lettre::Message, pub(super) Arc<ApiMessage>);

pub(super) struct ApiMessage {
    pub envelope: Envelope,
    pub text: Option<String>,
    pub html: Option<String>,
    pub attachments: Vec<Attachment>,
    pub inline_attachments: Vec<InlineAttachment>,
    pub headers: Headers,
    tracking_headers: String,
}

impl Message {
    pub fn formatted(&self) -> Vec<u8> {
        let mime = self.0.formatted();
        if self.1.tracking_headers.is_empty() {
            return mime;
        }
        let mut formatted = Vec::with_capacity(self.1.tracking_headers.len() + mime.len());
        formatted.extend_from_slice(self.1.tracking_headers.as_bytes());
        formatted.extend_from_slice(&mime);
        formatted
    }

    pub fn envelope(&self) -> &Envelope {
        &self.1.envelope
    }

    pub fn recipients(&self) -> Vec<String> {
        self.0
            .envelope()
            .to()
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    pub(super) fn compose(draft: MessageDraft) -> MailResult<Self> {
        let MessageDraft {
            envelope,
            headers,
            text,
            html,
            attachments,
            inline_attachments,
            ..
        } = draft;
        let api_envelope = envelope.clone();
        let api_headers = headers.clone();
        let tracking_headers = tracking_headers(&envelope)?;
        let mut builder = lettre::Message::builder().subject(header_text(envelope.subject)?);
        let from = envelope
            .from
            .ok_or_else(|| composition("The sender address is required."))?;
        builder = builder.from(mailbox(from)?);
        for address in envelope.to {
            builder = builder.to(mailbox(address)?);
        }
        for address in envelope.cc {
            builder = builder.cc(mailbox(address)?);
        }
        for address in envelope.bcc {
            builder = builder.bcc(mailbox(address)?);
        }
        for address in envelope.reply_to {
            builder = builder.reply_to(mailbox(address)?);
        }
        if let Some(id) = headers.message_id {
            builder = builder.message_id(Some(header_text(id)?));
        }
        if !headers.references.is_empty() {
            builder = builder.references(header_text(headers.references.join(" "))?);
        }
        for (name, value) in headers.text {
            if matches!(
                name.to_ascii_lowercase().as_str(),
                "from"
                    | "sender"
                    | "to"
                    | "cc"
                    | "bcc"
                    | "reply-to"
                    | "subject"
                    | "message-id"
                    | "references"
                    | "mime-version"
                    | "date"
            ) || name.to_ascii_lowercase().starts_with("content-")
            {
                return Err(composition("A reserved mail header cannot be overridden."));
            }
            if !name.bytes().all(|byte| byte.is_ascii_graphic()) {
                return Err(composition("Invalid mail header name."));
            }
            let name = header::HeaderName::new_from_ascii(name)
                .map_err(|_| composition("Invalid mail header name."))?;
            builder = builder.raw_header(header::HeaderValue::new(name, header_text(value)?));
        }
        let body = if inline_attachments.is_empty() {
            match (text.clone(), html.clone()) {
                (Some(text), Some(html)) => {
                    Body::Multipart(MultiPart::alternative_plain_html(text, html))
                }
                (Some(text), None) => Body::Single(SinglePart::plain(text)),
                (None, Some(html)) => Body::Single(SinglePart::html(html)),
                (None, None) => return Err(composition("The mailable has no content.")),
            }
        } else {
            let html = html
                .clone()
                .ok_or_else(|| composition("Inline attachments require an HTML body."))?;
            let mut related = MultiPart::related().singlepart(SinglePart::html(html));
            for (inline, mime) in inline_attachments
                .iter()
                .zip(inline_attachment::types(&inline_attachments)?)
            {
                related = related.singlepart(
                    lettre::message::Attachment::new_inline_with_name(
                        inline.content_id.clone(),
                        header_text(inline.attachment.name.clone())?,
                    )
                    .body(inline.attachment.bytes.clone(), mime),
                );
            }
            Body::Multipart(match &text {
                Some(text) => MultiPart::alternative()
                    .singlepart(SinglePart::plain(text.clone()))
                    .multipart(related),
                None => related,
            })
        };
        let message = if attachments.is_empty() {
            match body {
                Body::Single(part) => builder.singlepart(part),
                Body::Multipart(part) => builder.multipart(part),
            }
        } else {
            let mut mixed = match body {
                Body::Single(part) => MultiPart::mixed().singlepart(part),
                Body::Multipart(part) => MultiPart::mixed().multipart(part),
            };
            for attachment in &attachments {
                let mime = header::ContentType::parse(&attachment.mime)
                    .map_err(|_| composition("Invalid attachment media type."))?;
                mixed = mixed.singlepart(
                    lettre::message::Attachment::new(header_text(attachment.name.clone())?)
                        .body(attachment.bytes.clone(), mime),
                );
            }
            builder.multipart(mixed)
        };
        message
            .map(|message| {
                Self(
                    message,
                    Arc::new(ApiMessage {
                        envelope: api_envelope,
                        headers: api_headers,
                        tracking_headers,
                        text,
                        html,
                        attachments,
                        inline_attachments,
                    }),
                )
            })
            .map_err(|_| composition("A valid sender and at least one recipient are required."))
    }
}

impl MessageDraft {
    pub(super) fn render(
        envelope: Envelope,
        headers: Headers,
        mut content: Content,
        views: &Factory,
        markdown: &Markdown,
    ) -> MailResult<Self> {
        let explicit_text = render(
            content.text.take(),
            content.text_view.as_deref(),
            &content.with,
            views,
        )?;
        let html = render_html(&mut content, views, markdown)?;
        let text = match (explicit_text, content.markdown.as_deref()) {
            (None, Some(view)) => Some(markdown.render_text(view, &content.with, views)?),
            (text, _) => text,
        };
        Ok(Self {
            envelope,
            headers,
            text,
            html,
            attachments: Vec::new(),
            attachment_sources: Vec::new(),
            inline_attachments: Vec::new(),
        })
    }
}

fn tracking_headers(envelope: &Envelope) -> MailResult<String> {
    let tags = envelope.tags.iter().map(|tag| ("X-Tag".into(), tag));
    let metadata = envelope
        .metadata
        .iter()
        .map(|(key, value)| (format!("X-Metadata-{key}"), value));
    let mut formatted = String::new();
    let mut headers = header::Headers::new();
    for (name, value) in tags.chain(metadata) {
        if !name.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(composition("Invalid mail tracking header name."));
        }
        let name = header::HeaderName::new_from_ascii(name)
            .map_err(|_| composition("Invalid mail tracking header name."))?;
        // Lettre stores only one value per name, but MIME allows repeated tags.
        headers.clear();
        headers.insert_raw(header::HeaderValue::new(name, header_text(value.clone())?));
        write!(formatted, "{headers}").expect("writing to a string cannot fail");
    }
    Ok(formatted)
}

enum Body {
    Single(SinglePart),
    Multipart(MultiPart),
}

pub(super) enum Preview {
    Html(String),
    Text(String),
}

impl Preview {
    pub(super) fn render(
        mut content: Content,
        views: &Factory,
        markdown: &Markdown,
        mailable: &dyn super::Mailable,
    ) -> MailResult<Self> {
        if let Some(html) = render_html(&mut content, views, markdown)? {
            return inline_attachment::preview(html, &mailable.inline_attachments()?)
                .map(Self::Html);
        }
        render(
            content.text,
            content.text_view.as_deref(),
            &content.with,
            views,
        )?
        .map(Self::Text)
        .ok_or_else(|| composition("The mailable has no content."))
    }
}

fn render_html(
    content: &mut Content,
    views: &Factory,
    markdown: &Markdown,
) -> MailResult<Option<String>> {
    match content.markdown.as_deref() {
        Some(view) => markdown
            .render_html(view, &content.with, views, content.theme.as_deref())
            .map(Some),
        None => render(
            content.html.take(),
            content.view.as_deref(),
            &content.with,
            views,
        ),
    }
}

fn render(
    literal: Option<String>,
    view: Option<&str>,
    data: &crate::view::ViewData,
    views: &Factory,
) -> MailResult<Option<String>> {
    match view {
        Some(view) => views.render(view, data).map(Some).map_err(composition),
        None => Ok(literal),
    }
}

fn header_text(value: String) -> MailResult<String> {
    if value.chars().any(char::is_control) {
        return Err(composition(
            "Mail header values cannot contain control characters.",
        ));
    }
    Ok(value)
}

pub(super) fn mailbox(address: Address) -> MailResult<Mailbox> {
    let address_value = address
        .address
        .parse()
        .map_err(|_| composition("Invalid mailbox address."))?;
    Ok(Mailbox::new(
        address.name.map(header_text).transpose()?,
        address_value,
    ))
}

/// A message accepted by the selected transport.
#[derive(Clone)]
pub struct SentMessage {
    pub message: Message,
}
