use crate::{events::EventResult, foundation::Application};

use super::Listener;

/// Controls whether a listener participates in an application's compiled discovery manifest.
pub trait ShouldBeDiscovered: Listener {
    fn should_be_discovered(app: &Application) -> EventResult<bool>;
}
