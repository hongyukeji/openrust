//! Contracts shared by migration lifecycle events.

/// Subscribe to this name to receive every migration lifecycle event.
pub struct MigrationEvent;

impl MigrationEvent {
    pub const NAME: &'static str = r"OpenRust\Contracts\Database\Events\MigrationEvent";
}
