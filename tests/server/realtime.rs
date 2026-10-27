//! 验证实时 capability 通过真实 HTTP 路由、AuthLayer 与应用插件装配。

use std::{
    error::Error as StdError,
    time::{Duration, SystemTime},
};

use openrust::{
    AppBuilder, Application, BoxFuture, Error, NoConfig, Result, ServiceExt,
    auth::{
        AuthCredential, AuthLayer, AuthMode, AuthPlugin, AuthPrincipal, Authenticator, CurrentUser,
    },
    config::{FrameworkConfig, HttpStatusMode, RealtimeConfig},
    core::lifecycle::finalize,
    http::{
        AppBuilderHttpExt, HttpPlugin,
        axum::{
            body::Body,
            http::{Request, StatusCode, header},
            response::Response,
        },
    },
    prelude::{Inject, Realtime, RealtimePlugin},
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

impl Application for RealtimeHttpApp {
    type Config = NoConfig;

    fn build(app: &mut AppBuilder) {
        app.plugin(HttpPlugin)
            .plugin(AuthPlugin::with_authenticator(AuthenticatorForTest))
            .plugin(RealtimePlugin::with_config(
                RealtimeConfig::new().with_allowed_origins(["https://app.example".to_owned()]),
            ))
            .routes(|routes| {
                routes
                    .get("/api/realtime/sse", sse)
                    .layer(AuthLayer::required(AuthMode::PersonalAccessToken))
            });
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

async fn assembled_app() -> Result<openrust::App> {
    let mut config = FrameworkConfig::default();
    config.api.http_status = HttpStatusMode::Semantic;
    let mut builder = AppBuilder::new(config, figment::Figment::new());
    builder.insert(NoConfig {});
    RealtimeHttpApp::build(&mut builder);
    finalize(builder).await
}

#[openrust::tokio::test]
async fn http_auth_origin_and_sse_headers_are_enforced() -> TestResult {
    let app = assembled_app().await?;
    let router = openrust::http::build_router(&app)?;

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
