//! 经过真实HTTP middleware验证Source刷新与metadata/task-local的一致性。
use super::apply;
use crate::core::{Error, ExecutionContext, Result};
use crate::foundation::{Application, ProviderBuilder, RegisterContext};
use crate::support::{BoxFuture, ServiceProvider};
use crate::translation::{
    Locale, MessageArgs, RequestLocale, TranslationConfig, TranslationLoadSession,
    TranslationLoader, TranslationResource, Translator, parse_translation_lines,
    translation_session,
};
use axum::{Router, body::Body, extract::Extension, routing::get};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tower::ServiceExt;

struct Source {
    snapshot: RwLock<Arc<dyn TranslationLoadSession>>,
    fail: AtomicBool,
    pending: bool,
    captures: AtomicUsize,
    checks: AtomicUsize,
}
impl TranslationLoader for Source {
    fn begin_snapshot(&self) -> BoxFuture<'_, Result<Arc<dyn TranslationLoadSession>>> {
        Box::pin(async move {
            self.captures.fetch_add(1, Ordering::SeqCst);
            if self.fail.load(Ordering::SeqCst) {
                return Err(Error::custom("private_failure", "不可公开来源细节"));
            }
            self.snapshot
                .read()
                .map(|snapshot| Arc::clone(&snapshot))
                .map_err(|_| Error::custom("test_snapshot_lock", "测试快照锁失败"))
        })
    }
    fn needs_refresh(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move {
            self.checks.fetch_add(1, Ordering::SeqCst);
            if self.pending {
                std::future::pending::<()>().await;
            }
            Ok(true)
        })
    }
}
fn snapshot(fr: bool) -> Result<Arc<dyn TranslationLoadSession>> {
    let mut resources = BTreeMap::new();
    resources.insert(
        TranslationResource {
            locale: Locale::new("en")?,
            group: "*".into(),
            namespace: None,
        },
        parse_translation_lines(br#"{"label":"hello"}"#)?,
    );
    if fr {
        resources.insert(
            TranslationResource {
                locale: Locale::new("fr")?,
                group: "*".into(),
                namespace: None,
            },
            parse_translation_lines(br#"{"label":"bonjour"}"#)?,
        );
    }
    translation_session(resources)
}
struct Service(Translator);
impl ServiceProvider for Service {
    fn register<'a>(&'a self, context: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { context.instance(Arc::new(self.0.clone())).await })
    }
}
async fn through_http(initial_fr: bool, next_fr: bool, fail: bool, expected: &str) -> Result<()> {
    let source = Arc::new(Source {
        snapshot: RwLock::new(snapshot(initial_fr)?),
        fail: AtomicBool::new(false),
        pending: false,
        captures: AtomicUsize::new(0),
        checks: AtomicUsize::new(0),
    });
    let translator = Translator::from_loader(TranslationConfig::default(), source.clone()).await?;
    let mut builder = ProviderBuilder::new();
    builder.provider(Service(translator.clone()));
    let application =
        Application::from_provider_runtime(builder.start(figment::Figment::new()).await?)?;
    let translator_for_handler = translator.clone();
    let router = Router::new().route(
        "/",
        get(move |locale: RequestLocale| {
            let translator = translator_for_handler.clone();
            async move {
                let metadata = ExecutionContext::current()
                    .and_then(|context| context.metadata().locale().cloned())
                    .map(|locale| locale.to_string())
                    .unwrap_or_default();
                // 同步读取不得触发另一次刷新或SQL。
                format!(
                    "{}|{}|{}",
                    metadata,
                    locale.as_str(),
                    locale.translate(&translator, "label", &MessageArgs::new())
                )
            }
        }),
    );
    let router = apply(
        router,
        &crate::config::ServerConfig::default(),
        &crate::config::ApiConfig::default(),
        Arc::new(crate::foundation::ExceptionHandler::default()),
        false,
    )?
    .layer(Extension(application.state().clone()));
    *source
        .snapshot
        .write()
        .map_err(|_| Error::custom("test_snapshot_lock", "测试快照锁失败"))? = snapshot(next_fr)?;
    source.fail.store(fail, Ordering::SeqCst);
    let before = source.captures.load(Ordering::SeqCst);
    let outcome: Result<()> = async {
        let response = router
            .oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .header("accept-language", "fr, en;q=0.5")
                    .body(Body::empty())
                    .map_err(Error::internal)?,
            )
            .await
            .map_err(Error::internal)?;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 256)
            .await
            .map_err(Error::internal)?;
        assert_eq!(body.as_ref(), expected.as_bytes());
        assert_eq!(source.captures.load(Ordering::SeqCst), before + 1);
        assert_eq!(source.checks.load(Ordering::SeqCst), 1);
        assert_eq!(translator.refresh_status()?.last_error_code.is_some(), fail);
        Ok(())
    }
    .await;
    application
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    outcome
}
#[tokio::test]
async fn added_locale_is_negotiated_from_original_header_after_refresh() -> Result<()> {
    through_http(false, true, false, "fr|fr|bonjour").await
}
#[tokio::test]
async fn deleted_locale_updates_metadata_and_request_locale_together() -> Result<()> {
    through_http(true, false, false, "en|en|hello").await
}
#[tokio::test]
async fn failed_refresh_keeps_published_locale_in_metadata_and_request_locale() -> Result<()> {
    through_http(true, false, true, "fr|fr|bonjour").await
}

// 同一生产response_context负责admission；不能依赖其内层TimeoutLayer保护刷新。
async fn pending_source_request(
    admitted: bool,
    status_mode: crate::config::HttpStatusMode,
) -> Result<()> {
    let source = Arc::new(Source {
        snapshot: RwLock::new(snapshot(true)?),
        fail: AtomicBool::new(false),
        pending: true,
        captures: AtomicUsize::new(0),
        checks: AtomicUsize::new(0),
    });
    let translator = Translator::from_loader(TranslationConfig::default(), source.clone()).await?;
    let mut builder = ProviderBuilder::new();
    builder.provider(Service(translator.clone()));
    let application =
        Application::from_provider_runtime(builder.start(figment::Figment::new()).await?)?;
    let entered = Arc::new(AtomicBool::new(false));
    let handler_entered = entered.clone();
    let mut exceptions = crate::foundation::ExceptionHandler::default();
    exceptions.api_envelope_when(|_, _| true);
    let settings = super::ResponseSettings {
        api: crate::config::ApiConfig {
            http_status: status_mode,
            ..crate::config::ApiConfig::default()
        },
        exceptions: Arc::new(exceptions),
        debug: false,
        request_timeout: Duration::from_millis(20),
        request_slots: Arc::new(tokio::sync::Semaphore::new(usize::from(admitted))),
    };
    let router = Router::new()
        .route(
            "/",
            get(move || {
                let entered = handler_entered.clone();
                async move {
                    entered.store(true, Ordering::SeqCst);
                    "unexpected handler"
                }
            }),
        )
        .layer(axum::middleware::from_fn_with_state(
            settings,
            super::response_context,
        ))
        .layer(Extension(application.state().clone()));
    let outcome: Result<()> = async {
        let response = tokio::time::timeout(
            Duration::from_secs(1),
            router.oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .header("accept-language", "fr")
                    .header("accept", "application/json")
                    .body(Body::empty())
                    .map_err(Error::internal)?,
            ),
        )
        .await
        .map_err(Error::internal)?
        .map_err(Error::internal)?;
        assert_eq!(
            response.status(),
            if matches!(status_mode, crate::config::HttpStatusMode::AlwaysOk) {
                axum::http::StatusCode::OK
            } else if admitted {
                axum::http::StatusCode::REQUEST_TIMEOUT
            } else {
                axum::http::StatusCode::SERVICE_UNAVAILABLE
            }
        );
        if admitted {
            let bytes = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .map_err(Error::internal)?;
            let envelope: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(Error::internal)?;
            assert_eq!(envelope["code"], "request_timeout");
        }
        assert!(!entered.load(Ordering::SeqCst));
        assert_eq!(source.checks.load(Ordering::SeqCst), usize::from(admitted));
        assert_eq!(source.captures.load(Ordering::SeqCst), 1);
        assert_eq!(
            translator.translate(&Locale::new("fr")?, "label", &MessageArgs::new()),
            "bonjour"
        );
        // 普通同步翻译没有访问Source，已验证快照未因超时/拒绝丢失。
        assert_eq!(source.checks.load(Ordering::SeqCst), usize::from(admitted));
        Ok(())
    }
    .await;
    application
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    outcome
}
#[tokio::test]
async fn pending_source_does_not_delay_overload_rejection() -> Result<()> {
    pending_source_request(false, crate::config::HttpStatusMode::Semantic).await
}
#[tokio::test]
async fn pending_source_consumes_request_deadline_without_entering_handler() -> Result<()> {
    pending_source_request(true, crate::config::HttpStatusMode::Semantic).await
}
#[tokio::test]
async fn pending_source_deadline_renders_always_ok_timeout_envelope() -> Result<()> {
    pending_source_request(true, crate::config::HttpStatusMode::AlwaysOk).await
}

struct ReadinessService(Arc<crate::core::Readiness>);
impl ServiceProvider for ReadinessService {
    fn register<'a>(&'a self, context: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { context.instance(self.0.clone()).await })
    }
}
async fn check_cancelled_refresh_response(response: axum::response::Response) -> Result<()> {
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        response.headers()[axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://example.com"
    );
    assert_eq!(
        response
            .headers()
            .get("x-request-id")
            .and_then(|value| value.to_str().ok()),
        Some("source-cancel")
    );
    assert_eq!(
        response
            .headers()
            .get("x-source-exception")
            .and_then(|value| value.to_str().ok()),
        Some("applied")
    );
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .map_err(Error::internal)?;
    let body = serde_json::from_slice::<serde_json::Value>(&body).map_err(Error::internal)?;
    assert_eq!(body["code"], "server_draining");
    assert_eq!(body["request_id"], "source-cancel");
    Ok(())
}

#[tokio::test]
async fn cancelled_refresh_renders_inside_original_exception_and_request_context() -> Result<()> {
    let source = Arc::new(Source {
        snapshot: RwLock::new(snapshot(true)?),
        fail: AtomicBool::new(false),
        pending: true,
        captures: AtomicUsize::new(0),
        checks: AtomicUsize::new(0),
    });
    let translator = Translator::from_loader(TranslationConfig::default(), source.clone()).await?;
    let readiness = Arc::new(crate::core::Readiness::default());
    let mut builder = ProviderBuilder::new();
    builder
        .provider(Service(translator))
        .provider(ReadinessService(readiness.clone()));
    let application =
        Application::from_provider_runtime(builder.start(figment::Figment::new()).await?)?;
    let mut exceptions = crate::foundation::ExceptionHandler::default();
    exceptions
        .api_envelope_when(|_, _| true)
        .respond(|mut response, error, request| {
            assert_eq!(error.code(), "server_draining");
            assert_eq!(request.request_id(), "source-cancel");
            assert_eq!(request.path(), "/");
            assert_eq!(
                ExecutionContext::current()
                    .and_then(|context| context.metadata().locale().cloned())
                    .map(|locale| locale.to_string())
                    .as_deref(),
                Some("fr")
            );
            response.headers_mut().insert(
                "x-source-exception",
                axum::http::HeaderValue::from_static("applied"),
            );
            response
        });
    let mut server = crate::config::ServerConfig {
        request_timeout_secs: 5,
        ..crate::config::ServerConfig::default()
    };
    server.cors.enabled = true;
    server.cors.allowed_origins = vec!["https://example.com".into()];
    let api = crate::config::ApiConfig {
        http_status: crate::config::HttpStatusMode::AlwaysOk,
        ..crate::config::ApiConfig::default()
    };
    let router = apply(
        Router::new().route("/", get(|| async { "unexpected handler" })),
        &server,
        &api,
        Arc::new(exceptions),
        false,
    )?
    .layer(Extension(application.state().clone()));
    let outcome: Result<()> = async {
        let request = axum::http::Request::builder()
            .uri("/")
            .header("accept-language", "fr")
            .header("x-request-id", "source-cancel")
            .header("origin", "https://example.com")
            .header("accept", "application/json")
            .body(Body::empty())
            .map_err(Error::internal)?;
        let (response, ()) = tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(router.oneshot(request), async {
                while source.checks.load(Ordering::SeqCst) == 0 {
                    tokio::task::yield_now().await;
                }
                readiness.cancel_requests();
            })
        })
        .await
        .map_err(Error::internal)?;
        let response = response.map_err(Error::internal)?;
        check_cancelled_refresh_response(response).await?;
        assert_eq!(source.checks.load(Ordering::SeqCst), 1);
        assert_eq!(source.captures.load(Ordering::SeqCst), 1);
        Ok(())
    }
    .await;
    application
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    outcome
}
