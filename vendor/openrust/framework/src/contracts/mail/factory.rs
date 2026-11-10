use std::sync::Arc;

use crate::mail::MailResult;

use super::Mailer;

/// Resolves the default or a named mailer.
pub trait Factory: Send + Sync {
    fn mailer(&self, name: Option<&str>) -> MailResult<Arc<dyn Mailer>>;
}
