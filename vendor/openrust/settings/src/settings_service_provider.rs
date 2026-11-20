use crate::{
    DatabaseSettingsRepository, Settings, SettingsError, SettingsGroup, SettingsRepository,
};
use openrust::{
    contracts::{
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
    },
    database::{DatabaseManager, migrations::MigrationEntry},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use std::{
    any::TypeId,
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

/// Package provider. Registration and boot never read the settings table.
pub struct SettingsServiceProvider;
#[derive(Default)]
struct RegisteredGroups(Mutex<BTreeMap<String, TypeId>>);

fn binding(error: impl std::error::Error + Send + Sync + 'static) -> BindingResolutionError {
    BindingResolutionError::FactoryFailed {
        service: "Settings",
        source: Box::new(error),
    }
}
impl ServiceProvider for SettingsServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.mergeConfigFrom(app, serde_json::json!({"repository":"database","repositories":{"database":{"connection":null,"table":"settings"}}}), "settings")?;
            app.container()
                .singleton::<RegisteredGroups, _>(|_| Ok(Arc::new(RegisteredGroups::default())))?;
            app.container()
                .singleton::<dyn SettingsRepository, _>(|container| {
                    let config =
                        container.get::<dyn openrust::contracts::config::Repository>("config")?;
                    if config
                        .get("settings.repository")
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .as_deref()
                        != Some("database")
                    {
                        return Err(binding(SettingsError::InvalidDefinition));
                    }
                    let connection = config.get("settings.repositories.database.connection");
                    let connection = match connection.as_ref() {
                        None | Some(serde_json::Value::Null) => None,
                        Some(serde_json::Value::String(s)) => Some(s.as_str()),
                        _ => return Err(binding(SettingsError::InvalidDefinition)),
                    };
                    let table = config
                        .get("settings.repositories.database.table")
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .ok_or_else(|| binding(SettingsError::InvalidDefinition))?;
                    let connection = container
                        .make::<DatabaseManager>()?
                        .connection(connection)
                        .map_err(binding)?;
                    Ok(Arc::new(
                        DatabaseSettingsRepository::new(connection)
                            .withTable(table)
                            .map_err(binding)?,
                    ) as Arc<dyn SettingsRepository>)
                })?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let config = app.config()?;
            let table = config.get("settings.repositories.database.table");
            let connection = config.get("settings.repositories.database.connection");
            // Custom schemas are owned by the consuming application's migrations.
            if table.as_ref().and_then(|value| value.as_str()) == Some("settings")
                && connection.as_ref().is_none_or(serde_json::Value::is_null)
            {
                self.loadMigrationsFrom(app, Self::migrations()).await?;
            }
            Ok(())
        })
    }
}

impl SettingsServiceProvider {
    /// Call from the application's provider after SettingsServiceProvider registers.
    /// Bindings are lazy scoped async factories, sharing the existing HTTP/job scope.
    pub async fn registerSettings<T: SettingsGroup>(app: &Application) -> ProviderResult {
        if !crate::database_settings_repository::identifier(T::group()) {
            return Err(SettingsError::InvalidDefinition.into());
        }
        let groups = app.make::<RegisteredGroups>().await?;
        {
            let mut groups = groups
                .0
                .lock()
                .map_err(|_| SettingsError::InvalidDefinition)?;
            if groups.contains_key(&T::group().to_ascii_lowercase()) {
                return Err(SettingsError::InvalidDefinition.into());
            }
            groups.insert(T::group().to_ascii_lowercase(), TypeId::of::<T>());
        }
        if let Err(error) = app
            .container()
            .scopedAsync::<Settings<T>, _>(|container| {
                Box::pin(async move {
                    let repository = container.makeAsync::<dyn SettingsRepository>().await?;
                    let config =
                        container.get::<dyn openrust::contracts::config::Repository>("config")?;
                    let encrypter = if T::encrypted().is_empty() {
                        None
                    } else {
                        Some(
                            container
                                .makeAsync::<dyn openrust::contracts::encryption::Encrypter>()
                                .await?,
                        )
                    };
                    Ok(Arc::new(
                        Settings::<T>::load(repository, config.as_ref(), encrypter)
                            .await
                            .map_err(binding)?,
                    ))
                })
            })
            .await
        {
            groups
                .0
                .lock()
                .map_err(|_| SettingsError::InvalidDefinition)?
                .remove(&T::group().to_ascii_lowercase());
            return Err(error.into());
        }
        Ok(())
    }

    /// Register the package's table with the existing application migration registry.
    /// Custom connection/table deployments should register CreateSettingsTable themselves.
    pub fn migrations() -> &'static [MigrationEntry] {
        static ENTRIES: &[MigrationEntry] = &[MigrationEntry::new(
            "vendor/openrust/settings/database/migrations/2026_11_21_000000_create_settings_table.rs",
            &crate::create_settings_table::MIGRATION,
        )];
        ENTRIES
    }
}
