//! 业务观测输出使用局部 subscriber/recorder，不改进程全局状态或外发供应商。

use super::services::telemetry::{ArticleOperation, observe};
use openrust::{Error, Result};
use std::{
    io::Write,
    sync::{Arc, Mutex},
};
use tracing::instrument::WithSubscriber as _;
use tracing_subscriber::fmt::format::FmtSpan;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("capture lock poisoned"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Capture {
    fn text(&self) -> TestResultString {
        Ok(String::from_utf8(
            self.0.lock().map_err(|_| "capture lock poisoned")?.clone(),
        )?)
    }
}
type TestResultString = std::result::Result<String, Box<dyn std::error::Error>>;
fn subscriber(capture: Capture) -> impl tracing::Subscriber + Send + Sync {
    tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .with_writer(move || capture.clone())
        .finish()
}

#[tokio::test]
async fn business_spans_classify_results_without_recording_input_or_error_values() -> TestResult {
    let capture = Capture::default();
    async {
        let value = observe(ArticleOperation::Create, async {
            Ok("BODY_EMAIL_TOKEN_SENTINEL")
        })
        .await?;
        assert_eq!(value, "BODY_EMAIL_TOKEN_SENTINEL");
        let failure = observe::<()>(ArticleOperation::Update, async {
            Err(Error::custom_with_status(
                openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "UNBOUNDED_OWNER_123_SENTINEL",
                "PRIVATE_ERROR_SENTINEL",
            ))
        })
        .await;
        assert!(failure.is_err());
        for (error, expected) in [
            (
                Error::BadRequest("PRIVATE_VALIDATION_SENTINEL".into()),
                "invalid",
            ),
            (Error::Unauthorized, "denied"),
            (
                Error::NotFound("PRIVATE_ARTICLE_ID_SENTINEL".into()),
                "not_found",
            ),
            (
                Error::Conflict("PRIVATE_CONFLICT_SENTINEL".into()),
                "conflict",
            ),
            (Error::RateLimited, "throttled"),
        ] {
            let error = observe::<()>(ArticleOperation::Delete, async { Err(error) }).await;
            assert!(error.is_err(), "{expected}");
        }
        Ok::<_, Error>(())
    }
    .with_subscriber(subscriber(capture.clone()))
    .await?;
    let text = capture.text()?;
    for label in [
        "create",
        "update",
        "delete",
        "success",
        "failed",
        "invalid",
        "denied",
        "not_found",
        "conflict",
        "throttled",
    ] {
        assert!(text.contains(label));
    }
    assert!(text.contains("example.article"));
    assert!(text.contains("duration_seconds"));
    assert!(!text.contains("SENTINEL"));
    Ok(())
}

#[tokio::test]
async fn cancelling_an_observed_future_records_cancelled_once_without_success() -> TestResult {
    let capture = Capture::default();
    async {
        let started = tokio::sync::Notify::new();
        let action = observe::<()>(ArticleOperation::Create, async {
            started.notify_one();
            std::future::pending::<Result<()>>().await
        });
        tokio::pin!(action);
        tokio::select! {
            _ = &mut action => return Err(Error::custom("test_action_finished","pending action unexpectedly completed")),
            () = started.notified() => {},
        }
        Ok::<_,Error>(())
    }.with_subscriber(subscriber(capture.clone())).await?;
    let text = capture.text()?;
    assert_eq!(text.matches("article operation finished").count(), 1);
    assert!(text.contains("cancelled"));
    assert!(!text.contains("success"));
    Ok(())
}

#[cfg(feature = "metrics")]
#[tokio::test(flavor = "current_thread")]
async fn business_metrics_use_only_bounded_labels_and_local_prometheus_recorder() -> TestResult {
    let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
    let handle = recorder.handle();
    let _guard = metrics::set_default_local_recorder(&recorder);
    observe(ArticleOperation::Create, async {
        Ok("PRIVATE_BODY_SENTINEL")
    })
    .await?;
    let _ = observe::<()>(ArticleOperation::Update, async {
        Err(Error::custom_with_status(
            openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "OWNER_ID_SENTINEL",
            "PRIVATE_TOKEN_SENTINEL",
        ))
    })
    .await;
    handle.run_upkeep();
    let output = handle.render();
    assert!(output.contains("example_article_operations_total"));
    assert!(output.contains("example_article_operation_duration_seconds"));
    assert!(output.contains("operation=\"create\""));
    assert!(output.contains("outcome=\"success\""));
    assert!(output.contains("outcome=\"failed\""));
    for forbidden in ["SENTINEL", "owner_id", "article_id", "email", "token"] {
        assert!(!output.contains(forbidden));
    }
    for line in output.lines().filter(|line| !line.starts_with('#')) {
        if let Some((_, labels)) = line.split_once('{') {
            let labels = labels.split('}').next().ok_or("metric labels invalid")?;
            for label in labels.split(',') {
                let key = label.split('=').next().ok_or("metric label missing")?;
                assert!(matches!(key, "operation" | "outcome" | "quantile" | "le"));
            }
        }
    }
    Ok(())
}
