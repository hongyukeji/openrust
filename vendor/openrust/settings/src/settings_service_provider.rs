use crate::{
    DatabaseSettingsRepository, Settings, SettingsError, SettingsGroup, SettingsManager,
    SettingsOptions, SettingsRepository,
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
            if !app.configuration_is_cached() {
                let env = app.make::<openrust::config::Environment>().await?;
                self.merge_config_from(app, crate::configuration::load(&env)?, "settings")?;
            }
            app.container()
                .singleton::<RegisteredGroups, _>(|_| Ok(Arc::new(RegisteredGroups::default())))?;
            app.container()
                .singleton::<SettingsManager, _>(|container| {
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
                    let namespace = match config.get("settings.namespace") {
                        None | Some(serde_json::Value::Null) => format!(
                            "{}:{}:{}:{}:{}",
                            config
                                .get("app.env")
                                .and_then(|v| v.as_str().map(str::to_owned))
                                .unwrap_or_else(|| "production".into()),
                            connection.get_driver_name(),
                            connection.get_database_name(),
                            connection.get_table_prefix(),
                            table
                        ),
                        Some(serde_json::Value::String(v)) => v,
                        _ => return Err(binding(SettingsError::InvalidDefinition)),
                    };
                    let seconds = |key: &str, default| -> Result<u64, BindingResolutionError> {
                        match config.get(key) {
                            None => Ok(default),
                            Some(v) => v
                                .as_u64()
                                .ok_or_else(|| binding(SettingsError::InvalidDefinition)),
                        }
                    };
                    let options = SettingsOptions {
                        namespace,
                        refresh_interval: std::time::Duration::from_secs(seconds(
                            "settings.refresh_interval",
                            60,
                        )?),
                        cache_ttl: std::time::Duration::from_secs(seconds(
                            "settings.cache.ttl",
                            3600,
                        )?),
                    };
                    let enabled = match config.get("settings.cache.enabled") {
                        None => true,
                        Some(serde_json::Value::Bool(v)) => v,
                        _ => return Err(binding(SettingsError::InvalidDefinition)),
                    };
                    let store = match config.get("settings.cache.store") {
                        None | Some(serde_json::Value::Null) => None,
                        Some(serde_json::Value::String(v)) => Some(v),
                        _ => return Err(binding(SettingsError::InvalidDefinition)),
                    };
                    let cache = if !enabled {
                        None
                    } else if container.bound::<openrust::cache::CacheManager>() {
                        Some(
                            container
                                .make::<openrust::cache::CacheManager>()?
                                .store(store.as_deref())
                                .map_err(binding)?,
                        )
                    } else if store.is_some() {
                        return Err(binding(SettingsError::InvalidDefinition));
                    } else {
                        None
                    };
                    let source = Arc::new(
                        DatabaseSettingsRepository::new(connection)
                            .with_table(table)
                            .map_err(binding)?,
                    );
                    Ok(Arc::new(
                        SettingsManager::new(source, options, cache).map_err(binding)?,
                    ))
                })?;
            app.container()
                .singleton::<dyn SettingsRepository, _>(|container| {
                    Ok(container.make::<SettingsManager>()? as Arc<dyn SettingsRepository>)
                })?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.publishes(
                app,
                &[(
                    "config/settings.rs",
                    include_bytes!("../config/settings.rs"),
                )],
                "settings-config",
            )?;
            let config = app.config()?;
            let table = config.get("settings.repositories.database.table");
            let connection = config.get("settings.repositories.database.connection");
            // Custom schemas are owned by the consuming application's migrations.
            if table.as_ref().and_then(|value| value.as_str()) == Some("settings")
                && connection.as_ref().is_none_or(serde_json::Value::is_null)
            {
                self.load_migrations_from(app, Self::migrations()).await?;
            }
            Ok(())
        })
    }
}

impl SettingsServiceProvider {
    /// Call from the application's provider after SettingsServiceProvider registers.
    /// Bindings are lazy scoped async factories, sharing the existing HTTP/job scope.
    pub async fn register_settings<T: SettingsGroup>(app: &Application) -> ProviderResult {
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
            .scoped_async::<Settings<T>, _>(|container| {
                Box::pin(async move {
                    let repository = container.make_async::<dyn SettingsRepository>().await?;
                    let config =
                        container.get::<dyn openrust::contracts::config::Repository>("config")?;
                    let encrypter = if T::encrypted().is_empty() {
                        None
                    } else {
                        Some(
                            container
                                .make_async::<dyn openrust::contracts::encryption::Encrypter>()
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
