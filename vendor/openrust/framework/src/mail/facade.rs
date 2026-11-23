use std::sync::Arc;

use crate::{container::ServiceKey, support::facades::Facade};

use super::{
    MailFake, MailManager, MailResult, Mailable, Mailer, PendingMail, SentMessage,
    mailables::Address, message::Preview,
};

/// Resolves the mail manager for the current application.
pub struct Mail;

impl Facade for Mail {
    type Service = MailManager;

    fn getFacadeAccessor() -> ServiceKey {
        "mail.manager".into()
    }
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Mail {
    pub(crate) async fn render_for_assertions(
        mailable: &dyn Mailable,
    ) -> MailResult<(String, String)> {
        Self::getFacadeRoot()
            .map_err(super::composition)?
            .render_for_assertions(mailable)
            .await
    }

    /// Renders a mailable's body using the current application.
    pub async fn render(mailable: &dyn Mailable) -> MailResult<String> {
        match Self::getFacadeRoot()
            .map_err(super::composition)?
            .preview(mailable)
            .await?
        {
            Preview::Html(body) | Preview::Text(body) => Ok(body),
        }
    }

    pub(crate) async fn preview(
        mailable: &dyn Mailable,
    ) -> Result<crate::http::Response, crate::http::HttpError> {
        use crate::http::{Body, Response, StatusCode};
        use ::http::HeaderValue;
        let error = |source| crate::http::HttpError::Internal {
            source: Box::new(source),
        };
        let preview = Self::getFacadeRoot()
            .map_err(super::composition)
            .map_err(error)?
            .preview(mailable)
            .await
            .map_err(error)?;
        let (body, media_type) = match preview {
            Preview::Html(body) => (body, "text/html; charset=utf-8"),
            Preview::Text(body) => (body, "text/plain; charset=utf-8"),
        };
        Ok(Response::new(StatusCode::OK, Body::from(body)).header(
            ::http::header::CONTENT_TYPE,
            HeaderValue::from_static(media_type),
        ))
    }

    pub fn fake() -> MailResult<Arc<MailFake>> {
        Ok(Self::getFacadeRoot().map_err(super::transport)?.fake())
    }

    pub fn extend(
        name: impl Into<String>,
        creator: impl Fn(&serde_json::Value) -> MailResult<Arc<dyn super::transport::Transport>>
        + Send
        + Sync
        + 'static,
    ) -> MailResult<()> {
        Self::getFacadeRoot()
            .map_err(super::transport)?
            .extend(name, creator);
        Ok(())
    }

    pub fn mailer(name: Option<&str>) -> MailResult<Arc<Mailer>> {
        Self::getFacadeRoot()
            .map_err(super::transport)?
            .mailer(name)
    }

    pub fn to(address: impl Into<Address>) -> MailResult<PendingMail> {
        Ok(Self::getFacadeRoot().map_err(super::transport)?.to(address))
    }

    pub fn alwaysFrom(address: impl Into<Address>) -> MailResult<()> {
        Self::mailer(None)?.alwaysFrom(address)
    }
    pub fn alwaysTo(address: impl Into<Address>) -> MailResult<()> {
        Self::mailer(None)?.alwaysTo(address)
    }
    pub fn alwaysReplyTo(address: impl Into<Address>) -> MailResult<()> {
        Self::mailer(None)?.alwaysReplyTo(address)
    }
    pub fn alwaysReturnPath(address: impl Into<String>) -> MailResult<()> {
        Self::mailer(None)?.alwaysReturnPath(address)
    }

    pub async fn send(mailable: &dyn Mailable) -> MailResult<Option<SentMessage>> {
        Self::mailer(mailable.mailer())?.send(mailable).await
    }

    pub async fn sendNow(mailable: &dyn Mailable) -> MailResult<Option<SentMessage>> {
        Self::mailer(mailable.mailer())?.sendNow(mailable).await
    }
}
