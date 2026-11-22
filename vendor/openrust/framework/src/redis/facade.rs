use std::sync::Arc;

use crate::{container::ServiceKey, support::facades::Facade};

use super::{RedisManager, RedisResult, Value, connections::Connection};

/// Resolves a Redis connection in the current application.
pub struct Redis;

impl Facade for Redis {
    type Service = RedisManager;

    fn getFacadeAccessor() -> ServiceKey {
        "redis".into()
    }
}

impl Redis {
    pub fn purge(name: Option<&str>) -> RedisResult<()> {
        Self::getFacadeRoot()?.purge(name);
        Ok(())
    }

    pub fn extend(
        driver: impl Into<String>,
        callback: impl Fn(&serde_json::Value, &str) -> RedisResult<Connection> + Send + Sync + 'static,
    ) -> RedisResult<()> {
        Self::getFacadeRoot()?.extend(driver, callback);
        Ok(())
    }

    pub fn connection(name: Option<&str>) -> RedisResult<Arc<Connection>> {
        Self::getFacadeRoot()?.connection(name)
    }

    pub async fn command(method: &str, parameters: &[&[u8]]) -> RedisResult<Value> {
        Self::connection(None)?.command(method, parameters).await
    }

    pub async fn get(key: &str) -> RedisResult<Option<Vec<u8>>> {
        Self::connection(None)?.get(key).await
    }

    pub async fn set(key: &str, value: &[u8]) -> RedisResult<()> {
        Self::connection(None)?.set(key, value).await
    }
}
