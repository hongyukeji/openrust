use crate::contracts::events::Event;

/// Dispatched when the application's health endpoint is requested.
#[derive(Clone, Copy, Debug)]
pub struct DiagnosingHealth;

impl Event for DiagnosingHealth {
    fn name(&self) -> &str {
        "foundation.diagnosing_health"
    }
}
