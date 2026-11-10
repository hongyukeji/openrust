use std::sync::Arc;

use crate::notifications::{NotificationResult, channels::Channel};

/// Resolves the default or a named notification channel.
pub trait Factory: Send + Sync {
    fn channel(&self, name: Option<&str>) -> NotificationResult<Arc<dyn Channel>>;
}
