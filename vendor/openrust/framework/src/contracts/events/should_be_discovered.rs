use crate::{events::EventResult, foundation::Application};

use super::Listener;

/// Controls whether a listener participates in an application's compiled discovery manifest.
pub trait ShouldBeDiscovered: Listener {
    #[allow(
        non_snake_case,
        reason = "Matches Laravel's listener discovery contract"
    )]
    fn shouldBeDiscovered(app: &Application) -> EventResult<bool>;
}
