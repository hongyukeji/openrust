#![allow(non_snake_case, reason = "保留既定公开接口及命名")]
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
mod alipay;
#[path = "../config/pay.rs"]
mod configuration;
#[path = "../database/migrations/2026_11_21_000001_create_payment_notifications_table.rs"]
mod create_payment_notifications_table;
mod crypto;
mod error;
pub mod facades;
mod manager;
mod notifications;
mod types;
mod wechat;
pub use error::{Error, Result};
pub use manager::{Gateway, PayManager};
pub use notifications::{
    CreatePaymentNotificationsTable, DatabaseNotifications, NotificationOutcome,
};
pub use types::{Amount, ExpectedPayment, Order, Payment, Refund, Scene, VerifiedNotification};

use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use std::sync::Arc;

pub struct PayServiceProvider;
impl ServiceProvider for PayServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configuration_is_cached() {
                let env = app.make::<openrust::config::Environment>().await?;
                self.merge_config_from(app, configuration::load(&env)?, "pay")?;
            }
            app.container().singleton::<PayManager, _>(|container| {
                Ok(Arc::new(PayManager::new(
                    container.get("config")?,
                    container.make::<openrust::http::client::Factory>()?,
                )))
            })?;
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.publishes(
                app,
                &[("config/pay.rs", include_bytes!("../config/pay.rs"))],
                "pay-config",
            )?;
            self.load_migrations_from(app, Self::migrations()).await?;
            Ok(())
        })
    }
}
pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(PayServiceProvider)]
}

impl PayServiceProvider {
    /// Compiled migration entries registered by this provider's boot hook.
    pub fn migrations() -> &'static [openrust::database::migrations::MigrationEntry] {
        static ENTRIES: [openrust::database::migrations::MigrationEntry; 1] = [
            openrust::database::migrations::MigrationEntry::new(
                "vendor/openrust/pay/database/migrations/2026_11_21_000001_create_payment_notifications_table.rs",
                &create_payment_notifications_table::MIGRATION,
            ),
        ];
        &ENTRIES
    }
}
