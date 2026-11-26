use crate::{ActivityLogContext, ActivityLogger, ActivityRepository, PendingActivityLog};
use openrust::{
    container::ServiceKey, contracts::container::BindingResolutionError, support::facades::Facade,
};
use std::sync::Arc;
pub struct Activitylog;
impl Facade for Activitylog {
    type Service = ActivityLogContext;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<ActivityLogContext>()
    }
}
impl Activitylog {
    pub fn repository() -> std::result::Result<Arc<ActivityRepository>, BindingResolutionError> {
        Self::get_facade_root().map(|context| context.repository())
    }
    pub fn logger() -> std::result::Result<ActivityLogger, BindingResolutionError> {
        if let Some(logger) = ActivityLogger::current() {
            return Ok(logger);
        }
        Self::get_facade_root().map(|context| context.logger())
    }
    pub fn activity() -> std::result::Result<PendingActivityLog, BindingResolutionError> {
        Self::logger().map(|logger| logger.activity())
    }
}
