//! Shared responsibilities of model types.

pub(super) mod guards_attributes;
mod has_attributes;
mod has_events;
pub(super) mod has_timestamps;

pub use has_attributes::ModelState;
pub use has_events::{ModelEvent, ModelEventListener, ModelEventResponse};
