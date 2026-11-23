use crate::mail::{MailResult, mailables::AttachmentSource};

/// An object that can describe its own mail attachment.
pub trait Attachable {
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    fn toMailAttachment(&self) -> MailResult<AttachmentSource>;
}
