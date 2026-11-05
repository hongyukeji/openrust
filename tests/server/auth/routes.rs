//! 验证仅使用 facade 的应用路径可以注册并使用认证能力。
#![cfg(feature = "auth")]

use std::collections::BTreeMap;
use std::error::Error as StdError;

use openrust::ServiceExt;
use openrust::auth::{AuthServiceProvider, JwtServiceProvider};
use openrust::config::FrameworkConfig;
use openrust::http::axum::body::{Body, to_bytes};
use openrust::http::axum::http::{Request, StatusCode};
use openrust::http::{HttpServiceProvider, build_router};
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

    fn default_providers() -> openrust::core::DefaultProviders {
        openrust::core::DefaultProviders::empty()
    }

    fn providers(_config: &Self::Config) -> Vec<Box<dyn openrust::core::ServiceProvider>> {
        vec![
            Box::new(HttpServiceProvider),
            Box::new(AuthServiceProvider::with_authenticator(TestAuthenticator)),
            Box::new(TestInstance(std::sync::Arc::new(ProfileService))),
            Box::new(TestRoutes::new(|routes| {
                routes
                    .get("/api/profile", profile)
                    .layer(AuthLayer::required(AuthMode::PersonalAccessToken))
            })),
        ]
    }
}

impl Application for JwtApp {
    type Config = NoConfig;

    fn default_providers() -> openrust::core::DefaultProviders {
        openrust::core::DefaultProviders::empty()
    }

    fn providers(_config: &Self::Config) -> Vec<Box<dyn openrust::core::ServiceProvider>> {
        let key = [0x5a; 32];
        vec![
            Box::new(HttpServiceProvider),
            Box::new(JwtSettings::new(
                "https://openrust.example",
                "openrust-server-tests",
                "test-key",
                &BTreeMap::from([(
                    "test-key".to_owned(),
                    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, key),
                )]),
            )),
            Box::new(JwtServiceProvider),
            Box::new(AuthServiceProvider::with_authenticator(TestAuthenticator)),
            Box::new(TestInstance(std::sync::Arc::new(ProfileService))),
            Box::new(TestRoutes::new(|routes| {
                routes
                    .get("/api/profile", profile)
                    .layer(AuthLayer::required(AuthMode::Jwt))
            })),
        ]
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
    let _passwords = app
        .state()
        .resolve::<openrust::hashing::HashManager>()
        .await?;
    let tokens = app.state().resolve::<JwtTokens>().await?;
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

async fn test_application<A: Application<Config = NoConfig>>()
-> Result<openrust::App, openrust::Error> {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder.app_config::<NoConfig>();
    for provider in A::providers(&NoConfig {}) {
        builder.boxed_provider(provider);
    }
    builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)
}

struct TestInstance<T: Send + Sync + 'static>(std::sync::Arc<T>);
impl<T: Send + Sync + 'static> openrust::core::ServiceProvider for TestInstance<T> {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::core::RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.instance(std::sync::Arc::clone(&self.0)).await?;
            Ok(())
        })
    }
}

struct TestRoutes<F>(std::sync::Arc<F>);
impl<F: Fn(openrust::http::Routes) -> openrust::http::Routes> TestRoutes<F> {
    fn new(routes: F) -> Self {
        Self(std::sync::Arc::new(routes))
    }
}
impl<F> openrust::core::ServiceProvider for TestRoutes<F>
where
    F: Fn(openrust::http::Routes) -> openrust::http::Routes + Send + Sync + 'static,
{
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        let routes = std::sync::Arc::clone(&self.0);
        Box::pin(async move {
            app.routes(move |value| routes(value))?;
            Ok(())
        })
    }
}

struct JwtSettings(serde_json::Value);
impl JwtSettings {
    fn new(
        issuer: &str,
        audience: &str,
        active_key_id: &str,
        keys: &std::collections::BTreeMap<String, String>,
    ) -> Self {
        Self(serde_json::json!({"issuer":issuer,"audience":audience,
            "active_key_id":active_key_id,"keys":keys}))
    }
}
impl openrust::core::ServiceProvider for JwtSettings {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::core::RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move { app.merge_defaults("auth.jwt", self.0.clone()) })
    }
}
