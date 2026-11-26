pub mod concerns;
mod servers;
mod slow_jobs;
use crate::{Pulse, Result};
use openrust::{
    contracts::{
        config::Repository,
        events::{Dispatcher as _, Event, Listener},
    },
    events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
    support::{facades::Facade, source_path::cargo_package},
};
use serde_json::{Value, json};
pub use servers::{Memory, Servers};
use slow_jobs::SlowJobStarts;
use std::{panic::Location, path::Path, sync::Arc};

#[derive(Clone)]
pub struct Recorders {
    config: Arc<dyn Repository>,
    base_path: Option<std::path::PathBuf>,
    only: Option<&'static str>,
    slow_jobs: Option<Arc<SlowJobStarts>>,
}
impl Recorders {
    pub fn new(config: Arc<dyn Repository>) -> Self {
        Self::configured(config, None)
    }
    fn configured(config: Arc<dyn Repository>, only: Option<&'static str>) -> Self {
        Self {
            config,
            base_path: None,
            only,
            slow_jobs: only
                .is_none_or(|name| name == "SlowJobs")
                .then(Arc::default),
        }
    }
    pub fn with_base_path(mut self, path: std::path::PathBuf) -> Self {
        self.base_path = Some(path);
        self
    }
    fn resolve_location(&self, callers: &[&Location<'_>]) -> String {
        callers
            .iter()
            .find_map(|caller| {
                let path = Path::new(caller.file());
                let path = self
                    .base_path
                    .as_ref()
                    .and_then(|base| path.strip_prefix(base).ok())
                    .unwrap_or(path);
                let normalized = path.to_string_lossy().replace('\\', "/");
                let rooted = format!("/{}", normalized.trim_start_matches('/'));
                let framework = rooted.contains("/vendor/openrust/framework/src/")
                    || rooted.contains("/vendor/openrust/pulse/src/")
                    || matches!(
                        cargo_package(&rooted),
                        Some("openrust-framework" | "openrust-pulse")
                    );
                if framework
                    || matches!(
                        normalized.as_str(),
                        "artisan" | "main.rs" | "public/index.php"
                    )
                {
                    return None;
                }
                Some(format!("{normalized}:{}", caller.line()))
            })
            .unwrap_or_default()
    }
    fn options(&self, name: &str) -> Value {
        if self.only.is_some_and(|selected| selected != name) {
            return Value::Bool(false);
        }
        self.config
            .get(&format!("pulse.recorders.{name}"))
            .unwrap_or(Value::Bool(false))
    }
    fn accepts(
        &self,
        name: &str,
        key: &str,
        duration: Option<i64>,
        seed: Option<&str>,
    ) -> Result<bool> {
        let Some(options) = self.sampled_options(name, seed)? else {
            return Ok(false);
        };
        Self::matches_options(&options, key, duration)
    }

    fn sampled_options(&self, name: &str, seed: Option<&str>) -> Result<Option<Value>> {
        let options = self.options(name);
        if options == Value::Bool(false)
            || options.get("enabled").and_then(Value::as_bool) == Some(false)
        {
            return Ok(None);
        }
        Ok(concerns::sample(
            options
                .get("sample_rate")
                .and_then(Value::as_f64)
                .unwrap_or(1.0),
            seed,
        )?
        .then_some(options))
    }

    fn matches_options(options: &Value, key: &str, duration: Option<i64>) -> Result<bool> {
        if concerns::ignored(options.get("ignore").unwrap_or(&Value::Null), key)? {
            return Ok(false);
        }
        if let Some(duration) = duration {
            return Ok(duration as f64
                >= concerns::threshold(options.get("threshold").unwrap_or(&Value::Null), key)?);
        }
        Ok(true)
    }
    pub async fn record(&self, event: &dyn Event, pulse: &Pulse) -> Result<()> {
        use openrust::{
            cache::events::{CacheHit, CacheMissed},
            database::events::QueryExecuted,
            queue::events::*,
        };
        let timestamp = jiff::Timestamp::now();
        let now = timestamp.as_second();
        if let Some(starts) = &self.slow_jobs {
            starts.observe(event, self, pulse, timestamp).await;
        }
        if let Some(e) = event.downcast_ref::<QueryExecuted>() {
            let duration = e.time.as_millis() as i64;
            let timestamp = (jiff::Timestamp::now().as_millisecond() - duration) / 1000;
            let sql = e.sql.clone();
            let recorder = self.clone();
            let location = if self
                .options("SlowQueries")
                .get("location")
                .and_then(Value::as_bool)
                == Some(false)
            {
                Value::Null
            } else {
                Value::String(self.resolve_location(&e.callers))
            };
            pulse
                .lazy(move |pulse| {
                    Box::pin(async move {
                        if recorder.accepts("SlowQueries", &sql, Some(duration), None)? {
                            let sql = recorder
                                .options("SlowQueries")
                                .get("max_query_length")
                                .and_then(Value::as_u64)
                                .filter(|n| *n > 0)
                                .map(|n| {
                                    openrust::support::Str::limit(&sql, n as usize, "...", false)
                                })
                                .unwrap_or(sql);
                            pulse
                                .record(
                                    "slow_query",
                                    json!([sql, location]).to_string(),
                                    Some(duration),
                                    Some(timestamp),
                                )
                                .await
                                .max()
                                .count();
                        }
                        Ok(())
                    })
                })
                .await;
        } else if let Some(e) =
            event.downcast_ref::<openrust::foundation::http::events::RequestHandled>()
        {
            let Some(route) = e
                .request
                .extensions
                .get::<openrust::routing::RouteMetadata>()
            else {
                return Ok(());
            };
            let user = pulse.resolve_authenticated_user_id();
            let slow_options = self.sampled_options("SlowRequests", None)?;
            let user_options = if user.is_some() {
                self.sampled_options("UserRequests", None)?
            } else {
                None
            };
            if slow_options.is_none() && user_options.is_none() {
                return Ok(());
            }
            let mut path = format!(
                "{}/{}",
                route.domain.as_deref().unwrap_or(""),
                route.uri.trim_start_matches('/')
            );
            let mut via = route.action.clone();
            if e.route_name.as_deref() == Some(crate::http::METRICS_ROUTE)
                && let Some(dashboard) = route
                    .uri
                    .strip_suffix(crate::http::METRICS_PATH.trim_start_matches('/'))
            {
                via = format!("via {path}");
                path = format!("/{}", dashboard.trim_matches('/'));
            } else if e
                .route_name
                .as_deref()
                .is_some_and(|name| name.ends_with("livewire.update"))
                && let Some(snapshot) = e
                    .payload
                    .as_ref()
                    .and_then(|payload| payload.pointer("/components/0/snapshot"))
                    .and_then(Value::as_str)
                    .filter(|snapshot| !matches!(*snapshot, "" | "0"))
            {
                let snapshot: Value = serde_json::from_str(snapshot)?;
                if let Some(component_path) = snapshot.pointer("/memo/path").and_then(Value::as_str)
                {
                    via = format!("via {path}");
                    path = format!("/{}", component_path.trim_start_matches('/'));
                }
            }
            let observation = Arc::new(RequestObservation {
                method: e.request.method.as_str().to_owned(),
                path,
                via,
                timestamp: e.request_started_at / 1000,
                user,
                slow_options,
                user_options,
            });
            if let Ok(owner) = crate::facades::Pulse::get_facade_root()
                && std::ptr::eq(owner.as_ref(), pulse)
            {
                let pending = observation.clone();
                if openrust::foundation::http::after_termination(move |duration| async move {
                    owner.rescue(pending.record(&owner, duration)).await;
                    owner.ingest().await;
                    Ok::<_, crate::Error>(())
                })
                .is_ok()
                {
                    return Ok(());
                }
            }
            observation.record(pulse, e.duration).await?;
        } else if let Some((kind, key)) = event
            .downcast_ref::<CacheHit>()
            .map(|e| ("cache_hit", e.key.as_str()))
            .or_else(|| {
                event
                    .downcast_ref::<CacheMissed>()
                    .map(|e| ("cache_miss", e.key.as_str()))
            })
        {
            let recorder = self.clone();
            let key = key.to_owned();
            pulse
                .lazy(move |pulse| {
                    Box::pin(async move {
                        if recorder.accepts("CacheInteractions", &key, None, None)? {
                            let key = concerns::group_bytes(
                                recorder
                                    .options("CacheInteractions")
                                    .get("groups")
                                    .unwrap_or(&Value::Null),
                                &key,
                            )?;
                            pulse.record_key_bytes(kind, key, None, now).await.count();
                        }
                        Ok(())
                    })
                })
                .await;
        } else if let Some((kind, connection, queue, job)) = event
            .downcast_ref::<JobQueued>()
            .map(|e| ("queued", &e.connection_name, &e.queue, &e.job))
            .or_else(|| {
                event.downcast_ref::<JobProcessing>().map(|e| {
                    (
                        "processing",
                        &e.connection_name,
                        &e.job.queue,
                        &e.job.payload,
                    )
                })
            })
            .or_else(|| {
                event.downcast_ref::<JobProcessed>().map(|e| {
                    (
                        "processed",
                        &e.connection_name,
                        &e.job.queue,
                        &e.job.payload,
                    )
                })
            })
            .or_else(|| {
                event
                    .downcast_ref::<JobFailed>()
                    .map(|e| ("failed", &e.connection_name, &e.job.queue, &e.job.payload))
            })
            .or_else(|| {
                event
                    .downcast_ref::<JobReleasedAfterException>()
                    .map(|e| ("released", &e.connection_name, &e.job.queue, &e.job.payload))
            })
        {
            if connection != "sync" {
                let recorder = self.clone();
                let name = job.display_name().to_owned();
                let uuid = job.uuid().to_owned();
                let connection = connection.clone();
                let queue = queue.clone();
                let user = pulse.authenticated_user_id_resolver();
                pulse
                    .lazy(move |pulse| {
                        Box::pin(async move {
                            if recorder.accepts("Queues", &name, None, Some(&uuid))? {
                                pulse
                                    .record(
                                        kind,
                                        format!(
                                            "{connection}:{}",
                                            recorder.normalize_sqs_queue(&connection, &queue)
                                        ),
                                        None,
                                        Some(now),
                                    )
                                    .await
                                    .count()
                                    .only_buckets();
                            }
                            if kind == "queued"
                                && let Some(user) = user(pulse)
                                && recorder.accepts("UserJobs", &name, None, None)?
                            {
                                pulse
                                    .record("user_job", user, None, Some(now))
                                    .await
                                    .count();
                            }
                            Ok(())
                        })
                    })
                    .await;
            }
        } else if let Some(e) = event.downcast_ref::<crate::events::ExceptionReported>() {
            self.queue_exception(pulse, e.class.clone(), e.location.clone(), now)
                .await;
        } else if let Some(e) = event.downcast_ref::<crate::events::NativeExceptionReported>() {
            use openrust::{
                contracts::debug::Exception,
                support::facades::{App, Exceptions, Facade},
            };
            let handler = Exceptions::get_facade_root()?;
            let (class, location) = exception_group(
                &*handler,
                &Exception::Error(&e.exception),
                &App::base_path("")?,
            );
            self.queue_exception(pulse, class, location, now).await;
        }
        Ok(())
    }
    async fn queue_exception(
        &self,
        pulse: &Pulse,
        class: String,
        location: Option<String>,
        timestamp: i64,
    ) {
        let recorder = self.clone();
        pulse
            .lazy(move |pulse| {
                Box::pin(async move {
                    recorder
                        .exception(pulse, &class, location.as_deref(), timestamp)
                        .await
                })
            })
            .await;
    }
    pub async fn exception(
        &self,
        pulse: &Pulse,
        class: &str,
        location: Option<&str>,
        timestamp: i64,
    ) -> Result<()> {
        if self.accepts("Exceptions", class, None, None)? {
            let location = if self
                .options("Exceptions")
                .get("location")
                .and_then(Value::as_bool)
                == Some(false)
            {
                None
            } else {
                location
            };
            pulse
                .record(
                    "exception",
                    json!([class, location]).to_string(),
                    Some(timestamp),
                    Some(timestamp),
                )
                .await
                .max()
                .count();
        }
        Ok(())
    }
    pub fn normalize_sqs_queue(&self, connection: &str, queue: &str) -> String {
        let config = self
            .config
            .get(&format!("queue.connections.{connection}"))
            .unwrap_or(Value::Null);
        if config.get("driver").and_then(Value::as_str) != Some("sqs") {
            return queue.into();
        }
        let queue = config
            .get("prefix")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .and_then(|prefix| queue.strip_prefix(&format!("{prefix}/")))
            .unwrap_or(queue);
        config
            .get("suffix")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .and_then(|suffix| queue.strip_suffix(suffix))
            .unwrap_or(queue)
            .into()
    }
    pub async fn outgoing(
        &self,
        pulse: &Pulse,
        method: &str,
        url: &str,
        duration: i64,
    ) -> Result<()> {
        let recorder = self.clone();
        let timestamp = jiff::Timestamp::now().as_second();
        let method = method.to_owned();
        let url = SlowOutgoingRequests::normalize_url(url)?;
        pulse
            .lazy(move |pulse| {
                Box::pin(async move {
                    recorder
                        .outgoing_now(pulse, &method, &url, duration, timestamp)
                        .await
                })
            })
            .await;
        Ok(())
    }
    async fn outgoing_now(
        &self,
        pulse: &Pulse,
        method: &str,
        url: &str,
        duration: i64,
        timestamp: i64,
    ) -> Result<()> {
        if self.accepts("SlowOutgoingRequests", url, Some(duration), None)? {
            let url = concerns::group(
                self.options("SlowOutgoingRequests")
                    .get("groups")
                    .unwrap_or(&Value::Null),
                url,
            )?;
            pulse
                .record(
                    "slow_outgoing_request",
                    json!([method, url]).to_string(),
                    Some(duration),
                    Some(timestamp),
                )
                .await
                .max()
                .count();
        }
        Ok(())
    }
}
struct RequestObservation {
    method: String,
    path: String,
    via: String,
    timestamp: i64,
    user: Option<String>,
    slow_options: Option<Value>,
    user_options: Option<Value>,
}

impl RequestObservation {
    async fn record(&self, pulse: &Pulse, duration: std::time::Duration) -> Result<()> {
        let duration = duration.as_millis() as i64;
        if let Some(options) = self.slow_options.as_ref()
            && Recorders::matches_options(options, &self.path, Some(duration))?
        {
            pulse
                .record(
                    "slow_request",
                    json!([self.method, self.path, self.via]).to_string(),
                    Some(duration),
                    Some(self.timestamp),
                )
                .await
                .max()
                .count();
            if let Some(user) = self
                .user
                .as_deref()
                .filter(|user| !matches!(*user, "" | "0"))
            {
                pulse
                    .record("slow_user_request", user, None, Some(self.timestamp))
                    .await
                    .count();
            }
        }
        if let Some(user) = self.user.as_deref()
            && let Some(options) = self.user_options.as_ref()
            && Recorders::matches_options(options, &self.path, None)?
        {
            pulse
                .record("user_request", user, None, Some(self.timestamp))
                .await
                .count();
        }
        Ok(())
    }
}

/// A compiled recorder replaces PHP's runtime class and event reflection.
pub trait Recorder: Send + Sync {
    fn name(&self) -> &'static str;
    fn listens(&self) -> &'static [&'static str];
    fn record<'a>(&'a self, event: &'a dyn Event, pulse: &'a Pulse) -> crate::PulseFuture<'a, ()>;
}
macro_rules! recorder {
    ($name:ident, [$($event:expr),* $(,)?]) => {
        pub struct $name(Recorders);
        impl $name {
            pub fn new(config: Arc<dyn Repository>) -> Self {
                Self(Recorders::configured(config, Some(stringify!($name))))
            }
            pub fn with_base_path(mut self, path: std::path::PathBuf) -> Self { self.0.base_path = Some(path); self }
        }
        impl Recorder for $name {
            fn name(&self) -> &'static str { stringify!($name) }
            fn listens(&self) -> &'static [&'static str] { &[$($event),*] }
            fn record<'a>(&'a self, event: &'a dyn Event, pulse: &'a Pulse) -> crate::PulseFuture<'a, ()> {
                Box::pin(self.0.record(event, pulse))
            }
        }
    }
}
recorder!(
    SlowQueries,
    [openrust::database::events::QueryExecuted::NAME]
);
recorder!(
    CacheInteractions,
    [
        openrust::cache::events::CacheHit::NAME,
        openrust::cache::events::CacheMissed::NAME
    ]
);
recorder!(
    SlowRequests,
    ["OpenRust\\Foundation\\Http\\Events\\RequestHandled"]
);
recorder!(
    UserRequests,
    ["OpenRust\\Foundation\\Http\\Events\\RequestHandled"]
);
recorder!(
    Queues,
    [
        "JobQueued",
        "JobProcessing",
        "JobProcessed",
        "JobFailed",
        "JobReleasedAfterException"
    ]
);
recorder!(UserJobs, ["JobQueued"]);
recorder!(
    SlowJobs,
    [
        "JobProcessing",
        "JobProcessed",
        "JobFailed",
        "JobReleasedAfterException",
        "JobAttempted",
    ]
);
recorder!(
    Exceptions,
    [
        crate::events::ExceptionReported::NAME,
        crate::events::NativeExceptionReported::NAME
    ]
);
recorder!(SlowOutgoingRequests, []);
impl SlowOutgoingRequests {
    /// Removes HTTP URL user information before sampling, grouping and storage.
    pub fn normalize_url(url: &str) -> Result<String> {
        let mut parsed = url::Url::parse(url).map_err(|_| {
            crate::Error::InvalidConfiguration("出站请求地址必须是有效的绝对 URL".into())
        })?;
        if parsed.username().is_empty() && parsed.password().is_none() {
            return Ok(url.into());
        }
        let _ = parsed.set_password(None);
        let _ = parsed.set_username("");
        Ok(parsed.into())
    }

    pub async fn record_outgoing(
        &self,
        pulse: &Pulse,
        method: &str,
        url: &str,
        duration: i64,
    ) -> Result<()> {
        self.0.outgoing(pulse, method, url, duration).await
    }
}
impl Recorder for Servers {
    fn name(&self) -> &'static str {
        "Servers"
    }
    fn listens(&self) -> &'static [&'static str] {
        &["OpenRust\\Pulse\\Events\\SharedBeat"]
    }
    fn record<'a>(&'a self, event: &'a dyn Event, pulse: &'a Pulse) -> crate::PulseFuture<'a, ()> {
        Box::pin(async move {
            if let Some(beat) = event.downcast_ref::<crate::events::SharedBeat>() {
                self.record(beat, pulse).await?;
            }
            Ok(())
        })
    }
}
struct NativeListener {
    recorder: Arc<dyn Recorder>,
    options: Arc<crate::pulse::Options>,
}
impl Listener for NativeListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            if !Pulse::is_ignoring(&self.options)
                && let Ok(pulse) = crate::facades::Pulse::get_facade_root()
                && pulse
                    .automatic_recording
                    .load(std::sync::atomic::Ordering::Relaxed)
                && pulse.is_recording()
                && let Err(error) = self.recorder.record(event, &pulse).await
            {
                pulse.report_error(&error);
            }
            Ok(None)
        })
    }
}
pub(crate) fn register_recorder(
    dispatcher: &openrust::events::Dispatcher,
    recorder: Arc<dyn Recorder>,
    options: Arc<crate::pulse::Options>,
) -> EventResult<()> {
    let listener = Arc::new(NativeListener {
        recorder: recorder.clone(),
        options: options.clone(),
    });
    for name in recorder.listens() {
        dispatcher.listen(name, listener.clone(), Delivery::Sync)?;
    }
    options
        .recorders
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .push(recorder);
    Ok(())
}
pub(crate) fn register(
    dispatcher: &openrust::events::Dispatcher,
    client: &openrust::http::client::Factory,
    config: Arc<dyn Repository>,
    servers: Arc<Servers>,
    base_path: std::path::PathBuf,
    options: Arc<crate::pulse::Options>,
) -> EventResult<()> {
    let outgoing = Arc::new(SlowOutgoingRequests::new(config.clone()));
    let defaults: Vec<Arc<dyn Recorder>> = vec![
        Arc::new(SlowQueries::new(config.clone()).with_base_path(base_path)),
        Arc::new(CacheInteractions::new(config.clone())),
        Arc::new(SlowRequests::new(config.clone())),
        Arc::new(UserRequests::new(config.clone())),
        Arc::new(Queues::new(config.clone())),
        Arc::new(UserJobs::new(config.clone())),
        Arc::new(SlowJobs::new(config.clone())),
        Arc::new(Exceptions::new(config.clone())),
        outgoing.clone(),
        servers,
    ];
    for recorder in defaults {
        let settings = config
            .get(&format!("pulse.recorders.{}", recorder.name()))
            .unwrap_or(Value::Bool(false));
        if settings != Value::Bool(false)
            && settings.get("enabled").and_then(Value::as_bool) != Some(false)
        {
            register_recorder(dispatcher, recorder, options.clone())?;
        }
    }
    client.global_middleware(move |request, next| {
        let outgoing = outgoing.clone();
        let options = options.clone();
        Box::pin(async move {
            let method = request.method().to_string();
            let url = request.url().to_string();
            let started = std::time::Instant::now();
            let result = next.run(request).await;
            if !Pulse::is_ignoring(&options)
                && let Ok(pulse) = crate::facades::Pulse::get_facade_root()
                && pulse
                    .automatic_recording
                    .load(std::sync::atomic::Ordering::Relaxed)
                && pulse.is_recording()
                && let Err(e) = outgoing
                    .record_outgoing(&pulse, &method, &url, started.elapsed().as_millis() as i64)
                    .await
            {
                pulse.report_error(&e);
            }
            result
        })
    });
    Ok(())
}

pub(crate) fn register_exceptions(
    handler: Arc<dyn openrust::contracts::debug::ExceptionHandler>,
    config: Arc<dyn Repository>,
    base_path: std::path::PathBuf,
    options: Arc<crate::pulse::Options>,
) -> std::result::Result<(), openrust::contracts::container::BindingResolutionError> {
    use openrust::contracts::container::BindingResolutionError;
    use openrust::foundation::configuration::ReportDecision;

    let recorders = Arc::new(Recorders::new(config));
    let settings = recorders.options("Exceptions");
    if settings == Value::Bool(false)
        || settings.get("enabled").and_then(Value::as_bool) == Some(false)
    {
        return Ok(());
    }
    let owner = Arc::downgrade(&handler);
    let registered = handler.configure(&mut |configuration| {
        let owner = owner.clone();
        let recorders = recorders.clone();
        let options = options.clone();
        let base_path = base_path.clone();
        configuration.reportable_all(move |exception| {
            if !Pulse::is_ignoring(&options)
                && let Ok(pulse) = crate::facades::Pulse::get_facade_root()
                && pulse.has_options(&options)
                && pulse
                    .automatic_recording
                    .load(std::sync::atomic::Ordering::Relaxed)
                && pulse.is_recording()
                && let Some(handler) = owner.upgrade()
            {
                let (class, location) = exception_group(&*handler, exception, &base_path);
                let recorders = recorders.clone();
                let timestamp = jiff::Timestamp::now().as_second();
                pulse.defer(move |pulse| {
                    Box::pin(async move {
                        recorders
                            .exception(pulse, &class, location.as_deref(), timestamp)
                            .await
                    })
                });
            }
            ReportDecision::Continue
        });
    });
    match registered {
        Err(BindingResolutionError::FactoryFailed {
            service: "ExceptionHandler",
            ref source,
        }) if source
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::Unsupported) =>
        {
            Ok(())
        }
        result => result,
    }
}

fn exception_group(
    handler: &dyn openrust::contracts::debug::ExceptionHandler,
    exception: &openrust::contracts::debug::Exception<'_>,
    base_path: &std::path::Path,
) -> (String, Option<String>) {
    use openrust::{contracts::debug::Exception, view::ViewException};

    if let Some(view) = ViewException::from_exception(exception) {
        let previous = view.previous();
        let class = if previous.is::<ViewException>() {
            std::any::type_name::<ViewException>().to_owned()
        } else {
            let previous = Exception::Error(previous);
            let snapshot = handler.build_log_exception(&previous, previous.type_name(), true);
            snapshot["class"].as_str().unwrap_or("Error").to_owned()
        };
        // 固定上游以最早出现在嵌套错误消息中的视图路径分组。
        let mut origin = view;
        while let Some(nested) = origin.previous().downcast_ref::<ViewException>() {
            origin = nested;
        }
        let path = origin.path();
        let relative = path.strip_prefix(base_path).unwrap_or(path);
        return (class, Some(relative.to_string_lossy().replace('\\', "/")));
    }
    let shared = exception.shared();
    let class = shared
        .and_then(|error| Exception::Error(error).type_name())
        .or_else(|| exception.type_name());
    let snapshot = handler.build_log_exception(exception, class, true);
    let class = snapshot["class"].as_str().unwrap_or("Error").to_owned();
    let location = shared
        .and_then(|error| exception_location(error, base_path))
        .or_else(|| snapshot["location"].as_str().map(str::to_owned));
    (class, location)
}

fn exception_location(
    error: &openrust::contracts::debug::SharedException,
    base_path: &std::path::Path,
) -> Option<String> {
    let relative_file = |location: &std::panic::Location<'_>| {
        let path = std::path::Path::new(location.file());
        path.strip_prefix(base_path)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let internal =
        |file: &str| file.starts_with("vendor") || matches!(file, "artisan" | "public/index.php");
    let origin = error.origin()?;
    let file = relative_file(origin);
    if internal(&file)
        && let Some((file, line)) = error.callers().iter().find_map(|caller| {
            let file = relative_file(caller);
            (!internal(&file)).then(|| (file, caller.line()))
        })
    {
        return Some(format!("{file}:{line}"));
    }
    Some(format!("{file}:{}", origin.line()))
}
