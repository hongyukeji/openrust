#![doc = include_str!("../README.md")]
#![allow(non_snake_case, reason = "Preserves Laravel Settings public names")]

#[path = "../database/migrations/2026_11_21_000000_create_settings_table.rs"]
mod create_settings_table;
mod database_settings_repository;
mod error;
mod migration;
mod settings;
mod settings_repository;
mod settings_service_provider;

pub use database_settings_repository::DatabaseSettingsRepository;
pub use error::{SettingsError, SettingsFuture, SettingsResult};
pub use migration::CreateSettingsTable;
pub use settings::{Settings, SettingsGroup};
pub use settings_repository::{
    SettingsRepository, SettingsSnapshot, SettingsUpdate, StoredProperty,
};
pub use settings_service_provider::SettingsServiceProvider;
