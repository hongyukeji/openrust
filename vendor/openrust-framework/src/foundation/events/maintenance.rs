use crate::contracts::events::Event;

#[derive(Clone, Copy, Debug)]
pub struct MaintenanceModeEnabled;

impl Event for MaintenanceModeEnabled {
    fn name(&self) -> &str {
        "foundation.maintenance_mode_enabled"
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MaintenanceModeDisabled;

impl Event for MaintenanceModeDisabled {
    fn name(&self) -> &str {
        "foundation.maintenance_mode_disabled"
    }
}
