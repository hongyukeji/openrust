//! Service provider extension contracts.
use crate::container::ServiceKey;

/// Provides the original HTML markup.
#[allow(non_snake_case, reason = "Matches Laravel's Htmlable API")]
pub trait Htmlable {
    fn toHtml(&self) -> &str;
}

/// Static service metadata for a compiled deferred provider factory.
pub trait DeferrableProvider {
    fn provides() -> Vec<ServiceKey>;
}
