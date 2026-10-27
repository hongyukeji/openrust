//! 协议适配器共享的 OpenTelemetry 上下文辅助方法。

use opentelemetry::propagation::TextMapPropagator as _;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;

/// 将已校验的 W3C `traceparent` 设为当前 tracing span 的远端 parent。
pub(crate) fn set_remote_parent(span: &tracing::Span, traceparent: Option<&str>) {
    let Some(traceparent) = traceparent else {
        return;
    };
    let parent = opentelemetry_sdk::propagation::TraceContextPropagator::new()
        .extract(&TraceparentExtractor(traceparent));
    let _ = span.set_parent(parent);
}

struct TraceparentExtractor<'a>(&'a str);

impl opentelemetry::propagation::Extractor for TraceparentExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        key.eq_ignore_ascii_case("traceparent").then_some(self.0)
    }

    fn keys(&self) -> Vec<&str> {
        vec!["traceparent"]
    }
}

#[cfg(test)]
mod tests {
    use opentelemetry::{
        propagation::{Extractor as _, TextMapPropagator as _},
        trace::TraceContextExt as _,
    };

    use super::TraceparentExtractor;

    #[test]
    fn extractor_reads_only_the_validated_traceparent_field() {
        let carrier =
            TraceparentExtractor("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01");
        let context =
            opentelemetry_sdk::propagation::TraceContextPropagator::new().extract(&carrier);
        let span_context = context.span().span_context().clone();
        assert!(span_context.is_remote());
        assert_eq!(
            span_context.trace_id().to_string(),
            "4bf92f3577b34da6a3ce929d0e0e4736"
        );
        assert_eq!(carrier.get("tracestate"), None);
    }
}
