use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use opentelemetry::{Context, InstrumentationScope};
use opentelemetry_sdk::{
    Resource,
    error::OTelSdkResult,
    logs::{BatchLogProcessor, LogBatch, LogExporter, LogProcessor, SdkLogRecord},
    trace::{BatchSpanProcessor, Span, SpanData, SpanExporter, SpanProcessor},
};

pub(crate) const CAPACITY: usize = 2048;

// 与 SDK 队列使用同一容量。保留待导出项的名额，SDK 取出批次后释放。
// 在 SDK 入队前记录拒绝，指标按应用隔离，不依赖全局 MeterProvider。
#[derive(Debug, Default)]
pub(crate) struct Admission {
    pending: AtomicUsize,
    dropped: AtomicU64,
}

impl Admission {
    fn accept(&self) -> bool {
        if self
            .pending
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |pending| {
                (pending < CAPACITY).then_some(pending + 1)
            })
            .is_ok()
        {
            return true;
        }
        self.dropped.fetch_add(1, Ordering::Relaxed);
        false
    }

    fn start_export(&self, count: usize) {
        self.pending.fetch_sub(count, Ordering::Relaxed);
    }

    pub(crate) fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub(crate) fn wrap<T>(self: &Arc<Self>, inner: T) -> Bounded<T> {
        Bounded {
            inner,
            admission: self.clone(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Bounded<T> {
    inner: T,
    admission: Arc<Admission>,
}

impl SpanProcessor for Bounded<BatchSpanProcessor> {
    fn on_start(&self, span: &mut Span, context: &Context) {
        self.inner.on_start(span, context);
    }

    fn on_end(&self, span: SpanData) {
        if self.admission.accept() {
            self.inner.on_end(span);
        }
    }

    fn force_flush(&self) -> OTelSdkResult {
        self.inner.force_flush()
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        self.inner.shutdown_with_timeout(timeout)
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.inner.set_resource(resource);
    }
}

impl SpanExporter for Bounded<opentelemetry_otlp::SpanExporter> {
    async fn export(&self, batch: Vec<SpanData>) -> OTelSdkResult {
        self.admission.start_export(batch.len());
        self.inner.export(batch).await
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        self.inner.shutdown_with_timeout(timeout)
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.inner.set_resource(resource);
    }
}

impl LogProcessor for Bounded<BatchLogProcessor> {
    fn emit(&self, record: &mut SdkLogRecord, scope: &InstrumentationScope) {
        if self.admission.accept() {
            self.inner.emit(record, scope);
        }
    }

    fn force_flush(&self) -> OTelSdkResult {
        self.inner.force_flush()
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        self.inner.shutdown_with_timeout(timeout)
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.inner.set_resource(resource);
    }
}

impl LogExporter for Bounded<opentelemetry_otlp::LogExporter> {
    async fn export(&self, batch: LogBatch<'_>) -> OTelSdkResult {
        self.admission.start_export(batch.iter().count());
        self.inner.export(batch).await
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        self.inner.shutdown_with_timeout(timeout)
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.inner.set_resource(resource);
    }
}
