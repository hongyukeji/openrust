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
