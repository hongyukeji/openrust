use std::sync::Arc;

use crate::redis::RedisResult;

use super::Connection;

/// Resolves the default or a named Redis connection.
pub trait Factory: Send + Sync {
    fn connection(&self, name: Option<&str>) -> RedisResult<Arc<dyn Connection>>;
}
