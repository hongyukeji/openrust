use crate::{Error, Repository, Result, SupervisorOptions};
use http::{
    HeaderValue, StatusCode,
    header::{CACHE_CONTROL, CONTENT_TYPE},
};
use openrust::{
    container::ServiceKey,
    contracts::container::BindingResolutionError,
    http::{Body, HttpError, HttpFuture, Request, Response},
    support::facades::{Config, Facade, Route},
};
use std::{
    collections::BTreeSet,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

type Authorize =
    for<'a> fn(&'a mut Request) -> HttpFuture<'a, std::result::Result<bool, HttpError>>;
/// Application-owned authorization. Absent authorization always denies access.
#[derive(Default)]
pub struct Dashboard {
    authorize: RwLock<Option<Authorize>>,
    routes_registered: AtomicBool,
}
impl Dashboard {
    pub(crate) fn repository(&self) -> Result<Arc<Repository>> {
        Store::get_facade_root().map_err(|_| Error::Unavailable)
    }
    async fn allowed(&self, request: &mut Request) -> std::result::Result<bool, HttpError> {
        let callback = *self
            .authorize
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match callback {
            Some(callback) => callback(request).await,
            None => Ok(false),
        }
    }
    pub async fn stats(&self) -> Result<serde_json::Value> {
        let config = openrust::support::facades::Config::get_facade_root()
            .map_err(|_| Error::Unavailable)?;
        let options = SupervisorOptions::from_config(config.as_ref())?;
        let repository = self.repository()?;
        let queue = openrust::queue::QueueFacade::manager().map_err(|_| Error::Unavailable)?;
        let workload = repository.workload().get().await?;
        let mut monitored = BTreeSet::new();
        for option in options.values() {
            for name in &option.queue {
                monitored.insert((option.connection.clone(), name.clone()));
            }
        }
        // Explicit --environment deployments can differ from this HTTP process's
        // configured plan. Include the queues reported by live supervisors.
        for pool in &workload {
            if let (Some(connection), Some(queues)) =
                (pool["connection"].as_str(), pool["name"].as_str())
            {
                for name in queues.split(',') {
                    monitored.insert((connection.to_owned(), name.to_owned()));
                }
            }
        }
        let mut queues = Vec::new();
        for (connection, name) in monitored {
            let transport = queue
                .connection(Some(&connection))
                .map_err(|_| Error::Queue)?;
            queues.push(serde_json::json!({"connection":connection,"queue":name,"backlog":transport.metrics(Some(&name)).await.map_err(|_|Error::Queue)?,"metrics":repository.metrics(&connection,&name).await?}));
        }
        let paused = repository.paused().await?;
        let masters = repository.masters().await?;
        let metrics = repository.metrics_repository();
        let processes = masters
            .iter()
            .filter_map(|m| m.get("workers").and_then(serde_json::Value::as_array))
            .map(Vec::len)
            .sum::<usize>();
        let mut wait = serde_json::Map::new();
        if let Some(queue) = workload.first() {
            wait.insert(
                format!(
                    "{}:{}",
                    queue["connection"].as_str().unwrap_or_default(),
                    queue["name"].as_str().unwrap_or_default()
                ),
                queue["wait"].clone(),
            );
        }
        Ok(
            serde_json::json!({"paused":paused,"masters":masters,"queues":queues,"recent":repository.recent(100).await?,
                "failedJobs":repository.jobs().count_recently_failed().await?,"recentJobs":repository.jobs().count_recent().await?,
                "jobsPerMinute":metrics.jobs_processed_per_minute().await?,"queueWithMaxRuntime":metrics.queue_with_maximum_runtime().await?,"queueWithMaxThroughput":metrics.queue_with_maximum_throughput().await?,
                "processes":processes,"pausedMasters":masters.iter().filter(|m|m.get("status").and_then(serde_json::Value::as_str)==Some("paused")).count(),
                "periods":{"failedJobs":config.get("horizon.trim.recent_failed").or_else(||config.get("horizon.trim.failed")).unwrap_or(10080.into()),"recentJobs":config.get("horizon.trim.recent").unwrap_or(60.into())},"status":if masters.is_empty(){"inactive"}else if masters.iter().all(|master|master["status"]=="paused"){"paused"}else{"running"},"wait":wait
            }),
        )
    }
}
pub struct Horizon;
impl Facade for Horizon {
    type Service = Dashboard;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<Dashboard>()
    }
}
impl Horizon {
    pub fn manager() -> std::result::Result<Arc<Dashboard>, BindingResolutionError> {
        Self::get_facade_root()
    }
    pub fn auth(callback: Authorize) -> std::result::Result<(), BindingResolutionError> {
        *Self::manager()?
            .authorize
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(callback);
        Ok(())
    }
    pub fn routes() -> std::result::Result<(), openrust::routing::RoutingError> {
        let config = openrust::support::facades::Config::get_facade_root()
            .map_err(|source| openrust::routing::RoutingError::ContainerFailed { source })?;
        let path = config.get("horizon.path").unwrap_or("horizon".into());
        let path = path
            .as_str()
            .ok_or_else(|| openrust::routing::RoutingError::InvalidUri {
                uri: path.to_string(),
            })?;
        Self::routes_at(&format!("/{}", path.trim_matches('/')))
    }
    pub fn routes_at(path: &str) -> std::result::Result<(), openrust::routing::RoutingError> {
        // Static application-owned paths only; no route parameters or encoded separators.
        let path = if path == "/" {
            "/"
        } else {
            path.trim_end_matches('/')
        };
        if !path.starts_with('/')
            || path.is_empty()
            || path.len() > 200
            || !path
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"/-_".contains(&c))
        {
            return Err(openrust::routing::RoutingError::InvalidUri { uri: path.into() });
        }
        let dashboard = Self::manager()
            .map_err(|source| openrust::routing::RoutingError::ContainerFailed { source })?;
        if dashboard.routes_registered.load(Ordering::SeqCst) {
            return Ok(());
        }
        let config = openrust::support::facades::Config::get_facade_root()
            .map_err(|source| openrust::routing::RoutingError::ContainerFailed { source })?;
        let mut group = openrust::routing::RouteGroup::new();
        if let Some(domain) = config
            .get("horizon.domain")
            .filter(|value| !value.is_null())
        {
            group = group.domain(domain.as_str().ok_or_else(|| {
                openrust::routing::RoutingError::InvalidUri {
                    uri: domain.to_string(),
                }
            })?);
        }
        let middleware = config
            .get("horizon.middleware")
            .unwrap_or(serde_json::json!(["web"]));
        for name in middleware.as_array().ok_or_else(|| {
            openrust::routing::RoutingError::UnknownMiddleware {
                name: middleware.to_string(),
            }
        })? {
            group = group.middleware_name(name.as_str().ok_or_else(|| {
                openrust::routing::RoutingError::UnknownMiddleware {
                    name: name.to_string(),
                }
            })?);
        }
        Route::group(group, |router| {
            router.get(path, page)?;
            let path = path.trim_end_matches('/');
            router.get(&format!("{path}/api/stats"), stats)?;
            for suffix in [
                "workload",
                "masters",
                "monitoring",
                "monitoring/{tag}",
                "metrics/jobs",
                "metrics/jobs/{id}",
                "metrics/queues",
                "metrics/queues/{id}",
                "batches",
                "batches/{id}",
                "jobs/pending",
                "jobs/completed",
                "jobs/silenced",
                "jobs/failed",
                "jobs/failed/{id}",
                "jobs/{id}",
            ] {
                router.get(&format!("{path}/api/{suffix}"), api)?;
            }
            for suffix in [
                "monitoring",
                "jobs/retry/{id}",
                "batches/retry/{id}",
                "pause",
                "continue",
                "terminate",
            ] {
                router.post(&format!("{path}/api/{suffix}"), api)?;
            }
            router.delete(&format!("{path}/api/monitoring/{{tag}}"), api)?;
            Ok(())
        })?;
        dashboard.routes_registered.store(true, Ordering::SeqCst);
        Ok(())
    }
}
fn page(request: &mut Request) -> HttpFuture<'_, std::result::Result<Response, HttpError>> {
    Box::pin(async move {
        let dashboard = Horizon::manager().map_err(|_| HttpError::from(Error::Unavailable))?;
        if !dashboard.allowed(request).await? {
            return Ok(Response::empty(StatusCode::FORBIDDEN));
        }
        let nonce = uuid::Uuid::now_v7().simple().to_string();
        let config = Config::get_facade_root().map_err(|_| Error::Unavailable)?;
        let name = config.get("horizon.name");
        let suffix = name
            .as_ref()
            .and_then(serde_json::Value::as_str)
            .filter(|name| !matches!(*name, "" | "0"))
            .map(|name| {
                format!(
                    " - {}",
                    name.replace('&', "&amp;")
                        .replace('<', "&lt;")
                        .replace('>', "&gt;")
                )
            })
            .unwrap_or_default();
        let html = include_str!("dashboard.html")
            .replace("__NONCE__", &nonce)
            .replace("__HORIZON_NAME__", &suffix);
        Ok(Response::new(StatusCode::OK,Body::from(html))
            .header(CONTENT_TYPE,HeaderValue::from_static("text/html; charset=utf-8"))
            .header(CACHE_CONTROL,HeaderValue::from_static("no-store"))
            .header(http::header::CONTENT_SECURITY_POLICY,HeaderValue::from_str(&format!("default-src 'none'; style-src 'nonce-{nonce}'; script-src 'nonce-{nonce}'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'")).map_err(|_|HttpError::InvalidRequest)?))
    })
}
fn stats(request: &mut Request) -> HttpFuture<'_, std::result::Result<Response, HttpError>> {
    Box::pin(async move {
        let dashboard = Horizon::manager().map_err(|_| HttpError::from(Error::Unavailable))?;
        if !dashboard.allowed(request).await? {
            return Ok(Response::empty(StatusCode::FORBIDDEN));
        }
        Ok(Response::json(StatusCode::OK, dashboard.stats().await?)?
            .header(CACHE_CONTROL, HeaderValue::from_static("no-store")))
    })
}

fn api(request: &mut Request) -> HttpFuture<'_, std::result::Result<Response, HttpError>> {
    Box::pin(async move {
        use serde_json::json;
        let dashboard = Horizon::manager().map_err(|_| HttpError::from(Error::Unavailable))?;
        if !dashboard.allowed(request).await? {
            return Ok(Response::empty(StatusCode::FORBIDDEN));
        }
        let write = request.method() != http::Method::GET;
        // A non-simple header forces browsers to enforce same-origin preflight for mutations.
        if write
            && (request.header("x-horizon-request")? != Some("1")
                || request.header("sec-fetch-site")? == Some("cross-site"))
        {
            return Ok(Response::empty(StatusCode::FORBIDDEN));
        }
        let path = request
            .path()
            .rsplit_once("/api/")
            .map(|(_, path)| path)
            .ok_or(HttpError::NotFound)?
            .to_owned();
        let id = request.route("id").map(str::to_owned);
        let repository = dashboard.repository()?;
        let jobs = repository.jobs();
        let metrics = repository.metrics_repository();
        let tags = repository.tags();
        let starting = request
            .query(Some("starting_at"))?
            .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
            .map(|value| value as usize);
        let value = match (request.method().as_str(), path.as_str()) {
            ("GET", "workload") => {
                let mut workload = repository.workload().get().await?;
                workload.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
                json!(workload)
            }
            ("GET", "masters") => json!(repository.master_supervisors().all().await?),
            ("GET", "monitoring") => {
                let mut result = Vec::new();
                for tag in tags.monitoring().await? {
                    result.push(json!({"tag":tag,"count":tags.count(&tag).await?+tags.count(&format!("failed:{tag}")).await?}));
                }
                json!(result)
            }
            ("GET", _) if path.starts_with("monitoring/") => {
                let tag = request
                    .query(Some("tag"))?
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .or_else(|| request.route("tag").map(str::to_owned))
                    .ok_or(HttpError::InvalidRequest)?;
                let limit = request
                    .query(Some("limit"))?
                    .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
                    .unwrap_or(25)
                    .min(1000) as usize;
                if limit == 0 {
                    return Err(HttpError::InvalidRequest);
                }
                let ids = tags.paginate(&tag, starting.unwrap_or(0), limit).await?;
                json!({"jobs":jobs.get_jobs(&ids,0).await?,"total":tags.count(&tag).await?})
            }
            ("POST", "monitoring") => {
                let tag = request
                    .input(Some("tag"))
                    .await?
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .ok_or(HttpError::InvalidRequest)?;
                tags.monitor(&tag).await?;
                json!({"tag":tag})
            }
            ("DELETE", _) if path.starts_with("monitoring/") => {
                let tag = request.route("tag").ok_or(HttpError::InvalidRequest)?;
                tags.stop_monitoring(tag).await?;
                jobs.delete_monitored(&tags.jobs(tag).await?).await?;
                tags.forget(tag).await?;
                json!({"deleted":true})
            }
            ("GET", "metrics/jobs") => json!(metrics.measured_jobs().await?),
            ("GET", "metrics/queues") => json!(metrics.measured_queues().await?),
            ("GET", _) if path.starts_with("metrics/jobs/") => snapshot_response(
                metrics
                    .snapshots_for_job(id.as_deref().ok_or(HttpError::InvalidRequest)?)
                    .await?,
            ),
            ("GET", _) if path.starts_with("metrics/queues/") => snapshot_response(
                metrics
                    .snapshots_for_queue(id.as_deref().ok_or(HttpError::InvalidRequest)?)
                    .await?,
            ),
            ("GET", "jobs/pending") => {
                json!({"jobs":jobs.get_pending(starting).await?,"total":jobs.count_pending().await?})
            }
            ("GET", "jobs/completed") => {
                json!({"jobs":jobs.get_completed(starting).await?,"total":jobs.count_completed().await?})
            }
            ("GET", "jobs/silenced") => {
                json!({"jobs":jobs.get_silenced(starting).await?,"total":jobs.count_silenced().await?})
            }
            ("GET", "jobs/failed") => {
                json!({"jobs":jobs.get_failed(starting).await?,"total":jobs.count_failed().await?})
            }
            ("GET", _) if path.starts_with("jobs/failed/") => serde_json::to_value(
                jobs.find_failed(id.as_deref().ok_or(HttpError::InvalidRequest)?)
                    .await?
                    .ok_or(HttpError::NotFound)?,
            )
            .map_err(|_| HttpError::InvalidRequest)?,
            ("GET", _) if path.starts_with("jobs/") => serde_json::to_value(
                jobs.find(id.as_deref().ok_or(HttpError::InvalidRequest)?)
                    .await?
                    .ok_or(HttpError::NotFound)?,
            )
            .map_err(|_| HttpError::InvalidRequest)?,
            ("GET", "batches") => {
                let manager = openrust::queue::QueueFacade::manager()
                    .map_err(|_| HttpError::from(Error::Unavailable))?;
                let before = request
                    .query(Some("before_id"))?
                    .and_then(|value| value.as_str().map(str::to_owned));
                json!({"batches":manager.batch_repository().map_err(|_|HttpError::from(Error::Queue))?.get(50,before.as_deref()).await.map_err(|_|HttpError::from(Error::Queue))?})
            }
            ("GET", _) if path.starts_with("batches/") => {
                let manager = openrust::queue::QueueFacade::manager()
                    .map_err(|_| HttpError::from(Error::Unavailable))?;
                json!(
                    manager
                        .batch_repository()
                        .map_err(|_| HttpError::from(Error::Queue))?
                        .find(id.as_deref().ok_or(HttpError::InvalidRequest)?)
                        .await
                        .map_err(|_| HttpError::from(Error::Queue))?
                        .ok_or(HttpError::NotFound)?
                )
            }
            ("POST", _)
                if path.starts_with("jobs/retry/") || path.starts_with("batches/retry/") =>
            {
                let manager = openrust::queue::QueueFacade::manager()
                    .map_err(|_| HttpError::from(Error::Unavailable))?;
                let failures = FailedJobs::get_facade_root()
                    .map_err(|_| HttpError::from(Error::Unavailable))?;
                let id = id.as_deref().ok_or(HttpError::InvalidRequest)?;
                let ids = if path.starts_with("batches/") {
                    manager
                        .batch_repository()
                        .map_err(|_| HttpError::from(Error::Queue))?
                        .find(id)
                        .await
                        .map_err(|_| HttpError::from(Error::Queue))?
                        .ok_or(HttpError::NotFound)?
                        .failed_job_ids
                } else {
                    vec![id.into()]
                };
                let expected_batch = if path.starts_with("batches/") {
                    Some(id.to_owned())
                } else {
                    None
                };
                let mut retried = 0;
                for id in ids {
                    let monitored = if expected_batch.is_some() {
                        jobs.find_failed_by_uuid(&id).await?
                    } else {
                        jobs.find_failed(&id).await?
                    };
                    let Some(monitored) = monitored else {
                        continue;
                    };
                    let uuid = monitored.payload_uuid()?;
                    // A later retry can fail with the same payload UUID. Only its
                    // current transport identity may claim the native failure.
                    if jobs
                        .find_failed_by_uuid(uuid)
                        .await?
                        .is_none_or(|current| current.record.id != monitored.record.id)
                    {
                        continue;
                    }
                    if let Some(job) = failures
                        .find(uuid)
                        .await
                        .map_err(|_| HttpError::from(Error::Queue))?
                    {
                        if job.connection != monitored.record.connection
                            || job.queue != monitored.record.queue
                            || serde_json::to_value(&job.payload)
                                .map_err(|_| HttpError::from(Error::Queue))?
                                != monitored.payload
                        {
                            continue;
                        }
                        if expected_batch
                            .as_deref()
                            .is_some_and(|batch| job.payload.batch_id() != Some(batch))
                        {
                            continue;
                        }
                        if let Some(retry_id) = manager
                            .retry_failed_with_reference(
                                failures.as_ref(),
                                uuid,
                                &job,
                                &monitored.record.id,
                                crate::JobPayloadMetadata::retry_deadline(&job.payload)?,
                            )
                            .await
                            .map_err(|_| HttpError::from(Error::Queue))?
                        {
                            jobs.store_retry_reference(&monitored.record.id, &retry_id)
                                .await?;
                            retried += 1;
                        }
                    }
                }
                json!({"retried":retried})
            }
            ("POST", "pause") => {
                repository.pause().await?;
                json!({"paused":true})
            }
            ("POST", "continue") => {
                repository.r#continue().await?;
                json!({"paused":false})
            }
            ("POST", "terminate") => {
                repository.terminate().await?;
                json!({"terminated":true})
            }
            _ => return Err(HttpError::NotFound),
        };
        Ok(Response::json(StatusCode::OK, value)?
            .header(CACHE_CONTROL, HeaderValue::from_static("no-store")))
    })
}

pub(crate) struct Store;
impl Facade for Store {
    type Service = Repository;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<Repository>()
    }
}

fn snapshot_response(mut snapshots: Vec<serde_json::Value>) -> serde_json::Value {
    for snapshot in &mut snapshots {
        if let Some(runtime) = snapshot.get("runtime").and_then(serde_json::Value::as_f64) {
            snapshot["runtime"] = serde_json::json!((runtime / 1000. * 1000.).round() / 1000.);
        }
    }
    serde_json::json!(snapshots)
}

struct FailedJobs;
impl Facade for FailedJobs {
    type Service = dyn openrust::queue::failed::FailedJobProviderInterface;
    fn get_facade_accessor() -> ServiceKey {
        "queue.failer".into()
    }
}

pub(crate) fn configured_routes() -> std::result::Result<(), openrust::routing::RoutingError> {
    let config = openrust::support::facades::Config::get_facade_root()
        .map_err(|source| openrust::routing::RoutingError::ContainerFailed { source })?;
    if Horizon::manager()
        .map_err(|source| openrust::routing::RoutingError::ContainerFailed { source })?
        .routes_registered
        .load(Ordering::SeqCst)
    {
        return Ok(());
    }
    match config.get("horizon.routes") {
        Some(serde_json::Value::Bool(false)) => Ok(()),
        Some(serde_json::Value::Bool(true)) | None => Horizon::routes(),
        Some(value) => Err(openrust::routing::RoutingError::InvalidUri {
            uri: value.to_string(),
        }),
    }
}

impl From<crate::Error> for HttpError {
    fn from(source: crate::Error) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}
