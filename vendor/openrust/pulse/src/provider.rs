use crate::{
    Pulse,
    ingests::{Ingest, NullIngest, RedisIngest, Retention, StorageIngest},
    storage::{DatabaseStorage, Storage},
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
#[path = "../database/migrations/2026_11_21_000000_create_pulse_tables.rs"]
mod migration;
pub struct PulseServiceProvider;
fn binding(e: impl std::error::Error + Send + Sync + 'static) -> BindingResolutionError {
    BindingResolutionError::FactoryFailed {
        service: "Pulse",
        source: Box::new(e),
    }
}
fn invalid(message: &str) -> BindingResolutionError {
    binding(crate::Error::InvalidConfiguration(message.into()))
}
pub(crate) fn duration(value: &serde_json::Value) -> crate::Result<i64> {
    let text = value.as_str().ok_or_else(|| {
        crate::Error::InvalidConfiguration("retention must be a duration string".into())
    })?;
    let span: jiff::Span = text
        .parse()
        .map_err(|_| crate::Error::InvalidConfiguration("invalid retention duration".into()))?;
    let seconds = span
        .total(jiff::SpanTotal::from(jiff::Unit::Second).days_are_24_hours())
        .map_err(|_| crate::Error::InvalidConfiguration("invalid retention duration".into()))?;
    if seconds < 0.0 || !seconds.is_finite() || seconds > i64::MAX as f64 {
        return Err(crate::Error::InvalidConfiguration(
            "invalid retention duration".into(),
        ));
    }
    Ok(seconds as i64)
}
impl ServiceProvider for PulseServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configuration_is_cached() {
                let env = app.make::<openrust::config::Environment>().await?;
                self.merge_config_from(app, crate::configuration::load(&env)?, "pulse")?;
            }
            app.container()
                .singleton::<crate::recorders::Servers, _>(|c| {
                    Ok(Arc::new(crate::recorders::Servers::new(
                        c.get::<dyn openrust::contracts::config::Repository>("config")?,
                    )))
                })?;
            app.container()
                .singleton::<crate::pulse::Options, _>(|_| Ok(Arc::default()))?;
            app.container().scoped_if::<crate::Users, _>(|c| {
                let mut users = crate::Users::default();
                users.fields = c.make::<crate::pulse::Options>()?.userFields.clone();
                Ok(Arc::new(users))
            })?;
            app.container()
                .scoped_if::<dyn crate::ResolvesUsers, _>(|c| {
                    Ok(c.make::<crate::Users>()? as Arc<dyn crate::ResolvesUsers>)
                })?;
            app.container().scoped_if::<DatabaseStorage, _>(|c| {
                let config = c.get::<dyn openrust::contracts::config::Repository>("config")?;
                if config
                    .get("pulse.storage.driver")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .as_deref()
                    != Some("database")
                {
                    return Err(invalid("unknown storage driver"));
                }
                let name = config
                    .get("pulse.storage.database.connection")
                    .and_then(|v| v.as_str().map(str::to_owned));
                let chunk = config
                    .get("pulse.storage.database.chunk")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| invalid("storage.database.chunk must be positive"))?;
                let connection = c
                    .make::<openrust::database::DatabaseManager>()?
                    .connection(name.as_deref())
                    .map_err(binding)?;
                let keep = duration(
                    &config
                        .get("pulse.storage.trim.keep")
                        .unwrap_or("7 days".into()),
                )
                .map_err(binding)?;
                Ok(Arc::new(
                    DatabaseStorage::new(connection)
                        .withChunk(usize::try_from(chunk).map_err(|_| invalid("chunk overflow"))?)
                        .map_err(binding)?
                        .withKeep(keep),
                ))
            })?;
            app.container().scoped_if::<dyn Storage, _>(|c| {
                Ok(c.make::<DatabaseStorage>()? as Arc<dyn Storage>)
            })?;
            app.container().scoped_if::<dyn Ingest, _>(|c| {
                let config = c.get::<dyn openrust::contracts::config::Repository>("config")?;
                match config
                    .get("pulse.ingest.driver")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .as_deref()
                {
                    Some("storage") => {
                        Ok(Arc::new(StorageIngest(c.make::<dyn Storage>()?)) as Arc<dyn Ingest>)
                    }
                    None | Some("null") => Ok(Arc::new(NullIngest) as Arc<dyn Ingest>),
                    Some("redis") => {
                        let name = config
                            .get("pulse.ingest.redis.connection")
                            .and_then(|v| v.as_str().map(str::to_owned));
                        let connection = c
                            .make::<openrust::redis::RedisManager>()?
                            .connection(name.as_deref())
                            .map_err(binding)?;
                        let chunk = config
                            .get("pulse.ingest.redis.chunk")
                            .and_then(|v| v.as_u64())
                            .ok_or_else(|| invalid("invalid Redis chunk"))?;
                        let keep = config
                            .get("pulse.ingest.trim.keep")
                            .unwrap_or("7 days".into());
                        let keep = if let Some(n) = keep.as_u64() {
                            Retention::Entries(n)
                        } else {
                            Retention::Seconds(duration(&keep).map_err(binding)?)
                        };
                        Ok(Arc::new(
                            RedisIngest::new(connection)
                                .withChunk(
                                    usize::try_from(chunk)
                                        .map_err(|_| invalid("chunk overflow"))?,
                                )
                                .map_err(binding)?
                                .withKeep(keep),
                        ) as Arc<dyn Ingest>)
                    }
                    _ => Err(invalid("unknown ingest driver")),
                }
            })?;
            app.container().scoped::<Pulse, _>(|c| {
                let config = c.get::<dyn openrust::contracts::config::Repository>("config")?;
                let buffer = config
                    .get("pulse.ingest.buffer")
                    .and_then(|v| {
                        v.as_u64().or_else(|| {
                            v.as_f64()
                                .filter(|v| *v >= 0.0 && v.fract() == 0.0)
                                .map(|v| v as u64)
                        })
                    })
                    .ok_or_else(|| invalid("ingest.buffer must be an unsigned integer"))?;
                let odds = config
                    .get("pulse.ingest.trim.lottery")
                    .ok_or_else(|| invalid("missing trim lottery"))?;
                let odds = odds
                    .as_array()
                    .filter(|v| v.len() == 2)
                    .ok_or_else(|| invalid("trim lottery requires two integers"))?;
                let lottery = openrust::support::Lottery::odds(
                    odds[0]
                        .as_u64()
                        .ok_or_else(|| invalid("invalid trim lottery"))?,
                    odds[1]
                        .as_u64()
                        .ok_or_else(|| invalid("invalid trim lottery"))?,
                )
                .map_err(binding)?;
                let pulse = Arc::new(
                    Pulse::new(c.make::<dyn Storage>()?, c.make::<dyn Ingest>()?)
                        .withUserResolver(c.make::<dyn crate::ResolvesUsers>()?)
                        .withTrimLottery(lottery)
                        .withOptions(c.make::<crate::pulse::Options>()?)
                        .withBuffer(
                            usize::try_from(buffer).map_err(|_| invalid("buffer overflow"))?,
                        ),
                );
                if config.get("pulse.enabled").and_then(|v| v.as_bool()) == Some(false) {
                    pulse.stopRecording();
                }
                let pending = pulse.clone();
                match openrust::support::defer(
                    async move {
                        pending.ingest().await;
                        Ok::<_, crate::Error>(())
                    },
                    None,
                    true,
                ) {
                    Ok(_) => {
                        pulse
                            .automaticRecording
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    Err(openrust::concurrency::Error::OutsideRequest) => {}
                    Err(e) => return Err(binding(e)),
                }
                Ok(pulse)
            })?;
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.load_views_from(
                app,
                &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/views"),
                "pulse",
            )
            .await?;
            self.commands(app, crate::commands::register);
            self.load_routes_from(app, crate::http::routes)?;
            self.publishes(
                app,
                &[(
                    "resources/views/vendor/pulse/dashboard.html",
                    include_bytes!("../resources/views/dashboard.html"),
                )],
                "pulse-dashboard",
            )?;
            self.publishes(
                app,
                &[("config/pulse.rs", include_bytes!("../config/pulse.rs"))],
                "pulse-config",
            )?;
            self.load_migrations_from(app, Self::migrations()).await?;
            if app.config()?.get("pulse.enabled").and_then(|v| v.as_bool()) != Some(false) {
                crate::recorders::register(
                    app.make::<openrust::events::Dispatcher>().await?.as_ref(),
                    app.make::<openrust::http::client::Factory>()
                        .await?
                        .as_ref(),
                    app.config()?,
                    app.make::<crate::recorders::Servers>().await?,
                    app.base_path().to_owned(),
                    app.make::<crate::pulse::Options>().await?,
                )?;
                crate::recorders::registerLogging(
                    app.make::<openrust::log::LogManager>().await?.as_ref(),
                    app.config()?,
                    app.make::<crate::pulse::Options>().await?,
                )?;
            }
            Ok(())
        })
    }
}
impl PulseServiceProvider {
    pub fn migrations() -> &'static [openrust::database::migrations::MigrationEntry] {
        static MIGRATIONS: &[openrust::database::migrations::MigrationEntry] = &[
            openrust::database::migrations::MigrationEntry::new(
                "vendor/openrust/pulse/database/migrations/2026_11_21_000000_create_pulse_tables.rs",
                &migration::MIGRATION,
            ),
        ];
        MIGRATIONS
    }
}
