//! 独立应用只使用公开 Provider 和 Handler API 验证异常装配。

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use figment::{Figment, providers::Serialized};
use openrust::config::{FrameworkConfig, HttpStatusMode, NoConfig};
use openrust::core::Result;
use openrust::foundation::ApplicationDefinition;
use openrust::foundation::BootContext;
use openrust::foundation::RegisterContext;
use openrust::foundation::ReportDecision;
use openrust::http::{
    HttpServiceProvider,
    axum::{
        body::Body,
        http::{Request, StatusCode},
        response::IntoResponse,
    },
};
use openrust::support::{BoxFuture, ServiceProvider};
use openrust::testing::TestApp;

#[derive(Debug)]
struct ConsumerFailure;
impl std::fmt::Display for ConsumerFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("consumer private detail")
    }
}
impl std::error::Error for ConsumerFailure {}

struct ReportCount(AtomicUsize);
struct ConsumerProvider;
impl ServiceProvider for ConsumerProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.instance(Arc::new(ReportCount(AtomicUsize::new(0))))
                .await?;
            Ok(())
        })
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let counter = app.resolve::<ReportCount>().await?;
            app.exceptions(move |handler| {
                handler
                    .report::<ConsumerFailure, _>(move |_| {
                        counter.0.fetch_add(1, Ordering::SeqCst);
                        ReportDecision::Stop
                    })
                    .render::<ConsumerFailure, _>(|_, request| {
                        if request.path() == "/custom" {
                            Some((StatusCode::CONFLICT, "custom response").into_response())
                        } else {
                            None
                        }
                    })
                    .respond(|mut response, _, _| {
                        response.headers_mut().insert(
                            "x-handler",
                            openrust::http::axum::http::HeaderValue::from_static("consumer"),
                        );
                        response
                    });
            })?;
            app.routes(|routes| {
                routes
                    .get("/custom", fail)
                    .get("/default", fail)
                    .get("/missing", || async {
                        Err::<(), _>(openrust::Error::NotFound("missing".into()))
                    })
            })?;
            Ok(())
        })
    }
}

async fn fail() -> Result<()> {
    Err(openrust::Error::internal(ConsumerFailure))
}

struct ConsumerApp;
impl ApplicationDefinition for ConsumerApp {
    type Config = NoConfig;

    fn default_providers() -> openrust::support::DefaultProviders {
        openrust::support::DefaultProviders::empty()
    }

    fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
        vec![Box::new(HttpServiceProvider), Box::new(ConsumerProvider)]
    }
}

fn source() -> Figment {
    let mut config = FrameworkConfig::default();
    config.api.http_status = HttpStatusMode::Semantic;
    Figment::from(Serialized::defaults(config))
}

#[tokio::test]
async fn typed_report_render_finalize_and_default_negotiation()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut first = TestApp::<ConsumerApp>::builder(source()).build().await?;
    let mut second = TestApp::<ConsumerApp>::builder(source()).build().await?;
    let custom = first
        .request(Request::get("/custom").body(Body::empty())?)
        .await?;
    custom.assert_status(StatusCode::CONFLICT)?;
    assert_eq!(custom.body(), b"custom response");
    assert_eq!(
        custom
            .headers()
            .get("x-handler")
            .and_then(|value| value.to_str().ok()),
        Some("consumer")
    );
    let json = first
        .request(
            Request::get("/default")
                .header("accept", "application/json")
                .body(Body::empty())?,
        )
        .await?;
    json.assert_status(StatusCode::INTERNAL_SERVER_ERROR)?;
    let value: serde_json::Value = serde_json::from_slice(json.body())?;
    // E01统一安全文案：未安装翻译Provider时也使用内置英语目录。
    assert_eq!(value["message"], "internal server error");
    assert!(!String::from_utf8_lossy(json.body()).contains("private detail"));
    let html = first
        .request(
            Request::get("/default")
                .header("accept", "text/html")
                .body(Body::empty())?,
        )
        .await?;
    html.assert_status(StatusCode::INTERNAL_SERVER_ERROR)?;
    assert!(
        html.headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/html"))
    );
    let html_body = String::from_utf8_lossy(html.body());
    assert!(html_body.contains("<p>internal server error</p>"));
    assert!(!html_body.contains("private detail"));
    let head = first
        .request(
            Request::head("/default")
                .header("accept", "text/html")
                .body(Body::empty())?,
        )
        .await?;
    head.assert_status(StatusCode::INTERNAL_SERVER_ERROR)?;
    assert!(head.body().is_empty());
    let first_count = first.app()?.state().resolve::<ReportCount>().await?;
    let second_count = second.app()?.state().resolve::<ReportCount>().await?;
    first
        .app()?
        .report_exception(&openrust::Error::internal(ConsumerFailure));
    assert_eq!(first_count.0.load(Ordering::SeqCst), 5);
    assert_eq!(second_count.0.load(Ordering::SeqCst), 0);
    second
        .request(Request::get("/custom").body(Body::empty())?)
        .await?
        .assert_status(StatusCode::CONFLICT)?;
    assert_eq!(second_count.0.load(Ordering::SeqCst), 1);
    first
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    second
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    Ok(())
}
