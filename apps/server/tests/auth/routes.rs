//! 验证仅使用 facade 的应用路径可以注册并使用认证能力。
#![cfg(feature = "auth")]

use std::collections::BTreeMap;
use std::error::Error as StdError;

use openrust::ServiceExt;
use openrust::auth::Passwords;
use openrust::config::FrameworkConfig;
use openrust::http::axum::body::{Body, to_bytes};
use openrust::http::axum::http::{Request, StatusCode};
use openrust::http::{AppBuilderHttpExt, HttpPlugin, build_router};
use openrust::prelude::*;

type TestResult = Result<(), Box<dyn StdError>>;

struct TestAuthenticator;

impl Authenticator for TestAuthenticator {
    fn authenticate<'a>(
        &'a self,
        credential: AuthCredential<'a>,
    ) -> BoxFuture<'a, Result<AuthPrincipal>> {
        Box::pin(async move {
            if credential.source() == AuthSource::PersonalAccessToken
                && credential.value() == Some("facade-test-token")
            {
                Ok(AuthPrincipal::new(73, Some(vec!["profile:read".into()])))
            } else {
                Err(Error::Unauthorized)
            }
        })
    }
}

struct App;
struct JwtApp;
struct ProfileService;

impl Application for App {
    type Config = NoConfig;

    fn build(app: &mut AppBuilder) {
        app.plugin(HttpPlugin)
            .plugin(AuthPlugin::with_authenticator(TestAuthenticator))
            .insert(ProfileService)
            .routes(|routes| {
                routes
                    .get("/api/profile", profile)
                    .layer(AuthLayer::required(AuthMode::PersonalAccessToken))
            });
    }
}

impl Application for JwtApp {
    type Config = NoConfig;

    fn build(app: &mut AppBuilder) {
        let key = [0x5a; 32];
        app.plugin(HttpPlugin)
            .plugin(JwtPlugin::with_config(JwtConfig::new(
                "https://openrust.example",
                "openrust-server-tests",
                "test-key",
                BTreeMap::from([(
                    "test-key".to_owned(),
                    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, key),
                )]),
            )))
            .plugin(AuthPlugin::with_authenticator(TestAuthenticator))
            .insert(ProfileService)
            .routes(|routes| {
                routes
                    .get("/api/profile", profile)
                    .layer(AuthLayer::required(AuthMode::Jwt))
            });
    }
}

async fn profile(
    user: CurrentUser,
    Inject(_profile): Inject<ProfileService>,
) -> Result<Json<String>> {
    Ok(Json(format!(
        "{}:{}",
        user.id(),
        user.has_ability("profile:read")
    )))
}

#[openrust::tokio::test]
async fn facade_application_authentication_coexists_with_dependency_injection() -> TestResult {
    let app = test_application::<App>().await?;
    let response = build_router(&app)?
        .oneshot(
            Request::get("/api/profile")
                .header("authorization", "Bearer facade-test-token")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    let json: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(json["data"], "73:true");
    Ok(())
}

#[openrust::tokio::test]
async fn facade_application_authenticates_jwt_without_session_plugin() -> TestResult {
    let app = test_application::<JwtApp>().await?;
    let _passwords = app.state().try_get::<Passwords>()?;
    let tokens = app.state().try_get::<JwtTokens>()?;
    let token = tokens.issue_access_token(73)?;
    let response = build_router(&app)?
        .oneshot(
            Request::get("/api/profile")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    let json: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(json["data"], "73:false");
    Ok(())
}

async fn test_application<A: Application>() -> Result<openrust::App, openrust::Error> {
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment::Figment::new());
    builder.insert(NoConfig {});
    A::build(&mut builder);
    openrust::lifecycle::finalize(builder).await
}
