use std::sync::Arc;

use crate::queue::QueueResult;

use super::Queue;

/// Resolves the default or a named queue connection.
pub trait Factory: Send + Sync {
    fn connection(&self, name: Option<&str>) -> QueueResult<Arc<dyn Queue>>;
}
