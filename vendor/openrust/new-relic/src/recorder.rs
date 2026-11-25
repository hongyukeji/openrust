use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use openrust::{
    contracts::events::{Event, Listener},
    events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::{
        events::ApplicationTerminating,
        http::events::RequestHandled,
        observability::{Observation, Observer, Operation, Outcome},
    },
    queue::events::JobAttempted,
    routing::RouteMetadata,
};
use opentelemetry::{
    Context, KeyValue,
    metrics::{Histogram, MeterProvider as _},
    propagation::TextMapPropagator as _,
    trace::{
        SpanBuilder, SpanKind, Status, TraceContextExt as _, Tracer as _, TracerProvider as _,
    },
};
use opentelemetry_http::{HeaderExtractor, HeaderInjector};
use opentelemetry_otlp::{
    MetricExporter, Protocol, SpanExporter, WithExportConfig as _, WithHttpConfig as _,
};
use opentelemetry_sdk::{
    Resource,
    metrics::{PeriodicReader, SdkMeterProvider, Temporality},
    propagation::TraceContextPropagator,
    trace::{BatchConfigBuilder, BatchSpanProcessor, SdkTracer, SdkTracerProvider},
};

use crate::export::{Destination, EXPORT_TIMEOUT};

const SCOPE: &str = "openrust.new_relic";
const MAX_PAYLOAD_BYTES: usize = 999_999;

pub(crate) struct Recorder {
    tracer_provider: SdkTracerProvider,
    meter_provider: SdkMeterProvider,
    tracer: SdkTracer,
    http_duration: Histogram<f64>,
    command_duration: Histogram<f64>,
    job_duration: Histogram<f64>,
    stopped: AtomicBool,
    owner: Arc<()>,
    propagate_origins: Vec<url::Origin>,
    client_duration: Histogram<f64>,
    sql_duration: Histogram<f64>,
}

impl Recorder {
    pub(crate) fn new(
        name: String,
        destination: Destination,
        propagate_origins: Vec<url::Origin>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // 使用 SDK 的有界 span 队列与周期指标聚合，导出不占用请求执行线程。
        let client = destination.client()?;
        let spans = SpanExporter::builder()
            .with_http()
            .with_protocol(Protocol::HttpBinary)
            .with_timeout(EXPORT_TIMEOUT)
            .with_max_request_body_size(MAX_PAYLOAD_BYTES)
            .with_endpoint(destination.endpoint.join("/v1/traces")?.to_string())
            .with_http_client(client.clone())
            .build()?;
        let metrics = MetricExporter::builder()
            .with_http()
            .with_protocol(Protocol::HttpBinary)
            .with_timeout(EXPORT_TIMEOUT)
            .with_max_request_body_size(MAX_PAYLOAD_BYTES)
            .with_endpoint(destination.endpoint.join("/v1/metrics")?.to_string())
            .with_http_client(client)
            .with_temporality(Temporality::Delta)
            .build()?;
        let resource = Resource::builder().with_service_name(name).build();
        let tracer_provider = SdkTracerProvider::builder()
            .with_resource(resource.clone())
            .with_span_processor(
                BatchSpanProcessor::builder(spans)
                    .with_batch_config(
                        BatchConfigBuilder::default()
                            .with_max_queue_size(2048)
                            .with_max_export_batch_size(512)
                            .build(),
                    )
                    .build(),
            )
            .build();
        let meter_provider = SdkMeterProvider::builder()
            .with_resource(resource)
            .with_reader(
                PeriodicReader::builder(metrics)
                    .with_interval(Duration::from_secs(30))
                    .build(),
            )
            .build();
        let tracer = tracer_provider.tracer(SCOPE);
        let meter = meter_provider.meter(SCOPE);
        let duration = |name: &'static str| meter.f64_histogram(name).with_unit("s").build();
        Ok(Self {
            tracer_provider,
            meter_provider,
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
            propagate_origins,
            client_duration: duration("http.client.request.duration"),
            sql_duration: duration("db.client.operation.duration"),
        })
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
            context.span().update_name(format!("{operation} {route}"));
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
    Command,
    Job,
}

struct Active {
    owner: Arc<()>,
    kind: OperationKind,
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
            Operation::Job { name } => (
                SpanBuilder::from_name(format!("queue {name}"))
                    .with_attributes([KeyValue::new("openrust.queue.job", name.to_owned())]),
                local(),
                OperationKind::Job,
                None,
            ),
            _ => return None,
        };
        let span = self.tracer.build_with_context(builder, &parent);
        let context = parent.with_span(span).with_value(Active {
            owner: self.owner.clone(),
            kind,
        });
        if let Some(headers) = inject
            && !headers.contains_key("traceparent")
        {
            TraceContextPropagator::new().inject_context(&context, &mut HeaderInjector(headers));
        }
        Some(Box::new(SpanObservation {
            context,
            started: Instant::now(),
            command,
            measured,
        }))
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
            Outcome::Job { failed } => failed,
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
        if failed {
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
                && !self.stopped.swap(true, Ordering::AcqRel)
            {
                let spans = self.tracer_provider.clone();
                let metrics = self.meter_provider.clone();
                let span_task = tokio::task::spawn_blocking(move || {
                    spans.shutdown_with_timeout(EXPORT_TIMEOUT)
                });
                let metric_task = tokio::task::spawn_blocking(move || {
                    metrics.shutdown_with_timeout(EXPORT_TIMEOUT)
                });
                let (spans, metrics) = tokio::join!(span_task, metric_task);
                if !matches!(spans, Ok(Ok(()))) || !matches!(metrics, Ok(Ok(()))) {
                    eprintln!("New Relic 遥测未能完整刷新");
                }
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
