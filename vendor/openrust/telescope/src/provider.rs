use crate::{
    contracts::{ClearableRepository, EntriesRepository, PrunableRepository},
    storage::DatabaseEntriesRepository,
};
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
        static ENTRIES: &[openrust::database::migrations::MigrationEntry] = &[
            openrust::database::migrations::MigrationEntry::new(
                "vendor/openrust/telescope/database/migrations/2018_08_08_100000_create_telescope_entries_table.rs",
                &crate::migration::MIGRATION,
            ),
        ];
        ENTRIES
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
            if !app.configuration_is_cached() {
                let env = app.make::<openrust::config::Environment>().await?;
                self.merge_config_from(app, crate::configuration::load(&env)?, "telescope")?;
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
                .scoped_if::<dyn EntriesRepository, _>(|container| {
                    Ok(
                        container.make::<DatabaseEntriesRepository>()?
                            as Arc<dyn EntriesRepository>,
                    )
                })?;
            app.container()
                .scoped_if::<dyn ClearableRepository, _>(|container| {
                    Ok(container.make::<dyn EntriesRepository>()? as Arc<dyn ClearableRepository>)
                })?;
            app.container()
                .scoped_if::<dyn PrunableRepository, _>(|container| {
                    Ok(container.make::<dyn EntriesRepository>()? as Arc<dyn PrunableRepository>)
                })?;
            app.container().scoped::<crate::Telescope, _>(|container| {
                let config =
                    container.get::<dyn openrust::contracts::config::Repository>("config")?;
                let enabled = config
                    .get("telescope.enabled")
                    .and_then(|value| value.as_bool())
                    .ok_or_else(binding)?;
                let telescope = Arc::new(crate::Telescope::with_options(
                    container.make::<dyn EntriesRepository>()?,
                    enabled,
                    container.make::<crate::telescope::Options>()?,
                    match container.make::<openrust::log::context::Repository>() {
                        Ok(context) => Some(context),
                        // 管理端可在HTTP／作业作用域外配置共享选项，此时不创建Context。
                        Err(BindingResolutionError::ScopeClosed) => None,
                        Err(error) => return Err(error),
                    },
                ));
                let pending = telescope.clone();
                match openrust::support::defer(
                    async move {
                        let result = pending.store().await;
                        // 诊断存储自身的错误不再进入本次采集，避免报告时递归写入。
                        pending.stop_recording();
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
                        .automatic_recording
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
            self.load_migrations_from(app, Self::migrations()).await?;
            self.commands(app, crate::console::register);
            self.load_routes_from(app, crate::http::register)?;
            app.make::<openrust::queue::QueueManager>()
                .await?
                .register(
                    crate::jobs::ProcessPendingUpdates::NAME,
                    Arc::new(crate::jobs::Handler),
                )?;
            crate::watchers::register_all(app).await?;
            let redis = app.make::<openrust::redis::RedisManager>().await?;
            redis.set_event_dispatcher(app.make::<openrust::events::Dispatcher>().await?);
            redis.enable_events();
            Ok(())
        })
    }
}
