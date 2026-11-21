use crate::storage::DatabaseEntriesRepository;
use openrust::{
    contracts::{
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
    },
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use std::sync::Arc;

pub struct TelescopeServiceProvider;
impl TelescopeServiceProvider {
    pub fn migrations() -> &'static [openrust::database::migrations::MigrationEntry] {
        static ENTRIES: [openrust::database::migrations::MigrationEntry; 1] = [
            openrust::database::migrations::MigrationEntry::new(
                "vendor/openrust/telescope/database/migrations/2018_08_08_100000_create_telescope_entries_table.rs",
                &crate::migration::MIGRATION,
            ),
        ];
        &ENTRIES
    }
}
fn binding() -> BindingResolutionError {
    BindingResolutionError::FactoryFailed {
        service: "DatabaseEntriesRepository",
        source: Box::new(crate::Error::InvalidConfiguration),
    }
}
impl ServiceProvider for TelescopeServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configurationIsCached() {
                let env = app.make::<openrust::config::Environment>().await?;
                self.mergeConfigFrom(app, crate::configuration::load(&env)?, "telescope")?;
            }
            app.container()
                .singleton::<crate::telescope::Options, _>(|_| {
                    Ok(Arc::new(crate::telescope::Options::default()))
                })?;
            app.container()
                .scoped::<DatabaseEntriesRepository, _>(|container| {
                    let config =
                        container.get::<dyn openrust::contracts::config::Repository>("config")?;
                    if config
                        .get("telescope.driver")
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .as_deref()
                        != Some("database")
                    {
                        return Err(binding());
                    }
                    let name = match config.get("telescope.storage.database.connection") {
                        None | Some(serde_json::Value::Null) => None,
                        Some(serde_json::Value::String(name)) => Some(name),
                        _ => return Err(binding()),
                    };
                    let chunk = config
                        .get("telescope.storage.database.chunk")
                        .and_then(|v| v.as_u64())
                        .filter(|v| *v > 0)
                        .ok_or_else(binding)?;
                    let chunk = usize::try_from(chunk).map_err(|_| binding())?;
                    let connection = container
                        .make::<openrust::database::DatabaseManager>()?
                        .connection(name.as_deref())
                        .map_err(|_| binding())?;
                    Ok(Arc::new(DatabaseEntriesRepository::new(
                        connection,
                        Some(chunk),
                    )))
                })?;
            app.container()
                .scopedIf::<dyn crate::contracts::EntriesRepository, _>(|container| {
                    Ok(container.make::<DatabaseEntriesRepository>()?
                        as Arc<dyn crate::contracts::EntriesRepository>)
                })?;
            app.container().scoped::<crate::Telescope, _>(|container| {
                let config =
                    container.get::<dyn openrust::contracts::config::Repository>("config")?;
                let enabled = config
                    .get("telescope.enabled")
                    .and_then(|value| value.as_bool())
                    .ok_or_else(binding)?;
                let telescope = Arc::new(crate::Telescope::withOptions(
                    container.make::<dyn crate::contracts::EntriesRepository>()?,
                    enabled,
                    container.make::<crate::telescope::Options>()?,
                ));
                let pending = telescope.clone();
                match openrust::support::defer(
                    async move {
                        let result = pending.store().await;
                        // 诊断存储自身的错误不再进入本次采集，避免报告时递归写入。
                        pending.stopRecording();
                        let failed = result?;
                        if !failed.is_empty() {
                            crate::jobs::ProcessPendingUpdates::new(failed, 0)
                                .dispatch()
                                .await
                                .map_err(crate::Error::Queue)?;
                        }
                        Ok::<_, crate::Error>(())
                    },
                    None,
                    true,
                ) {
                    Ok(_) => telescope
                        .automaticRecording
                        .store(true, std::sync::atomic::Ordering::Release),
                    Err(openrust::concurrency::Error::OutsideRequest) => {}
                    Err(_) => return Err(binding()),
                }
                Ok(telescope)
            })?;
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.publishes(
                app,
                &[(
                    "config/telescope.rs",
                    include_bytes!("../config/telescope.rs"),
                )],
                "telescope-config",
            )?;
            self.publishes(
                app,
                &[(
                    "app/providers/telescope_service_provider.rs",
                    include_bytes!("../stubs/telescope_service_provider.rs"),
                )],
                "telescope-provider",
            )?;
            self.loadMigrationsFrom(app, Self::migrations()).await?;
            self.commands(app, crate::console::register);
            self.loadRoutesFrom(app, crate::http::register)?;
            app.make::<openrust::queue::QueueManager>()
                .await?
                .register(
                    crate::jobs::ProcessPendingUpdates::NAME,
                    Arc::new(crate::jobs::Handler),
                )?;
            crate::watchers::registerAll(app).await?;
            let redis = app.make::<openrust::redis::RedisManager>().await?;
            redis.setEventDispatcher(app.make::<openrust::events::Dispatcher>().await?);
            redis.enableEvents();
            Ok(())
        })
    }
}
