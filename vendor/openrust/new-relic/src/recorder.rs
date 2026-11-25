use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use openrust::{
    console::events::CommandFinished,
    contracts::events::{Event, Listener},
    events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::{events::ApplicationTerminating, http::events::RequestHandled},
    queue::events::JobAttempted,
    routing::RouteMetadata,
};
use opentelemetry::{
    Context, KeyValue,
    metrics::{Histogram, MeterProvider as _},
    propagation::TextMapPropagator as _,
    trace::{Span as _, SpanBuilder, SpanKind, Status, Tracer as _, TracerProvider as _},
};
use opentelemetry_http::HeaderExtractor;
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
}

impl Recorder {
    pub(crate) fn new(
        name: String,
        destination: Destination,
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
        })
    }

    fn request(&self, request: &RequestHandled) {
        let method = match request.request.method.as_str() {
            method @ ("GET" | "HEAD" | "POST" | "PUT" | "DELETE" | "CONNECT" | "OPTIONS"
            | "TRACE" | "PATCH" | "QUERY") => method.to_owned(),
            _ => "_OTHER".into(),
        };
        let status = request.response.status;
        let failed = status.is_server_error();
        let mut attrs = vec![
            KeyValue::new("http.request.method", method.clone()),
            KeyValue::new("http.response.status_code", i64::from(status.as_u16())),
        ];
        let route = request
            .request
            .extensions
            .get::<RouteMetadata>()
            .map(|route| route.uri.as_str());
        if let Some(route) = route {
            attrs.push(KeyValue::new("http.route", route.to_owned()));
        }
        if failed {
            attrs.push(KeyValue::new("error.type", status.as_u16().to_string()));
        }
        let operation = if method == "_OTHER" { "HTTP" } else { &method };
        let span_name = route.map_or_else(
            || operation.to_owned(),
            |route| format!("{operation} {route}"),
        );
        let parent = TraceContextPropagator::new()
            .extract_with_context(&Context::new(), &HeaderExtractor(&request.request.headers));
        let start = UNIX_EPOCH
            .checked_add(Duration::from_millis(
                u64::try_from(request.request_started_at).unwrap_or_default(),
            ))
            .unwrap_or_else(SystemTime::now);
        self.http_duration
            .record(request.duration.as_secs_f64(), &attrs);
        self.finish_span(
            SpanBuilder::from_name(span_name)
                .with_kind(SpanKind::Server)
                .with_start_time(start)
                .with_attributes(attrs),
            &parent,
            start.checked_add(request.duration).unwrap_or(start),
            failed,
        );
    }

    fn command(&self, event: &CommandFinished) {
        let failed = event.exit_code != 0;
        let attrs = vec![
            KeyValue::new("openrust.artisan.command", event.command.clone()),
            KeyValue::new("process.exit.code", i64::from(event.exit_code)),
        ];
        self.command_duration
            .record(event.duration.as_secs_f64(), &attrs);
        self.background(
            format!("artisan {}", event.command),
            event.duration,
            attrs,
            failed,
        );
    }

    fn job(&self, event: &JobAttempted) {
        let attrs = vec![
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
            self.job_duration.record(duration.as_secs_f64(), &attrs);
            self.background(
                format!("queue {}", event.job.payload.name()),
                duration,
                attrs,
                !event.successful(),
            );
        }
    }

    fn background(&self, name: String, duration: Duration, attrs: Vec<KeyValue>, failed: bool) {
        let end = SystemTime::now();
        let start = end.checked_sub(duration).unwrap_or(end);
        self.finish_span(
            SpanBuilder::from_name(name)
                .with_kind(SpanKind::Internal)
                .with_start_time(start)
                .with_attributes(attrs),
            &Context::new(),
            end,
            failed,
        );
    }

    fn finish_span(&self, builder: SpanBuilder, parent: &Context, end: SystemTime, failed: bool) {
        let mut span = self.tracer.build_with_context(builder, parent);
        if failed {
            span.set_status(Status::error(""));
        }
        span.end_with_timestamp(end);
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
            } else if let Some(command) = event.downcast_ref::<CommandFinished>() {
                self.command(command);
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
            CommandFinished::NAME,
            JobAttempted::NAME,
            ApplicationTerminating::NAME,
        ] {
            registry.listen(event, recorder.clone(), Delivery::Sync)?;
        }
        Ok(())
    })
}
