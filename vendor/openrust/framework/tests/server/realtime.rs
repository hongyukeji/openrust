//! 验证实时 capability 通过真实 HTTP 路由、AuthLayer 与应用插件装配。

use std::{
    error::Error as StdError,
    time::{Duration, SystemTime},
};

use openrust::{
    ApplicationDefinition, BoxFuture, Error, NoConfig, Result, ServiceExt,
    auth::{
        AuthCredential, AuthLayer, AuthMode, AuthPrincipal, AuthServiceProvider, Authenticator,
        CurrentUser,
    },
    config::{FrameworkConfig, HttpStatusMode, RealtimeConfig},
    http::{
        HttpServiceProvider,
        axum::{
            body::Body,
            http::{Request, StatusCode, header},
            response::Response,
        },
    },
    prelude::{Inject, Realtime},
    realtime::RealtimeServiceProvider,
};

type TestResult = Result<(), Box<dyn StdError>>;

struct RealtimeHttpApp;
struct AuthenticatorForTest;

impl Authenticator for AuthenticatorForTest {
    fn authenticate<'a>(
        &'a self,
        credential: AuthCredential<'a>,
    ) -> BoxFuture<'a, Result<AuthPrincipal>> {
        Box::pin(async move {
            if credential.value() == Some("realtime-test-token") {
                Ok(AuthPrincipal::new(
                    73,
                    Some(vec!["notifications:read".into()]),
                ))
            } else {
                Err(Error::Unauthorized)
            }
        })
    }
}

impl ApplicationDefinition for RealtimeHttpApp {
    type Config = NoConfig;

    fn default_providers() -> openrust::support::DefaultProviders {
        openrust::support::DefaultProviders::empty()
    }

    fn providers(_config: &Self::Config) -> Vec<Box<dyn openrust::support::ServiceProvider>> {
        vec![
            Box::new(HttpServiceProvider),
            Box::new(AuthServiceProvider::with_authenticator(
                AuthenticatorForTest,
            )),
            Box::new(RealtimeServiceProvider::with_config(
                RealtimeConfig::new().with_allowed_origins(["https://app.example".to_owned()]),
            )),
            Box::new(TestRoutes::new(|routes| {
                routes
                    .get("/api/realtime/sse", sse)
                    .layer(AuthLayer::required(AuthMode::PersonalAccessToken))
            })),
        ]
    }
}

async fn sse(
    headers: openrust::http::axum::http::HeaderMap,
    user: CurrentUser,
    Inject(realtime): Inject<Realtime>,
) -> Result<Response> {
    if !user.has_ability("notifications:read") {
        return Err(Error::Forbidden("无权接收通知".into()));
    }
    realtime.validate_origin(&headers)?;
    let session = realtime
        .connect(
            &user,
            "notifications",
            SystemTime::now() + Duration::from_secs(60),
        )
        .await?;
    Ok(session.sse())
}

async fn assembled_app() -> Result<openrust::Application> {
    let mut config = FrameworkConfig::default();
    config.api.http_status = HttpStatusMode::Semantic;
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config))
        .merge(figment::Figment::new());
    let mut builder = openrust::foundation::ProviderBuilder::new();
    builder.app_config::<NoConfig>();
    for provider in RealtimeHttpApp::providers(&NoConfig {}) {
        builder.boxed_provider(provider);
    }
    builder
        .start(sources)
        .await
        .and_then(openrust::Application::from_provider_runtime)
}

#[openrust::tokio::test]
async fn http_auth_origin_and_sse_headers_are_enforced() -> TestResult {
    let app = assembled_app().await?;
    let router = openrust::foundation::http::build_router(&app)?;

    let unauthorized = router
        .clone()
        .oneshot(Request::get("/api/realtime/sse").body(Body::empty())?)
        .await?;
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let rejected_origin = router
        .clone()
        .oneshot(
            Request::get("/api/realtime/sse")
                .header(header::AUTHORIZATION, "Bearer realtime-test-token")
                .header(header::ORIGIN, "https://evil.example")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(rejected_origin.status(), StatusCode::FORBIDDEN);

    let accepted = router
        .oneshot(
            Request::get("/api/realtime/sse")
                .header(header::AUTHORIZATION, "Bearer realtime-test-token")
                .header(header::ORIGIN, "https://app.example")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(accepted.status(), StatusCode::OK);
    assert_eq!(
        accepted.headers()[header::CONTENT_TYPE],
        "text/event-stream"
    );
    assert_eq!(
        accepted.headers()[header::CACHE_CONTROL],
        "no-cache, no-transform"
    );
    assert_eq!(accepted.headers()["x-accel-buffering"], "no");
    drop(accepted);
    Ok(())
}

struct TestRoutes<F>(std::sync::Arc<F>);
impl<F: Fn(openrust::routing::Router) -> openrust::routing::Router> TestRoutes<F> {
    fn new(routes: F) -> Self {
        Self(std::sync::Arc::new(routes))
    }
}
impl<F> openrust::support::ServiceProvider for TestRoutes<F>
where
    F: Fn(openrust::routing::Router) -> openrust::routing::Router + Send + Sync + 'static,
{
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::foundation::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        let routes = std::sync::Arc::clone(&self.0);
        Box::pin(async move {
            app.routes(move |value| routes(value))?;
            Ok(())
        })
    }
}
