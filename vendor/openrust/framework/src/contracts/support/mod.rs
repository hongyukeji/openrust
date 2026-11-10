//! Service provider extension contracts.
use crate::container::ServiceKey;

/// Static service metadata for a compiled deferred provider factory.
pub trait DeferrableProvider {
    fn provides() -> Vec<ServiceKey>;
}
