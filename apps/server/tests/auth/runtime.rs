//! Runtime contract tests for route authentication and identity extraction.
#![cfg(feature = "auth")]

use std::error::Error as StdError;

use base64::Engine as _;
use openrust::auth::{
    AuthCredential, AuthLayer, AuthMode, AuthPlugin, AuthPrincipal, AuthSource, Authenticator,
    CurrentUser, JwtConfig, JwtPlugin, JwtTokens, OptionalUser, Session, SessionConfig,
    SessionPlugin,
};
use openrust::http::axum::body::{Body, to_bytes};
use openrust::http::axum::http::{Request, StatusCode};
use openrust::http::{AppBuilderHttpExt, HttpPlugin, Inject, Json, Routes, build_router};
use openrust::{
    AppBuilder, BoxFuture, Error, Plugin, Result, config::FrameworkConfig, lifecycle::finalize,
};
use tower::ServiceExt;
use tower_sessions::MemoryStore;

type TestResult = Result<(), Box<dyn StdError>>;

struct FakeAuthenticator;

impl Authenticator for FakeAuthenticator {
    fn authenticate<'a>(
        &'a self,
        credential: AuthCredential<'a>,
    ) -> BoxFuture<'a, Result<AuthPrincipal>> {
        Box::pin(async move {
            match credential.source() {
                AuthSource::Session => {
                    let Some(session) = credential.session() else {
                        return Err(Error::Unauthorized);
                    };
                    let user_id = session
                        .get::<i64>("test.user_id")
                        .await
                        .map_err(Error::internal)?;
                    let csrf_hash = session
                        .get::<String>("test.csrf_hash")
                        .await
                        .map_err(Error::internal)?;
                    if user_id.is_none() {
                        return Err(Error::Unauthorized);
                    }
                    if credential.method() == "POST"
                        && (csrf_hash.as_deref() != Some("csrf-secret")
                            || credential.csrf_token() != Some("csrf-secret"))
                    {
                        return Err(Error::Forbidden("invalid CSRF token".into()));
                    }
                    Ok(AuthPrincipal::new(user_id.unwrap_or_default(), None))
                }
                AuthSource::Jwt => Err(Error::Unauthorized),
                AuthSource::PersonalAccessToken => {
                    if credential.value() == Some("pat-secret") {
                        Ok(AuthPrincipal::new(42, Some(vec!["posts:read".into()])))
                    } else {
                        Err(Error::Unauthorized)
                    }
                }
            }
        })
    }
}

struct Marker(&'static str);

async fn current_user(user: CurrentUser, Inject(marker): Inject<Marker>) -> Result<Json<String>> {
    Ok(Json(format!(
        "{}:{}:{}:{}",
        user.id(),
        marker.0,
        user.source() == AuthSource::PersonalAccessToken,
        user.has_ability("posts:read")
    )))
}

async fn optional_user(
    OptionalUser(user): OptionalUser,
    Inject(marker): Inject<Marker>,
) -> Result<Json<Option<String>>> {
    Ok(Json(user.map(|user| format!("{}:{}", user.id(), marker.0))))
}

async fn jwt_user(user: CurrentUser) -> Result<Json<String>> {
    Ok(Json(format!("{}:{:?}", user.id(), user.source())))
}

#[tokio::test]
async fn jwt_authentication_uses_the_configured_key_ring_without_session_middleware() -> TestResult
{
    let key = base64::engine::general_purpose::STANDARD.encode([0x53_u8; 32]);
    let jwt = JwtConfig::new(
        "https://issuer.example",
        "runtime-tests",
        "active",
        [("active".to_owned(), key)].into_iter().collect(),
    );
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment::Figment::new());
    builder
        .plugin(HttpPlugin)
        .plugin(JwtPlugin::with_config(jwt))
        .plugin(AuthPlugin::with_authenticator(FakeAuthenticator))
        .routes(|routes| {
            routes
                .get("/jwt", jwt_user)
                .layer(AuthLayer::required(AuthMode::Jwt))
        });
    let app = finalize(builder).await?;
    let tokens = app.state().try_get::<JwtTokens>()?;
    let token = tokens.issue_access_token(73)?;
    let response = build_router(&app)?
        .oneshot(
            Request::get("/jwt")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let json = payload(response).await?;
    assert_eq!(json["data"], "73:Jwt");
    Ok(())
}

async fn create_test_session(session: Session) -> Result<openrust::http::Json<&'static str>> {
    session
        .insert("test.user_id", 42_i64)
        .await
        .map_err(Error::internal)?;
    session
        .insert("test.csrf_hash", "csrf-secret")
        .await
        .map_err(Error::internal)?;
    Ok(openrust::http::Json("created"))
}

fn current_routes(routes: Routes) -> Routes {
    routes
        .get("/current", current_user)
        .post("/current", current_user)
        .layer(AuthLayer::required(AuthMode::SessionOrToken))
        .get("/session", create_test_session)
}

fn optional_routes(routes: Routes) -> Routes {
    routes
        .get("/optional", optional_user)
        .layer(AuthLayer::optional(AuthMode::SessionOrToken))
        .get("/session", create_test_session)
}

async fn build_app(routes: fn(Routes) -> Routes, auth: AuthPlugin) -> Result<openrust::App> {
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment::Figment::new());
    builder
        .plugin(HttpPlugin)
        .plugin(auth)
        .plugin(
            SessionPlugin::with_store(MemoryStore::default()).with_cookie_config(SessionConfig {
                secure: false,
                ..SessionConfig::default()
            }),
        )
        .insert(Marker("injected"))
        .routes(routes);
    finalize(builder).await
}

async fn payload(
    response: openrust::http::axum::response::Response,
) -> Result<serde_json::Value, Box<dyn StdError>> {
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[tokio::test]
async fn current_user_and_injected_service_share_the_route_request() -> TestResult {
    let app = build_app(
        current_routes,
        AuthPlugin::with_authenticator(FakeAuthenticator),
    )
    .await?;
    let router = build_router(&app)?;
    let response = router
        .oneshot(
            Request::get("/current")
                .header("authorization", "Bearer pat-secret")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let json = payload(response).await?;
    assert_eq!(json["data"], "42:injected:true:true");
    Ok(())
}

#[tokio::test]
async fn cookie_authentication_uses_the_shared_session_and_preserves_csrf_checks() -> TestResult {
    let app = build_app(
        current_routes,
        AuthPlugin::with_authenticator(FakeAuthenticator),
    )
    .await?;
    let router = build_router(&app)?;
    let created = router
        .clone()
        .oneshot(Request::get("/session").body(Body::empty())?)
        .await?;
    let set_cookie = created
        .headers()
        .get("set-cookie")
        .ok_or("Session middleware did not set a cookie")?
        .to_str()?;
    let cookie = set_cookie
        .split(';')
        .next()
        .ok_or("Session cookie was empty")?;

    let authenticated = router
        .clone()
        .oneshot(
            Request::get("/current")
                .header("cookie", cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(authenticated.status(), StatusCode::OK);
    let authenticated_payload = payload(authenticated).await?;
    assert_eq!(
        authenticated_payload["data"], "42:injected:false:false",
        "{authenticated_payload}"
    );

    let denied = router
        .clone()
        .oneshot(
            Request::post("/current")
                .header("cookie", cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(payload(denied).await?["code"], "forbidden");

    let accepted = router
        .oneshot(
            Request::post("/current")
                .header("cookie", cookie)
                .header("x-csrf-token", "csrf-secret")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(accepted.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn optional_identity_is_anonymous_only_when_selected_credentials_are_absent() -> TestResult {
    let app = build_app(
        optional_routes,
        AuthPlugin::with_authenticator(FakeAuthenticator),
    )
    .await?;
    let router = build_router(&app)?;
    let anonymous = router
        .clone()
        .oneshot(Request::get("/optional").body(Body::empty())?)
        .await?;
    assert_eq!(payload(anonymous).await?["data"], serde_json::Value::Null);

    let malformed = router
        .oneshot(
            Request::get("/optional")
                .header("authorization", "Bearer  ")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(payload(malformed).await?["code"], "unauthorized");
    Ok(())
}

#[tokio::test]
async fn default_database_authenticator_requires_database_at_startup() -> TestResult {
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment::Figment::new());
    builder.plugin(AuthPlugin::new());
    let Err(error) = finalize(builder).await else {
        return Err("default auth plugin started without its database".into());
    };
    assert!(error.to_string().contains("openrust::db::Db"), "{error}");
    Ok(())
}

struct DuplicateAuthService;

impl Plugin for DuplicateAuthService {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        app.plugin(AuthPlugin::with_authenticator(FakeAuthenticator));
        Ok(())
    }
}

#[tokio::test]
async fn duplicate_auth_plugin_is_rejected() -> TestResult {
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment::Figment::new());
    builder
        .plugin(AuthPlugin::with_authenticator(FakeAuthenticator))
        .plugin(DuplicateAuthService);
    let Err(error) = finalize(builder).await else {
        return Err("duplicate auth plugin unexpectedly started".into());
    };
    assert!(
        error
            .to_string()
            .contains("register only one authentication plugin")
    );
    Ok(())
}
