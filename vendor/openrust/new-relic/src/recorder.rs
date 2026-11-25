use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use openrust::{
    contracts::events::{Event, Listener},
    events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::{
        events::ApplicationTerminating,
        http::events::RequestHandled,
        observability::{JobOutcome, Observation, Observer, Operation, Outcome, TraceContext},
    },
    queue::events::JobAttempted,
    routing::RouteMetadata,
};
use opentelemetry::{
    Context, KeyValue,
    logs::LoggerProvider as _,
    metrics::{Histogram, MeterProvider as _},
    propagation::TextMapPropagator as _,
    trace::{
        Link, SpanBuilder, SpanKind, Status, TraceContextExt as _, Tracer as _, TracerProvider as _,
    },
};
use opentelemetry_http::{HeaderExtractor, HeaderInjector};
use opentelemetry_otlp::{
    LogExporter, MetricExporter, Protocol, SpanExporter, WithExportConfig as _, WithHttpConfig as _,
};
use opentelemetry_sdk::{
    Resource,
    logs::{SdkLogger, SdkLoggerProvider},
    metrics::{PeriodicReader, SdkMeterProvider, Temporality},
    propagation::TraceContextPropagator,
    trace::{BatchConfigBuilder, BatchSpanProcessor, Sampler, SdkTracer, SdkTracerProvider},
};

use crate::export::{Destination, EXPORT_TIMEOUT, ExportStats, Transport};

const SCOPE: &str = "openrust.new_relic";
const MAX_PAYLOAD_BYTES: usize = 999_999;

pub(crate) struct Settings {
    pub(crate) name: String,
    pub(crate) version: Option<String>,
    pub(crate) instance_id: Option<String>,
    pub(crate) propagate_origins: Vec<url::Origin>,
    pub(crate) exception_stacktrace: bool,
    pub(crate) logs_enabled: bool,
    pub(crate) traces_enabled: bool,
    pub(crate) metrics_enabled: bool,
    pub(crate) ratio: f64,
}

pub(crate) struct Recorder {
    export_client: Transport,
    tracer_provider: SdkTracerProvider,
    meter_provider: SdkMeterProvider,
    logger_provider: Option<SdkLoggerProvider>,
    tracer: SdkTracer,
    http_duration: Histogram<f64>,
    command_duration: Histogram<f64>,
    job_duration: Histogram<f64>,
    stopped: AtomicBool,
    owner: Arc<()>,
    propagate_origins: Vec<url::Origin>,
    client_duration: Histogram<f64>,
    sql_duration: Histogram<f64>,
    exception_stacktrace: bool,
}

impl Recorder {
    pub(crate) fn new(
        settings: Settings,
        destination: Destination,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // 使用 SDK 的有界 span 队列与周期指标聚合，导出不占用请求执行线程。
        let client = destination.client()?;
        let mut attributes = vec![KeyValue::new("service.name", settings.name)];
        if let Some(version) = settings.version {
            attributes.push(KeyValue::new("service.version", version));
        }
        if let Some(instance) = settings.instance_id {
            attributes.push(KeyValue::new("service.instance.id", instance));
        }
        let resource = Resource::builder().with_attributes(attributes).build();
        let logger_provider = if settings.logs_enabled {
            let exporter = LogExporter::builder()
                .with_http()
                .with_protocol(Protocol::HttpBinary)
                .with_timeout(EXPORT_TIMEOUT)
                .with_max_request_body_size(MAX_PAYLOAD_BYTES)
                .with_endpoint(destination.endpoint.join("/v1/logs")?.to_string())
                .with_http_client(client.clone())
                .build()?;
            Some(
                SdkLoggerProvider::builder()
                    .with_resource(resource.clone())
                    .with_batch_exporter(exporter)
                    .build(),
            )
        } else {
            None
        };
        let mut tracer_provider = SdkTracerProvider::builder().with_resource(resource.clone());
        if settings.traces_enabled {
            let exporter = SpanExporter::builder()
                .with_http()
                .with_protocol(Protocol::HttpBinary)
                .with_timeout(EXPORT_TIMEOUT)
                .with_max_request_body_size(MAX_PAYLOAD_BYTES)
                .with_endpoint(destination.endpoint.join("/v1/traces")?.to_string())
                .with_http_client(client.clone())
                .build()?;
            tracer_provider = tracer_provider
                .with_sampler(Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(
                    settings.ratio,
                ))))
                .with_span_processor(
                    BatchSpanProcessor::builder(exporter)
                        .with_batch_config(
                            BatchConfigBuilder::default()
                                .with_max_queue_size(2048)
                                .with_max_export_batch_size(512)
                                .build(),
                        )
                        .build(),
                );
        } else {
            tracer_provider = tracer_provider.with_sampler(Sampler::AlwaysOff);
        }
        let tracer_provider = tracer_provider.build();
        let mut meter_provider = SdkMeterProvider::builder().with_resource(resource);
        if settings.metrics_enabled {
            let exporter = MetricExporter::builder()
                .with_http()
                .with_protocol(Protocol::HttpBinary)
                .with_timeout(EXPORT_TIMEOUT)
                .with_max_request_body_size(MAX_PAYLOAD_BYTES)
                .with_endpoint(destination.endpoint.join("/v1/metrics")?.to_string())
                .with_http_client(client.clone())
                .with_temporality(Temporality::Delta)
                .build()?;
            meter_provider = meter_provider.with_reader(
                PeriodicReader::builder(exporter)
                    .with_interval(Duration::from_secs(30))
                    .build(),
            );
        }
        let meter_provider = meter_provider.build();
        let tracer = tracer_provider.tracer(SCOPE);
        let meter = meter_provider.meter(SCOPE);
        let duration = |name: &'static str| meter.f64_histogram(name).with_unit("s").build();
        Ok(Self {
            export_client: client,
            tracer_provider,
            meter_provider,
            logger_provider,
            tracer,
            http_duration: meter
                .f64_histogram("http.server.request.duration")
                .with_unit("s")
                .with_boundaries(vec![
                    0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 7.5, 10.0,
                ])
                .build(),
            command_duration: duration("openrust.artisan.command.duration"),
            job_duration: duration("openrust.queue.job.duration"),
            stopped: AtomicBool::new(false),
            owner: Arc::new(()),
            propagate_origins: settings.propagate_origins,
            client_duration: duration("http.client.request.duration"),
            sql_duration: duration("db.client.operation.duration"),
            exception_stacktrace: settings.exception_stacktrace,
        })
    }

    fn transaction<'a>(&self, context: &'a Context) -> Option<&'a Active> {
        context.get::<Active>().filter(|active| {
            Arc::ptr_eq(&self.owner, &active.owner)
                && matches!(
                    active.kind,
                    OperationKind::Http
                        | OperationKind::Command
                        | OperationKind::Job
                        | OperationKind::JobExecution
                )
        })
    }

    pub(crate) fn name_transaction(&self, name: &str) -> bool {
        let context = Context::current();
        if name.trim().is_empty() || name.len() > 128 {
            return false;
        }
        let Some(active) = self.transaction(&context) else {
            return false;
        };
        active.renamed.store(true, Ordering::Relaxed);
        context.span().update_name(name.to_owned());
        true
    }

    pub(crate) fn add_attribute(&self, key: &str, value: &str) -> bool {
        let context = Context::current();
        let Some(active) = self.transaction(&context) else {
            return false;
        };
        if key.is_empty()
            || key.len() > 64
            || value.len() > 512
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            || active
                .attributes
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                    (count < 32).then_some(count + 1)
                })
                .is_err()
        {
            return false;
        }
        context
            .span()
            .set_attribute(KeyValue::new(format!("custom.{key}"), value.to_owned()));
        true
    }

    pub(crate) fn report_log(&self, kind: &str, trace: Option<&TraceContext>) {
        let parent = trace.map_or_else(Context::new, |trace| {
            TraceContextPropagator::new()
                .extract_with_context(&Context::new(), &HeaderExtractor(&trace.headers()))
        });
        let kind = kind.chars().take(128).collect::<String>();
        let span = Context::new().with_span(self.tracer.build_with_context(
            SpanBuilder::from_name("log error").with_kind(SpanKind::Internal),
            &parent,
        ));
        span.span().set_status(Status::error(kind.clone()));
        span.span()
            .add_event("exception", vec![KeyValue::new("exception.type", kind)]);
        span.span().end();
    }

    pub(crate) fn export_stats(&self) -> ExportStats {
        self.export_client.stats()
    }

    pub(crate) async fn export_pending(&self, shutdown: bool) -> bool {
        if shutdown {
            if self.stopped.swap(true, Ordering::AcqRel) {
                return true;
            }
        } else if self.stopped.load(Ordering::Acquire) {
            return false;
        }
        let spans = self.tracer_provider.clone();
        let metrics = self.meter_provider.clone();
        let span_task = tokio::task::spawn_blocking(move || {
            if shutdown {
                spans.shutdown_with_timeout(EXPORT_TIMEOUT)
            } else {
                spans.force_flush()
            }
        });
        let metric_task = tokio::task::spawn_blocking(move || {
            if shutdown {
                metrics.shutdown_with_timeout(EXPORT_TIMEOUT)
            } else {
                metrics.force_flush()
            }
        });
        let log_task = self.logger_provider.clone().map(|logs| {
            tokio::task::spawn_blocking(move || {
                if shutdown {
                    logs.shutdown_with_timeout(EXPORT_TIMEOUT)
                } else {
                    logs.force_flush()
                }
            })
        });
        let finished = async {
            let (spans, metrics, logs) = tokio::join!(span_task, metric_task, async {
                match log_task {
                    Some(task) => task.await.is_ok_and(|result| result.is_ok()),
                    None => true,
                }
            });
            matches!(spans, Ok(Ok(()))) && matches!(metrics, Ok(Ok(()))) && logs
        };
        tokio::time::timeout(EXPORT_TIMEOUT + Duration::from_secs(3), finished)
            .await
            .unwrap_or(false)
    }

    pub(crate) fn logger(&self) -> Option<SdkLogger> {
        self.logger_provider
            .as_ref()
            .map(|provider| provider.logger(SCOPE))
    }

    fn active(&self, context: &Context, kind: OperationKind) -> bool {
        context
            .get::<Active>()
            .is_some_and(|active| Arc::ptr_eq(&self.owner, &active.owner) && active.kind == kind)
    }

    fn request(&self, request: &RequestHandled) {
        let context = Context::current();
        if !self.active(&context, OperationKind::Http) {
            return;
        }
        let method = http_method(&request.request.method);
        let status = request.response.status;
        let mut attributes = vec![
            KeyValue::new("http.request.method", method),
            KeyValue::new("http.response.status_code", i64::from(status.as_u16())),
        ];
        let route = request
            .request
            .extensions
            .get::<RouteMetadata>()
            .map(|route| route.uri.as_str());
        if let Some(route) = route {
            attributes.push(KeyValue::new("http.route", route.to_owned()));
            let operation = if method == "_OTHER" { "HTTP" } else { method };
            if context
                .get::<Active>()
                .is_none_or(|active| !active.renamed.load(Ordering::Relaxed))
            {
                context.span().update_name(format!("{operation} {route}"));
            }
        }
        if status.is_server_error() {
            attributes.push(KeyValue::new("error.type", status.as_u16().to_string()));
        }
        self.http_duration
            .record(request.duration.as_secs_f64(), &attributes);
        context.span().set_attributes(attributes);
    }

    fn job(&self, event: &JobAttempted) {
        let context = Context::current();
        if !self.active(&context, OperationKind::Job) {
            return;
        }
        let attributes = vec![
            KeyValue::new("openrust.queue.job", event.job.payload.name().to_owned()),
            KeyValue::new(
                "openrust.queue.result",
                if event.successful() {
                    "success"
                } else {
                    "error"
                },
            ),
        ];
        if let Some(duration) = event
            .duration
            .and_then(|value| Duration::try_from_secs_f64(value).ok())
        {
            self.job_duration
                .record(duration.as_secs_f64(), &attributes);
        }
        context.span().set_attributes(attributes);
        context.span().set_attribute(KeyValue::new(
            "openrust.queue.attempt",
            i64::from(event.job.attempts),
        ));
        if !event.successful() {
            context.span().set_status(Status::error(""));
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OperationKind {
    Http,
    HttpClient,
    Sql,
    Dispatch,
    JobExecution,
    Command,
    Job,
}

struct Active {
    owner: Arc<()>,
    kind: OperationKind,
    errors: Mutex<HashSet<usize>>,
    renamed: AtomicBool,
    attributes: AtomicUsize,
}

impl Observer for Recorder {
    fn begin(&self, operation: Operation<'_>) -> Option<Box<dyn Observation>> {
        if self.stopped.load(Ordering::Acquire) {
            return None;
        }
        let ambient = Context::current();
        let local = || {
            if ambient
                .get::<Active>()
                .is_some_and(|active| Arc::ptr_eq(&self.owner, &active.owner))
            {
                ambient.clone()
            } else {
                Context::new()
            }
        };
        let mut inject = None;
        let mut carrier = None;
        let mut measured = None;
        let (builder, parent, kind, command) = match operation {
            Operation::Http { method, headers } => {
                let method = http_method(method);
                let operation = if method == "_OTHER" { "HTTP" } else { method };
                let parent = TraceContextPropagator::new()
                    .extract_with_context(&Context::new(), &HeaderExtractor(headers));
                (
                    SpanBuilder::from_name(operation)
                        .with_kind(SpanKind::Server)
                        .with_attributes([KeyValue::new("http.request.method", method)]),
                    parent,
                    OperationKind::Http,
                    None,
                )
            }
            Operation::HttpClient {
                method,
                origin,
                headers,
            } => {
                let method = http_method(method);
                let mut attributes = vec![KeyValue::new("http.request.method", method)];
                if let url::Origin::Tuple(_, host, port) = origin {
                    attributes.push(KeyValue::new("server.address", host.to_string()));
                    attributes.push(KeyValue::new("server.port", i64::from(*port)));
                }
                if self.propagate_origins.contains(origin) {
                    inject = Some(headers);
                }
                measured = Some((attributes.clone(), self.client_duration.clone()));
                (
                    SpanBuilder::from_name(if method == "_OTHER" { "HTTP" } else { method })
                        .with_kind(SpanKind::Client)
                        .with_attributes(attributes),
                    local(),
                    OperationKind::HttpClient,
                    None,
                )
            }
            Operation::Sql { system, operation } => {
                let system = match system {
                    "pgsql" => "postgresql",
                    "sqlsrv" => "microsoft.sql_server",
                    value => value,
                };
                let mut attributes = vec![KeyValue::new("db.system.name", system)];
                if let Some(operation) = operation {
                    attributes.push(KeyValue::new("db.operation.name", operation));
                }
                measured = Some((attributes.clone(), self.sql_duration.clone()));
                (
                    SpanBuilder::from_name(operation.unwrap_or(system))
                        .with_kind(SpanKind::Client)
                        .with_attributes(attributes),
                    local(),
                    OperationKind::Sql,
                    None,
                )
            }
            Operation::Command { name } => (
                SpanBuilder::from_name(format!("artisan {name}"))
                    .with_attributes([KeyValue::new("openrust.artisan.command", name.to_owned())]),
                local(),
                OperationKind::Command,
                Some((name.to_owned(), self.command_duration.clone())),
            ),
            Operation::JobDispatch { destination, trace } => {
                // 已带创建上下文的任务保持原有关联。
                if trace.is_none() {
                    carrier = Some(trace);
                }
                (
                    SpanBuilder::from_name("queue send")
                        .with_kind(SpanKind::Producer)
                        .with_attributes([
                            KeyValue::new("messaging.system", "openrust.queue"),
                            KeyValue::new("messaging.system.name", "openrust.queue"),
                            KeyValue::new("messaging.operation.name", "send"),
                            KeyValue::new("messaging.operation.type", "send"),
                            KeyValue::new("messaging.destination.name", destination.to_owned()),
                        ]),
                    local(),
                    OperationKind::Dispatch,
                    None,
                )
            }
            Operation::JobCreate { destination, trace } => {
                // 已带创建上下文的任务保持原有关联。
                if trace.is_none() {
                    carrier = Some(trace);
                }
                (
                    SpanBuilder::from_name("queue create")
                        .with_kind(SpanKind::Producer)
                        .with_attributes([
                            KeyValue::new("messaging.system", "openrust.queue"),
                            KeyValue::new("messaging.system.name", "openrust.queue"),
                            KeyValue::new("messaging.operation.name", "create"),
                            KeyValue::new("messaging.operation.type", "create"),
                            KeyValue::new("messaging.destination.name", destination.to_owned()),
                        ]),
                    local(),
                    OperationKind::Dispatch,
                    None,
                )
            }
            Operation::Job { name, trace } => {
                let parent = local();
                let link = trace.and_then(|trace| {
                    let context = TraceContextPropagator::new()
                        .extract_with_context(&Context::new(), &HeaderExtractor(&trace.headers()));
                    let remote = context.span().span_context().clone();
                    let current = parent.span().span_context().clone();
                    (remote.is_valid() && remote != current).then(|| Link::with_context(remote))
                });
                (
                    SpanBuilder::from_name(format!("queue {name}"))
                        .with_kind(SpanKind::Consumer)
                        .with_links(link.into_iter().collect())
                        .with_attributes([
                            KeyValue::new("openrust.queue.job", name.to_owned()),
                            KeyValue::new("messaging.system", "openrust.queue"),
                            KeyValue::new("messaging.system.name", "openrust.queue"),
                            KeyValue::new("messaging.operation.name", "process"),
                            KeyValue::new("messaging.operation.type", "process"),
                        ]),
                    parent,
                    OperationKind::Job,
                    None,
                )
            }
            Operation::JobExecution { name, parent } => {
                let parent = parent.map_or_else(Context::new, |parent| {
                    TraceContextPropagator::new()
                        .extract_with_context(&Context::new(), &HeaderExtractor(&parent.headers()))
                });
                (
                    SpanBuilder::from_name(format!("queue execute {name}")),
                    parent,
                    OperationKind::JobExecution,
                    None,
                )
            }
            _ => return None,
        };
        let span = self.tracer.build_with_context(builder, &parent);
        let context = parent.with_span(span).with_value(Active {
            owner: self.owner.clone(),
            kind,
            errors: Mutex::new(HashSet::new()),
            renamed: AtomicBool::new(false),
            attributes: AtomicUsize::new(0),
        });
        if let Some(headers) = inject
            && !headers.contains_key("traceparent")
        {
            TraceContextPropagator::new().inject_context(&context, &mut HeaderInjector(headers));
        }
        if let Some(carrier) = carrier {
            let mut headers = reqwest::header::HeaderMap::new();
            TraceContextPropagator::new()
                .inject_context(&context, &mut HeaderInjector(&mut headers));
            *carrier = TraceContext::from_headers(&headers);
        }
        Some(Box::new(SpanObservation {
            context,
            started: Instant::now(),
            command,
            measured,
        }))
    }
    fn report(&self, identity: usize, snapshot: &serde_json::Value) {
        if self.stopped.load(Ordering::Acquire) {
            return;
        }
        let kind = snapshot
            .get("class")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Error");
        let safe_kind = kind.chars().take(256).collect::<String>();
        let mut attributes = vec![
            KeyValue::new("exception.type", safe_kind.clone()),
            KeyValue::new("exception.message", format!("应用异常：{safe_kind}")),
        ];
        // 只有应用已经在异常源提供的堆栈才会送出，不在报告位置伪造。
        if self.exception_stacktrace
            && let Some(trace) = snapshot
                .get("stacktrace")
                .and_then(serde_json::Value::as_str)
        {
            attributes.push(KeyValue::new(
                "exception.stacktrace",
                trace.chars().take(1024).collect::<String>(),
            ));
        }
        let active = Context::current();
        if let Some(operation) = active.get::<Active>()
            && Arc::ptr_eq(&self.owner, &operation.owner)
        {
            let mut identities = operation
                .errors
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if identities.len() >= 64 || !identities.insert(identity) {
                return;
            }
            drop(identities);
            active.span().set_status(Status::error(safe_kind));
            active.span().add_event("exception", attributes);
        } else {
            let span = Context::new().with_span(self.tracer.build_with_context(
                SpanBuilder::from_name("exception").with_kind(SpanKind::Internal),
                &Context::new(),
            ));
            span.span().set_status(Status::error(safe_kind));
            span.span().add_event("exception", attributes);
            span.span().end();
        }
    }

    fn current_trace(&self) -> Option<TraceContext> {
        let context = Context::current();
        if !context
            .get::<Active>()
            .is_some_and(|active| Arc::ptr_eq(&self.owner, &active.owner))
        {
            return None;
        }
        let mut headers = reqwest::header::HeaderMap::new();
        TraceContextPropagator::new().inject_context(&context, &mut HeaderInjector(&mut headers));
        TraceContext::from_headers(&headers)
    }
}

struct SpanObservation {
    context: Context,
    started: Instant,
    command: Option<(String, Histogram<f64>)>,
    measured: Option<(Vec<KeyValue>, Histogram<f64>)>,
}

impl Observation for SpanObservation {
    fn in_scope(&self, poll: &mut dyn FnMut()) {
        let _guard = self.context.clone().attach();
        poll();
    }

    fn finish(&mut self, outcome: Outcome) {
        let span = self.context.span();
        let failed = match outcome {
            Outcome::Http(status) => {
                span.set_attribute(KeyValue::new(
                    "http.response.status_code",
                    i64::from(status.as_u16()),
                ));
                if status.is_server_error() {
                    span.set_attribute(KeyValue::new("error.type", status.as_u16().to_string()));
                }
                status.is_server_error()
            }
            Outcome::HttpClient(result) => {
                let mut outcome_attributes = Vec::new();
                let error = match result {
                    Ok(status) => {
                        outcome_attributes.push(KeyValue::new(
                            "http.response.status_code",
                            i64::from(status.as_u16()),
                        ));
                        (status.is_client_error() || status.is_server_error())
                            .then(|| status.as_u16().to_string())
                    }
                    Err(category) => Some(category.to_owned()),
                };
                if let Some(error) = &error {
                    outcome_attributes.push(KeyValue::new("error.type", error.clone()));
                }
                span.set_attributes(outcome_attributes.iter().cloned());
                if let Some((attributes, histogram)) = &mut self.measured {
                    attributes.extend(outcome_attributes);
                    histogram.record(self.started.elapsed().as_secs_f64(), attributes);
                }
                error.is_some()
            }
            Outcome::Sql { failed } => {
                if let Some((attributes, histogram)) = &mut self.measured {
                    if failed {
                        let error = KeyValue::new("error.type", "database");
                        span.set_attribute(error.clone());
                        attributes.push(error);
                    }
                    histogram.record(self.started.elapsed().as_secs_f64(), attributes);
                }
                failed
            }
            Outcome::Command(code) => {
                span.set_attribute(KeyValue::new("process.exit.code", i64::from(code)));
                if let Some((name, histogram)) = &self.command {
                    histogram.record(
                        self.started.elapsed().as_secs_f64(),
                        &[
                            KeyValue::new("openrust.artisan.command", name.clone()),
                            KeyValue::new("process.exit.code", i64::from(code)),
                        ],
                    );
                }
                code != 0
            }
            Outcome::Job(outcome) => {
                let (label, failed) = match outcome {
                    JobOutcome::Processed => ("processed", false),
                    JobOutcome::Released => ("released", false),
                    JobOutcome::TimedOut => ("timed_out", true),
                    JobOutcome::Failed => ("failed", true),
                    JobOutcome::Idle => ("idle", false),
                    _ => ("unknown", false),
                };
                span.set_attribute(KeyValue::new("openrust.queue.outcome", label));
                failed
            }
            Outcome::JobDispatch { failed } => failed,
            Outcome::Failed => {
                if let Some((name, histogram)) = &self.command {
                    histogram.record(
                        self.started.elapsed().as_secs_f64(),
                        &[
                            KeyValue::new("openrust.artisan.command", name.clone()),
                            KeyValue::new("error.type", "command"),
                        ],
                    );
                }
                true
            }
            Outcome::Cancelled => {
                span.add_event("openrust.operation.cancelled", Vec::new());
                false
            }
            _ => false,
        };
        if failed
            && self.context.get::<Active>().is_none_or(|active| {
                active
                    .errors
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .is_empty()
            })
        {
            span.set_status(Status::error(""));
        }
        span.end();
    }
}

fn http_method(method: &reqwest::Method) -> &'static str {
    match method.as_str() {
        "GET" => "GET",
        "HEAD" => "HEAD",
        "POST" => "POST",
        "PUT" => "PUT",
        "DELETE" => "DELETE",
        "CONNECT" => "CONNECT",
        "OPTIONS" => "OPTIONS",
        "TRACE" => "TRACE",
        "PATCH" => "PATCH",
        "QUERY" => "QUERY",
        _ => "_OTHER",
    }
}

impl Listener for Recorder {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            if self.stopped.load(Ordering::Acquire) {
                return Ok(None);
            }
            if let Some(request) = event.downcast_ref::<RequestHandled>() {
                self.request(request);
            } else if let Some(job) = event.downcast_ref::<JobAttempted>() {
                self.job(job);
            } else if event.downcast_ref::<ApplicationTerminating>().is_some()
                && !self.export_pending(true).await
            {
                eprintln!("New Relic 遥测未能完整刷新");
            }
            Ok(None)
        })
    }
}

pub(crate) fn register(dispatcher: &Dispatcher, recorder: Arc<Recorder>) -> EventResult<()> {
    dispatcher.register(|registry| {
        for event in [
            r"OpenRust\Foundation\Http\Events\RequestHandled",
            JobAttempted::NAME,
            ApplicationTerminating::NAME,
        ] {
            registry.listen(event, recorder.clone(), Delivery::Sync)?;
        }
        Ok(())
    })
}
