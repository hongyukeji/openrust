use crate::{
    mail::{MailFuture, MailResult},
    queue::Delay,
};

use super::Mailable;

/// Schedules a mailable for queued delivery.
pub trait MailQueue: Send + Sync {
    fn queue<'a>(
        &'a self,
        mailable: &'a dyn Mailable,
        queue: Option<&'a str>,
    ) -> MailFuture<'a, MailResult<Option<String>>>;

    fn later<'a>(
        &'a self,
        delay: Delay,
        mailable: &'a dyn Mailable,
        queue: Option<&'a str>,
    ) -> MailFuture<'a, MailResult<Option<String>>>;
}
