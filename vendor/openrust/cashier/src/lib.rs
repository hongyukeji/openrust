#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
mod billing;
mod client;
#[path = "../config/cashier.rs"]
mod configuration;
#[path = "../database/migrations/2026_11_21_000001_create_cashier_tables.rs"]
mod create_cashier_tables;
mod error;
pub mod facades;
mod store;
mod types;
mod webhook;
pub use billing::{Billable, SubscriptionBuilder, SubscriptionHandle};
pub use client::{CashierManager, Client, Provider, STRIPE_VERSION};
pub use create_cashier_tables::CreateCashierTables;
pub use error::{Error, Result};
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use std::sync::Arc;
pub use store::{BillingStore, WebhookOutcome};
pub use types::{Invoice, Price, Proration, Subscription, SubscriptionItem};
pub use webhook::VerifiedEvent;
pub struct CashierServiceProvider;
impl CashierServiceProvider {
    pub fn migrations() -> &'static [openrust::database::migrations::MigrationEntry] {
        static ENTRIES: &[openrust::database::migrations::MigrationEntry] = &[
            openrust::database::migrations::MigrationEntry::new(
                "vendor/openrust/cashier/database/migrations/2026_11_21_000001_create_cashier_tables.rs",
                &create_cashier_tables::MIGRATION,
            ),
        ];
        ENTRIES
    }
}
impl ServiceProvider for CashierServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configuration_is_cached() {
                let env = app.make::<openrust::config::Environment>().await?;
                self.merge_config_from(app, configuration::load(&env)?, "cashier")?;
            }
            app.container().singleton::<CashierManager, _>(|c| {
                Ok(Arc::new(CashierManager::new(
                    c.get("config")?,
                    c.make::<openrust::http::client::Factory>()?,
                )))
            })?;
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.publishes(
                app,
                &[("config/cashier.rs", include_bytes!("../config/cashier.rs"))],
                "cashier-config",
            )?;
            self.load_migrations_from(app, Self::migrations()).await?;
            Ok(())
        })
    }
}
pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(CashierServiceProvider)]
}
