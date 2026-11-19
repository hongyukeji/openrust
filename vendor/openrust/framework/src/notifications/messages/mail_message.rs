use crate::view::ViewValue as Value;

use crate::{
    mail::{
        MailError, MailResult, Mailable, MessageCustomization,
        mailables::{Address, Attachment, AttachmentSource, Content, Envelope, Headers},
    },
    support::facades::{Config, Lang},
    view::ViewData,
};

pub(crate) const EMAIL_VIEW: &str = include_str!("../resources/views/email.blade.html");
pub(crate) const VIEWS: &[(&str, &str)] = &[("email", EMAIL_VIEW)];

const EMAIL_TEMPLATE: &str = "notifications::email";

const SUBCOPY: &str = concat!(
    "If you're having trouble clicking the \":actionText\" button, copy and paste the URL below\n",
    "into your web browser:"
);

/// The content and addressing options of a mail notification.
#[derive(Default)]
pub struct MailMessage {
    pub envelope: Envelope,
    pub content: Content,
    pub attachments: Vec<Attachment>,
    pub attachment_sources: Vec<AttachmentSource>,
    pub headers: Headers,
    pub mailer: Option<String>,
}

#[allow(
    non_snake_case,
    reason = "Matches Laravel's notification mail message methods"
)]
impl MailMessage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.envelope.subject = subject.into();
        self
    }

    /// Selects a named HTML view. Add a plain-text alternative with `text`.
    pub fn view(mut self, name: impl Into<String>, data: ViewData) -> Self {
        self.content.markdown = None;
        self.content.html = None;
        self.content.text = None;
        self.content.text_view = None;
        self.content.view = Some(name.into());
        self.content.with.extend(data);
        self
    }

    /// Selects a named plain-text view, retaining an explicit HTML body or view.
    pub fn text(mut self, name: impl Into<String>, data: ViewData) -> Self {
        self.content.markdown = None;
        self.content.text = None;
        self.content.text_view = Some(name.into());
        self.content.with.extend(data);
        self
    }

    /// Selects the Markdown view rendered into both message bodies.
    pub fn markdown(mut self, name: impl Into<String>, data: ViewData) -> Self {
        self.content.view = None;
        self.content.html = None;
        self.content.text = None;
        self.content.text_view = None;
        self.content.markdown = Some(name.into());
        self.content.with.extend(data);
        self
    }

    /// Overrides the Markdown template while respecting explicitly selected bodies.
    pub fn template(mut self, name: impl Into<String>) -> Self {
        if self.content.view.is_none()
            && self.content.html.is_none()
            && self.content.text.is_none()
            && self.content.text_view.is_none()
        {
            self.content.markdown = Some(name.into());
        }
        self
    }

    pub fn theme(mut self, name: impl Into<String>) -> Self {
        self.content.theme = Some(name.into());
        self
    }

    /// Sets the MIME priority on the scale from 1 (highest) to 5 (lowest).
    pub fn priority(mut self, level: i32) -> Self {
        const LABELS: [&str; 5] = ["Highest", "High", "Normal", "Low", "Lowest"];
        let level = level.clamp(1, 5) as usize;
        self.headers
            .text
            .retain(|name, _| !name.eq_ignore_ascii_case("X-Priority"));
        self.headers.text.insert(
            "X-Priority".into(),
            format!("{level} ({})", LABELS[level - 1]),
        );
        self
    }

    pub fn from(mut self, address: impl Into<Address>) -> Self {
        self.envelope.from = Some(address.into());
        self
    }

    pub fn replyTo(mut self, address: impl Into<Address>) -> Self {
        self.envelope.reply_to.push(address.into());
        self
    }

    pub fn cc(mut self, address: impl Into<Address>) -> Self {
        self.envelope.cc.push(address.into());
        self
    }

    pub fn bcc(mut self, address: impl Into<Address>) -> Self {
        self.envelope.bcc.push(address.into());
        self
    }

    /// Adds an attachment whose source is resolved during delivery.
    pub fn attach(mut self, source: AttachmentSource) -> Self {
        self.attachment_sources.push(source);
        self
    }

    pub fn attachMany(mut self, sources: impl IntoIterator<Item = AttachmentSource>) -> Self {
        self.attachment_sources.extend(sources);
        self
    }

    pub fn attachData(
        self,
        data: impl Into<Vec<u8>>,
        name: impl Into<String>,
        mime: Option<&str>,
    ) -> Self {
        self.attach(Attachment::fromData(data, name).with_metadata(None, mime))
    }

    pub fn attachFromStorage(
        self,
        path: impl Into<String>,
        name: Option<&str>,
        mime: Option<&str>,
    ) -> Self {
        self.attach(Attachment::fromStorage(path).with_metadata(name, mime))
    }

    pub fn attachFromStorageDisk(
        self,
        disk: impl Into<String>,
        path: impl Into<String>,
        name: Option<&str>,
        mime: Option<&str>,
    ) -> Self {
        self.attach(Attachment::fromStorageDisk(disk, path).with_metadata(name, mime))
    }

    pub fn tag(mut self, value: impl Into<String>) -> Self {
        self.envelope.tags.push(value.into());
        self
    }

    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.envelope.metadata.insert(key.into(), value.into());
        self
    }

    /// Runs a registered message customizer at delivery, including in a queue worker.
    pub fn withMessage(mut self, customization: MessageCustomization) -> Self {
        self.envelope.using.push(customization);
        self
    }

    pub fn greeting(self, greeting: impl Into<String>) -> Self {
        self.with_value("greeting", greeting)
    }

    pub fn salutation(self, salutation: impl Into<String>) -> Self {
        self.with_value("salutation", salutation)
    }

    pub fn level(self, level: impl Into<String>) -> Self {
        self.with_value("level", level)
    }

    pub fn success(self) -> Self {
        self.level("success")
    }

    pub fn error(self) -> Self {
        self.level("error")
    }

    /// Adds a text line before the action, or after it if an action is set.
    pub fn line(mut self, line: impl AsRef<str>) -> Self {
        let data = &mut self.content.with;
        let key = if filled(data, "actionText") {
            "outroLines"
        } else {
            "introLines"
        };
        let lines = data
            .entry(key.to_owned())
            .or_insert_with(|| Value::Array(Vec::new()));
        if !lines.is_array() {
            *lines = Value::Array(Vec::new());
        }
        let line = line
            .as_ref()
            .replace("\r\n", "\n")
            .split(['\r', '\n'])
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(" ");
        lines
            .as_array_mut()
            .expect("notification line array")
            .push(line.trim().into());
        self
    }

    pub fn lineIf(self, condition: bool, line: impl AsRef<str>) -> Self {
        if condition { self.line(line) } else { self }
    }

    pub fn lines(self, lines: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        lines.into_iter().fold(self, Self::line)
    }

    pub fn linesIf(
        self,
        condition: bool,
        lines: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        if condition { self.lines(lines) } else { self }
    }

    pub fn action(self, text: impl Into<String>, url: impl Into<String>) -> Self {
        self.with_value("actionText", text)
            .with_value("actionUrl", url)
    }

    pub fn mailer(mut self, mailer: impl Into<String>) -> Self {
        self.mailer = Some(mailer.into());
        self
    }

    fn with_value(mut self, key: &str, value: impl Into<String>) -> Self {
        self.content.with.insert(key.into(), value.into().into());
        self
    }
}

impl Mailable for MailMessage {
    fn envelope(&self) -> MailResult<Envelope> {
        Ok(self.envelope.clone())
    }

    fn content(&self) -> MailResult<Content> {
        let mut content = self.content.clone();
        if content.view.is_none()
            && content.markdown.is_none()
            && content.html.is_none()
            && content.text.is_none()
            && content.text_view.is_none()
        {
            content.markdown = Some(EMAIL_TEMPLATE.into());
        }
        if content.markdown.as_deref() == Some(EMAIL_TEMPLATE) {
            default_view_data(&mut content.with)?;
        }
        Ok(content)
    }

    fn attachments(&self) -> MailResult<Vec<Attachment>> {
        Ok(self.attachments.clone())
    }

    fn attachment_sources(&self) -> MailResult<Vec<AttachmentSource>> {
        Ok(self.attachment_sources.clone())
    }

    fn headers(&self) -> MailResult<Headers> {
        Ok(self.headers.clone())
    }

    fn mailer(&self) -> Option<&str> {
        self.mailer.as_deref()
    }
}

fn filled(data: &ViewData, key: &str) -> bool {
    data.get(key)
        .and_then(Value::as_str)
        .is_some_and(|value| !matches!(value, "" | "0"))
}

fn default_view_data(data: &mut ViewData) -> MailResult<()> {
    let translate = |key: &str, args: &std::collections::BTreeMap<String, String>| {
        Lang::get(key, args, None).map_err(|source| MailError::Composition {
            source: Box::new(source),
        })
    };
    let level = data.entry("level".into()).or_insert_with(|| "info".into());
    let level = level.as_str().unwrap_or("info");
    let color = match level {
        "success" => "success",
        "error" => "error",
        _ => "primary",
    };
    let greeting = if level == "error" {
        "Whoops!"
    } else {
        "Hello!"
    };
    data.insert("color".into(), color.into());
    if !filled(data, "greeting") {
        data.insert(
            "greeting".into(),
            translate(greeting, &Default::default())?.into(),
        );
    }
    if !filled(data, "salutation") {
        data.insert(
            "regards".into(),
            translate("Regards,", &Default::default())?.into(),
        );
        data.insert(
            "appName".into(),
            Config::stringOr("app.name", "OpenRust").into(),
        );
    }
    for name in ["introLines", "outroLines"] {
        data.entry(name.to_owned())
            .or_insert_with(|| Value::Array(Vec::new()));
    }
    if let Some(action) = data.get("actionText").and_then(Value::as_str) {
        let subcopy = translate(SUBCOPY, &[("actionText".into(), action.into())].into())?;
        let url = data
            .get("actionUrl")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let display_url = url.replace("mailto:", "").replace("tel:", "");
        data.insert("hasAction".into(), true.into());
        data.insert("subcopy".into(), subcopy.into());
        data.insert("displayableActionUrl".into(), display_url.into());
    }
    Ok(())
}
