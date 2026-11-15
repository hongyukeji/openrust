//! Shared responsibilities of model types.

pub(super) mod guards_attributes;
mod has_attributes;
pub(super) mod has_events;
pub(super) mod has_relationships;
pub(super) mod has_timestamps;
pub(super) mod queries_relationships;

pub use has_attributes::ModelState;
pub use has_events::{ModelEvent, ModelEventListener, ModelEventResponse};

pub(in crate::database) use guards_attributes::unguarded;
pub(in crate::database) use has_events::without_events;
