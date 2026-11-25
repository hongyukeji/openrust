use crate::{Dashboard, Error, JobRecord, Repository};
use openrust::support::facades::Facade;
use openrust::{
    contracts::{
        container::{BindingResolutionError, Container as _},
        events::{Dispatcher as _, Event, Listener},
        foundation::Application as _,
    },
    events::{Delivery, EventContext, EventError, EventFuture, EventResult, ListenerResponse},
    foundation::Application,
    http::HttpFuture,
    queue::events::*,
    support::{ProviderResult, ServiceProvider},
};
use std::sync::Arc;

pub struct HorizonServiceProvider;
fn binding() -> BindingResolutionError {
    BindingResolutionError::FactoryFailed {
        service: "Horizon",
        source: Box::new(Error::InvalidConfiguration),
    }
}
impl ServiceProvider for HorizonServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configuration_is_cached() {
                let env = app.make::<openrust::config::Environment>().await?;
                self.merge_config_from(app, crate::configuration::load(&env)?, "horizon")?;
            }
            app.container()
                .singleton::<crate::notifications::NotificationRoutes, _>(|_| {
                    Ok(Arc::new(crate::notifications::NotificationRoutes::default()))
                })?;
            app.container()
                .singleton_if::<dyn crate::LongWaitNotificationFactory, _>(|_| {
                    Ok(Arc::new(
                        crate::notifications::DefaultLongWaitNotificationFactory,
                    ))
                })?;
            app.container()
                .singleton::<crate::tags::TagManager, _>(|_| {
                    Ok(Arc::new(crate::tags::TagManager::default()))
                })?;
            app.container()
                .singleton::<Dashboard, _>(|_| Ok(Arc::new(Dashboard::default())))?;
            app.container().singleton::<Repository, _>(|container| {
                let config =
                    container.get::<dyn openrust::contracts::config::Repository>("config")?;
                let name = config
                    .get("horizon.use")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .ok_or_else(binding)?;
                let prefix = config
                    .get("horizon.prefix")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .ok_or_else(binding)?;
                let retention = config
                    .get("horizon.retention")
                    .and_then(|v| v.as_u64())
                    .and_then(|v| u32::try_from(v).ok())
                    .ok_or_else(binding)?;
                // Horizon uses the standalone Redis queue transport, following upstream's boundary.
                if config
                    .get(&format!("database.redis.clusters.{name}"))
                    .is_some()
                {
                    return Err(binding());
                }
                let connection = container
                    .make::<openrust::redis::RedisManager>()?
                    .connection(Some(&name))
                    .map_err(|_| binding())?;
                Ok(Arc::new(
                    Repository::new(connection, prefix, retention)
                        .and_then(|repository| repository.with_config(config.as_ref()))
                        .map_err(|_| binding())?,
                ))
            })?;
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.publishes(
                app,
                &[("config/horizon.rs", include_bytes!("../config/horizon.rs"))],
                "horizon-config",
            )?;
            self.publishes(
                app,
                &[(
                    "app/providers/horizon_service_provider.rs",
                    include_bytes!("../stubs/horizon_service_provider.rs"),
                )],
                "horizon-provider",
            )?;
            self.commands(app, crate::console::register);
            self.load_routes_from(app, crate::dashboard::configured_routes)?;
            let dispatcher = app.make::<openrust::events::Dispatcher>().await?;
            dispatcher.listen(
                crate::LongWaitDetected::NAME,
                Arc::new(crate::SendNotification),
                Delivery::Sync,
            )?;
            let listener = Arc::new(JobListener);
            for name in [
                JobQueued::NAME,
                JobProcessing::NAME,
                JobFailed::NAME,
                JobReleased::NAME,
                JobAttempted::NAME,
                Looping::NAME,
            ] {
                dispatcher.listen(name, listener.clone(), Delivery::Sync)?;
            }
            Ok(())
        })
    }
}
struct JobListener;
impl Listener for JobListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            if event.downcast_ref::<Looping>().is_some() {
                if !crate::console::is_worker() {
                    return Ok(None);
                }
                let repository = crate::dashboard::Store::get_facade_root()?;
                let state = async {
                    if repository.paused().await? {
                        return Ok::<_, Error>(true);
                    }
                    if let Some(name) = crate::console::supervisor_name() {
                        return repository.supervisor_paused(&name).await;
                    }
                    Ok(false)
                }
                .await;
                return match state {
                    Ok(false) => Ok(None),
                    // Metadata failure suspends reservations until the supervisor can shut down.
                    _ => Ok(Some(ListenerResponse::Value(Box::new(false)))),
                };
            }
            if let Some(event) = event.downcast_ref::<JobQueued>() {
                let config = openrust::support::facades::Config::get_facade_root()?;
                if config
                    .get(&format!(
                        "queue.connections.{}.driver",
                        event.connection_name
                    ))
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .as_deref()
                    != Some("redis")
                {
                    return Ok(None);
                }
                let repository = crate::dashboard::Store::get_facade_root()?;
                let tags = crate::Tags::for_payload(&event.job).map_err(|source| {
                    EventError::Listener {
                        source: Box::new(source),
                    }
                })?;
                repository
                    .jobs()
                    .pushed(
                        &event.id,
                        &event.connection_name,
                        &event.queue,
                        &event.job,
                        &tags,
                    )
                    .await
                    .map_err(|source| EventError::Listener {
                        source: Box::new(source),
                    })?;
                return Ok(None);
            }
            let (connection, job, phase, exception, duration) =
                if let Some(e) = event.downcast_ref::<JobProcessing>() {
                    (&e.connection_name, &e.job, "processing", None, 0.)
                } else if let Some(e) = event.downcast_ref::<JobFailed>() {
                    (
                        &e.connection_name,
                        &e.job,
                        "failed",
                        Some(e.exception.as_str()),
                        0.,
                    )
                } else if let Some(e) = event.downcast_ref::<JobReleased>() {
                    (&e.connection_name, &e.job, "released", None, 0.)
                } else if let Some(e) = event.downcast_ref::<JobAttempted>() {
                    (
                        &e.connection_name,
                        &e.job,
                        "finalize",
                        e.exception.as_deref(),
                        e.duration.unwrap_or_default(),
                    )
                } else {
                    return Ok(None);
                };
            let config = openrust::support::facades::Config::get_facade_root()?;
            if config
                .get(&format!("queue.connections.{connection}.driver"))
                .and_then(|v| v.as_str().map(str::to_owned))
                .as_deref()
                != Some("redis")
            {
                return Ok(None);
            }
            let repository = crate::dashboard::Store::get_facade_root()?;
            let tags =
                crate::Tags::for_payload(&job.payload).map_err(|source| EventError::Listener {
                    source: Box::new(source),
                })?;
            let silenced = config
                .get("horizon.silenced")
                .and_then(|value| {
                    value.as_array().map(|names| {
                        names
                            .iter()
                            .any(|name| name.as_str() == Some(job.payload.name()))
                    })
                })
                .unwrap_or(false)
                || config
                    .get("horizon.silenced_tags")
                    .and_then(|value| {
                        value.as_array().map(|silenced| {
                            silenced
                                .iter()
                                .filter_map(|tag| tag.as_str())
                                .any(|tag| tags.iter().any(|value| value == tag))
                        })
                    })
                    .unwrap_or(false);
            repository
                .record_payload(
                    JobRecord {
                        id: job.id.clone(),
                        name: job.payload.name().into(),
                        connection: connection.clone(),
                        queue: job.queue.clone(),
                        attempts: job.attempts,
                        status: String::new(),
                        duration,
                        updated_at: 0,
                    },
                    phase,
                    &job.payload,
                    &tags,
                    silenced,
                    exception,
                )
                .await
                .map_err(|source| EventError::Listener {
                    source: Box::new(source),
                })?;
            Ok(None)
        })
    }
}
