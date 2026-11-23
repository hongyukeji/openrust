#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use crate::mail::{MailFuture, MailResult, SentMessage};

use super::Mailable;

/// Dispatches a mailable or sends it immediately.
pub trait Mailer: Send + Sync {
    fn send<'a>(
        &'a self,
        mailable: &'a dyn Mailable,
    ) -> MailFuture<'a, MailResult<Option<SentMessage>>>;

    fn send_now<'a>(
        &'a self,
        mailable: &'a dyn Mailable,
    ) -> MailFuture<'a, MailResult<Option<SentMessage>>>;
}
