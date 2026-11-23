use crate::{events::EventResult, foundation::Application};

use super::Listener;

/// Controls whether a listener participates in an application's compiled discovery manifest.
pub trait ShouldBeDiscovered: Listener {
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    fn shouldBeDiscovered(app: &Application) -> EventResult<bool>;
}
