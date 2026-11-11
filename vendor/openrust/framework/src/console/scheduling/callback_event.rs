use std::ops::{Deref, DerefMut};

use super::Event;

/// An event that invokes application code or dispatches a queued job.
pub struct CallbackEvent(pub(super) Event);

impl Deref for CallbackEvent {
    type Target = Event;

    fn deref(&self) -> &Event {
        &self.0
    }
}

impl DerefMut for CallbackEvent {
    fn deref_mut(&mut self) -> &mut Event {
        &mut self.0
    }
}
