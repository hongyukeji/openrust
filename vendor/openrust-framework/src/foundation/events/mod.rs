//! Discovery of application event handlers.

mod discover_events;
pub(super) mod event_cache;

pub use discover_events::DiscoverEvents;

mod diagnosing_health;
pub use diagnosing_health::DiagnosingHealth;

mod maintenance;
pub use maintenance::{MaintenanceModeDisabled, MaintenanceModeEnabled};

mod publishing_stubs;
pub use publishing_stubs::PublishingStubs;

mod vendor_tag_published;
pub use vendor_tag_published::VendorTagPublished;

mod locale_updated;
pub use locale_updated::LocaleUpdated;

/// 主程序结束服务或独立 Artisan 命令后通知监听器收尾。
pub struct ApplicationTerminating;

impl ApplicationTerminating {
    pub const NAME: &'static str = r"OpenRust\Foundation\Events\ApplicationTerminating";
}

impl crate::contracts::events::Event for ApplicationTerminating {
    fn name(&self) -> &str {
        Self::NAME
    }
}
