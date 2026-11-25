use crate::{
    Dashboard, Error, JobDeleted, JobFailed, JobPayloadMetadata, JobPending, JobPushed, JobRecord,
    JobReleased, JobReserved, JobsMigrated, Repository,
};
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
                .singleton::<crate::silenced::SilencedTypes, _>(|_| {
                    Ok(Arc::new(crate::silenced::SilencedTypes::default()))
                })?;
            app.container()
                .singleton::<Dashboard, _>(|_| Ok(Arc::new(Dashboard::default())))?;
            app.container()
                .singleton::<crate::master_names::MasterNames, _>(|_| Ok(Arc::default()))?;
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
                let mut repository = Repository::new(connection, prefix, retention)
                    .and_then(|repository| repository.with_config(config.as_ref()))
                    .map_err(|_| binding())?;
                repository.master_names = container.make::<crate::master_names::MasterNames>()?;
                Ok(Arc::new(repository))
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
            JobPayloadMetadata::register(
                app.make::<openrust::queue::QueueManager>().await?.as_ref(),
            );
            let dispatcher = app.make::<openrust::events::Dispatcher>().await?;
            for listener in [
                Arc::new(crate::TrimRecentJobs) as Arc<dyn Listener>,
                Arc::new(crate::TrimFailedJobs),
                Arc::new(crate::TrimMonitoredJobs),
            ] {
                dispatcher.listen(
                    crate::MasterSupervisorLooped::NAME,
                    listener,
                    Delivery::Sync,
                )?;
            }
            dispatcher.listen(
                crate::MasterSupervisorLooped::NAME,
                Arc::new(crate::ExpireSupervisors),
                Delivery::Sync,
            )?;
            dispatcher.listen(
                crate::SupervisorLooped::NAME,
                Arc::new(crate::MonitorSupervisorMemory),
                Delivery::Sync,
            )?;
            dispatcher.listen(
                crate::MasterSupervisorLooped::NAME,
                Arc::new(crate::MonitorMasterSupervisorMemory),
                Delivery::Sync,
            )?;
            dispatcher.listen(
                crate::SupervisorLooped::NAME,
                Arc::new(crate::MonitorWaitTimes),
                Delivery::Sync,
            )?;
            dispatcher.listen(
                crate::LongWaitDetected::NAME,
                Arc::new(crate::SendNotification),
                Delivery::Sync,
            )?;
            let listener = Arc::new(JobListener);
            for name in [
                RedisJobQueueing::NAME,
                JobQueued::NAME,
                RedisJobReserved::NAME,
                RedisJobsMigrated::NAME,
                RedisJobReleased::NAME,
                RedisJobDeleted::NAME,
                RedisJobFailed::NAME,
                JobPending::NAME,
                JobPushed::NAME,
                JobReserved::NAME,
                JobsMigrated::NAME,
                JobDeleted::NAME,
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
                let name = crate::console::supervisor_name();
                let state = repository.reservations_paused(name.as_deref()).await;
                return match state {
                    Ok(false) => Ok(None),
                    // Metadata failure suspends reservations until the supervisor can shut down.
                    _ => Ok(Some(ListenerResponse::Value(Box::new(false)))),
                };
            }
            use openrust::support::facades::Event as EventFacade;
            if let Some(event) = event.downcast_ref::<RedisJobQueueing>() {
                EventFacade::dispatch(JobPending {
                    connection_name: event.connection_name.clone(),
                    queue: event.queue.clone(),
                    id: event.id.clone(),
                    payload: event.job.clone(),
                })
                .await?;
                return Ok(None);
            }
            if let Some(event) = event.downcast_ref::<JobQueued>() {
                EventFacade::dispatch(JobPushed {
                    connection_name: event.connection_name.clone(),
                    queue: event.queue.clone(),
                    id: event.id.clone(),
                    payload: event.job.clone(),
                })
                .await?;
                return Ok(None);
            }
            if let Some(event) = event.downcast_ref::<RedisJobReserved>() {
                EventFacade::dispatch(JobReserved {
                    connection_name: event.connection_name.clone(),
                    job: event.job.clone(),
                })
                .await?;
                return Ok(None);
            }
            if let Some(event) = event.downcast_ref::<RedisJobsMigrated>() {
                EventFacade::dispatch(JobsMigrated {
                    connection_name: event.connection_name.clone(),
                    queue: event.queue.clone(),
                    payloads: event.jobs.clone(),
                })
                .await?;
                return Ok(None);
            }
            if let Some(event) = event.downcast_ref::<RedisJobReleased>() {
                EventFacade::dispatch(JobReleased {
                    connection_name: event.connection_name.clone(),
                    job: event.job.clone(),
                    delay: event.delay,
                })
                .await?;
                return Ok(None);
            }
            if let Some(event) = event.downcast_ref::<RedisJobDeleted>() {
                EventFacade::dispatch(JobDeleted {
                    connection_name: event.connection_name.clone(),
                    job: event.job.clone(),
                    failed: event.failed,
                    duration: event.duration,
                })
                .await?;
                return Ok(None);
            }
            if let Some(event) = event.downcast_ref::<RedisJobFailed>() {
                EventFacade::dispatch(JobFailed {
                    connection_name: event.connection_name.clone(),
                    job: event.job.clone(),
                    exception: event.exception.clone(),
                })
                .await?;
                return Ok(None);
            }
            if let Some(event) = event.downcast_ref::<JobPending>() {
                let repository = crate::dashboard::Store::get_facade_root()?;
                let (tags, silenced) = JobPayloadMetadata::tags_and_silence(&event.payload)
                    .map_err(|source| EventError::Listener {
                        source: Box::new(source),
                    })?;
                repository
                    .jobs()
                    .queueing(
                        &event.id,
                        &event.connection_name,
                        &event.queue,
                        &event.payload,
                        &tags,
                        silenced,
                    )
                    .await
                    .map_err(|source| EventError::Listener {
                        source: Box::new(source),
                    })?;
                return Ok(None);
            }
            if let Some(event) = event.downcast_ref::<JobPushed>() {
                let repository = crate::dashboard::Store::get_facade_root()?;
                let (tags, _) =
                    JobPayloadMetadata::tags_and_silence(&event.payload).map_err(|source| {
                        EventError::Listener {
                            source: Box::new(source),
                        }
                    })?;
                repository
                    .jobs()
                    .monitor_pushed(&event.id, &tags)
                    .await
                    .map_err(|source| EventError::Listener {
                        source: Box::new(source),
                    })?;
                return Ok(None);
            }
            if let Some(event) = event.downcast_ref::<JobsMigrated>() {
                if !event.payloads.is_empty() {
                    crate::dashboard::Store::get_facade_root()?
                        .jobs()
                        .migrate_snapshots(&event.connection_name, &event.queue, &event.payloads)
                        .await
                        .map_err(|source| EventError::Listener {
                            source: Box::new(source),
                        })?;
                }
                return Ok(None);
            }
            let release = event.downcast_ref::<JobReleased>();
            let (connection, job, phase, exception, duration) =
                if let Some(e) = event.downcast_ref::<JobReserved>() {
                    (&e.connection_name, &e.job, "processing", None, 0.)
                } else if let Some(e) = event.downcast_ref::<JobDeleted>() {
                    if e.failed {
                        return Ok(None);
                    }
                    (
                        &e.connection_name,
                        &e.job,
                        "finalize",
                        None,
                        e.duration.as_secs_f64() * 1_000.,
                    )
                } else if let Some(e) = event.downcast_ref::<JobFailed>() {
                    (
                        &e.connection_name,
                        &e.job,
                        "failed",
                        Some(e.exception.as_str()),
                        0.,
                    )
                } else if let Some(e) = release {
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
            let (tags, silenced) =
                JobPayloadMetadata::tags_and_silence(&job.payload).map_err(|source| {
                    EventError::Listener {
                        source: Box::new(source),
                    }
                })?;
            let record = JobRecord {
                id: job.id.clone(),
                name: job.payload.name().into(),
                connection: connection.clone(),
                queue: job.queue.clone(),
                attempts: job.attempts,
                status: String::new(),
                duration,
                updated_at: 0,
            };
            if let Some(release) = release {
                let delay_ms = u64::try_from(release.delay.as_millis())
                    .map_err(|_| EventError::InvalidPayload)?;
                repository
                    .record_release(record, &job.payload, &tags, silenced, delay_ms)
                    .await
            } else {
                repository
                    .record_payload(record, phase, &job.payload, &tags, silenced, exception)
                    .await
            }
            .map_err(|source| EventError::Listener {
                source: Box::new(source),
            })?;
            Ok(None)
        })
    }
}
