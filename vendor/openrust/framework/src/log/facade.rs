use std::sync::Arc;

use crate::{container::ServiceKey, support::facades::Facade};

use super::{Level, LogContext, LogError, LogManager, LogResult, Logger};

/// Resolves the current application's log manager.
pub struct Log;

impl Facade for Log {
    type Service = LogManager;

    fn getFacadeAccessor() -> ServiceKey {
        "log".into()
    }
}

impl Log {
    pub fn channel(name: Option<&str>) -> LogResult<Arc<Logger>> {
        Self::getFacadeRoot()
            .map_err(LogError::Unavailable)?
            .channel(name)
    }

    pub fn stack(names: &[&str], name: Option<&str>) -> LogResult<Arc<Logger>> {
        Self::getFacadeRoot()
            .map_err(LogError::Unavailable)?
            .stack(names, name)
    }

    pub fn log(level: Level, message: &str, context: &LogContext) -> LogResult<()> {
        Self::getFacadeRoot()
            .map_err(LogError::Unavailable)?
            .log(level, message, context)
    }

    log_methods!();
}
