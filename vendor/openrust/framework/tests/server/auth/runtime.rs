//! 验证路由认证与身份提取的运行时契约。
#![cfg(feature = "auth")]

use std::error::Error as StdError;

use base64::Engine as _;
#[cfg(feature = "session-redis")]
use openrust::auth::RedisSessionStore;
use openrust::auth::{
    AuthCredential, AuthLayer, AuthMode, AuthPrincipal, AuthServiceProvider, AuthSource,
    Authenticator, CurrentUser, JwtServiceProvider, JwtTokens, OptionalUser, Session,
    SessionConfig, SessionServiceProvider,
};
use openrust::http::axum::body::{Body, to_bytes};
use openrust::http::axum::http::{Request, StatusCode};
use openrust::http::{HttpServiceProvider, Inject, Json, Routes, build_router};
use openrust::session::MemorySessionStore as MemoryStore;
#[cfg(feature = "session-redis")]
use openrust::session::SessionStore as _;
#[cfg(feature = "session-redis")]
use openrust::session::{SessionId as Id, SessionRecord as Record};
use openrust::{BoxFuture, Error, Result, config::FrameworkConfig};
use tower::ServiceExt;

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
                        .get_typed::<i64>("test.user_id")
                        .map_err(Error::internal)?;
                    let csrf_hash = session
                        .get_typed::<String>("test.csrf_hash")
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
                AuthSource::Jwt | AuthSource::Guard => Err(Error::Unauthorized),
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
    let jwt = JwtSettings::new(
        "https://issuer.example",
        "runtime-tests",
        "active",
        &[("active".to_owned(), key)].into_iter().collect(),
    );
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(jwt)
        .provider(JwtServiceProvider)
        .provider(AuthServiceProvider::with_authenticator(FakeAuthenticator))
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/jwt", jwt_user)
                .layer(AuthLayer::required(AuthMode::Jwt))
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let tokens = app.state().resolve::<JwtTokens>().await?;
    let token = tokens.issue_access_token(73)?;
    let response = build_router(&app)?
        .oneshot(
            Request::get("/jwt")
                .header("host", "localhost")
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
        .put("test.user_id", 42_i64.into())
        .map_err(Error::internal)?;
    session
        .put("test.csrf_hash", "csrf-secret".into())
        .map_err(Error::internal)?;
    Ok(openrust::http::Json("created"))
}

async fn delete_test_session(session: Session) -> Result<openrust::http::Json<&'static str>> {
    session.flush().map_err(Error::internal)?;
    Ok(openrust::http::Json("deleted"))
}

async fn rotate_test_session(session: Session) -> Result<openrust::http::Json<&'static str>> {
    session.migrate(true).await.map_err(Error::internal)?;
    session
        .put("test.user_id", 42_i64.into())
        .map_err(Error::internal)?;
    session
        .put("test.csrf_hash", "csrf-secret".into())
        .map_err(Error::internal)?;
    Ok(openrust::http::Json("rotated"))
}

fn current_routes(routes: Routes) -> Routes {
    routes
        .get("/current", current_user)
        .post("/current", current_user)
        .layer(AuthLayer::required(AuthMode::SessionOrToken))
        .get("/session", create_test_session)
        .delete("/session", delete_test_session)
        .get("/rotate", rotate_test_session)
}

#[cfg(feature = "session-redis")]
async fn build_redis_session_app(store: RedisSessionStore) -> Result<openrust::App> {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(AuthServiceProvider::with_authenticator(FakeAuthenticator))
        .provider(
            SessionServiceProvider::with_store(store).with_cookie_config(SessionConfig {
                secure: Some(false),
                lifetime: 1,
                cookie: Some("openrust_session".into()),
                ..SessionConfig::default()
            }),
        )
        .provider(TestInstance(std::sync::Arc::new(Marker("injected"))))
        .provider(TestRoutes::with_session(current_routes));
    builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)
}

fn optional_routes(routes: Routes) -> Routes {
    routes
        .get("/optional", optional_user)
        .layer(AuthLayer::optional(AuthMode::SessionOrToken))
        .get("/session", create_test_session)
}

async fn build_app(
    routes: fn(Routes) -> Routes,
    auth: AuthServiceProvider,
) -> Result<openrust::App> {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(auth)
        .provider(
            SessionServiceProvider::with_store(MemoryStore::default()).with_cookie_config(
                SessionConfig {
                    secure: Some(false),
                    cookie: Some("openrust_session".into()),
                    ..SessionConfig::default()
                },
            ),
        )
        .provider(TestInstance(std::sync::Arc::new(Marker("injected"))))
        .provider(TestRoutes::with_session(routes));
    builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)
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
        AuthServiceProvider::with_authenticator(FakeAuthenticator),
    )
    .await?;
    let router = build_router(&app)?;
    let response = router
        .oneshot(
            Request::get("/current")
                .header("host", "localhost")
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
        AuthServiceProvider::with_authenticator(FakeAuthenticator),
    )
    .await?;
    let router = build_router(&app)?;
    let created = router
        .clone()
        .oneshot(
            Request::get("/session")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
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
                .header("host", "localhost")
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
                .header("host", "localhost")
                .header("cookie", cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(payload(denied).await?["code"], "forbidden");

    let accepted = router
        .oneshot(
            Request::post("/current")
                .header("host", "localhost")
                .header("cookie", cookie)
                .header("x-csrf-token", "csrf-secret")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(accepted.status(), StatusCode::OK);
    Ok(())
}

#[cfg(feature = "session-redis")]
#[tokio::test]
async fn redis_sessions_are_shared_between_apps_and_revoke_across_instances() -> TestResult {
    let dsn = std::env::var("OPENRUST_TEST_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379/".to_owned());
    let (store_a, store_b) = verify_redis_session_store_lifecycle(&dsn).await?;
    let app_a = build_redis_session_app(store_a).await?;
    let app_b = build_redis_session_app(store_b.clone()).await?;
    let router_a = build_router(&app_a)?;
    let router_b = build_router(&app_b)?;
    verify_redis_session_http(router_a, router_b, store_b).await
}

#[cfg(feature = "session-redis")]
async fn verify_redis_session_http(
    router_a: openrust::http::axum::Router,
    router_b: openrust::http::axum::Router,
    store_b: RedisSessionStore,
) -> TestResult {
    let created = router_a
        .clone()
        .oneshot(
            Request::get("/session")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(created.status(), StatusCode::OK);
    let set_cookie = created
        .headers()
        .get("set-cookie")
        .ok_or("Redis Session middleware did not set a cookie")?
        .to_str()?;
    let cookie = set_cookie
        .split(';')
        .next()
        .ok_or("Redis Session cookie was empty")?;
    let cookie_value = cookie
        .split_once('=')
        .ok_or("Redis Session cookie had no value")?
        .1;
    let session_id: openrust::session::SessionId = cookie_value.parse()?;
    let loaded_record = store_b
        .load(
            &session_id,
            openrust::session::DEFAULT_MAX_BYTES,
            time::OffsetDateTime::now_utc() + time::Duration::minutes(120),
        )
        .await?
        .ok_or("Redis Session record was not shared")?;
    assert_eq!(loaded_record.data["test"]["user_id"], serde_json::json!(42));
    let session = Session::new(Some(session_id), std::sync::Arc::new(store_b.clone()), None)?;
    session.start().await?;
    assert_eq!(session.get_typed::<i64>("test.user_id")?, Some(42));

    let shared = router_b
        .clone()
        .oneshot(
            Request::get("/current")
                .header("host", "localhost")
                .header("cookie", cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(shared.status(), StatusCode::OK);
    let shared_payload = payload(shared).await?;
    assert_eq!(
        shared_payload["data"], "42:injected:false:false",
        "{shared_payload}"
    );

    let rotated_cookie = rotate_redis_session(&router_a, cookie, &store_b).await?;
    let rotated_shared = router_b
        .clone()
        .oneshot(
            Request::get("/current")
                .header("host", "localhost")
                .header("cookie", &rotated_cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(
        payload(rotated_shared).await?["data"],
        "42:injected:false:false"
    );
    verify_redis_csrf_and_revocation(router_a, router_b, rotated_cookie).await
}

#[cfg(feature = "session-redis")]
async fn verify_redis_csrf_and_revocation(
    router_a: openrust::http::axum::Router,
    router_b: openrust::http::axum::Router,
    cookie: String,
) -> TestResult {
    let denied = router_b
        .clone()
        .oneshot(
            Request::post("/current")
                .header("host", "localhost")
                .header("cookie", &cookie)
                .body(Body::empty())?,
        )
        .await?;
    let denied_payload = payload(denied).await?;
    assert_eq!(denied_payload["code"], "forbidden", "{denied_payload}");
    let accepted = router_b
        .clone()
        .oneshot(
            Request::post("/current")
                .header("host", "localhost")
                .header("cookie", &cookie)
                .header("x-csrf-token", "csrf-secret")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(accepted.status(), StatusCode::OK);

    let deleted = router_a
        .oneshot(
            Request::delete("/session")
                .header("host", "localhost")
                .header("cookie", &cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(deleted.status(), StatusCode::OK);
    let revoked = router_b
        .oneshot(
            Request::get("/current")
                .header("host", "localhost")
                .header("cookie", &cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(payload(revoked).await?["code"], "unauthorized");
    Ok(())
}

#[cfg(feature = "session-redis")]
async fn rotate_redis_session(
    router: &openrust::http::axum::Router,
    cookie: &str,
    store: &RedisSessionStore,
) -> Result<String, Box<dyn StdError>> {
    let rotated = router
        .clone()
        .oneshot(
            Request::get("/rotate")
                .header("host", "localhost")
                .header("cookie", cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(rotated.status(), StatusCode::OK);
    let rotated_cookie = rotated
        .headers()
        .get("set-cookie")
        .ok_or("Session ID rotation did not set a cookie")?
        .to_str()?
        .split(';')
        .next()
        .ok_or("rotated Session cookie was empty")?
        .to_owned();
    assert_ne!(rotated_cookie, cookie);
    let old_id = cookie
        .split_once('=')
        .ok_or("old Session cookie had no value")?
        .1
        .parse::<openrust::session::SessionId>()?;
    assert!(
        store
            .load(
                &old_id,
                openrust::session::DEFAULT_MAX_BYTES,
                time::OffsetDateTime::now_utc() + time::Duration::minutes(120)
            )
            .await?
            .is_none()
    );
    Ok(rotated_cookie)
}

#[cfg(feature = "session-redis")]
async fn verify_redis_session_store_lifecycle(
    dsn: &str,
) -> Result<(RedisSessionStore, RedisSessionStore), Box<dyn StdError>> {
    use openrust::session::{
        DEFAULT_MAX_BYTES as MAX_BYTES, SessionWriteMode as Mode, SessionWriteOutcome as Outcome,
    };
    let empty_expiry = time::OffsetDateTime::now_utc() + time::Duration::minutes(120);
    let store_a = RedisSessionStore::connect(dsn, "auth-runtime").await?;
    assert!(!format!("{store_a:?}").contains(dsn));
    let store_b = RedisSessionStore::connect(dsn, "auth-runtime").await?;
    let isolated_store = RedisSessionStore::connect(dsn, "auth-runtime-isolated").await?;

    assert!(
        RedisSessionStore::connect("redis://127.0.0.1:1/", "invalid namespace")
            .await
            .is_err()
    );
    let unavailable_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let unavailable_port = unavailable_listener.local_addr()?.port();
    drop(unavailable_listener);
    let unavailable_url = format!("redis://127.0.0.1:{unavailable_port}/");
    let Err(unavailable) = RedisSessionStore::connect(&unavailable_url, "auth-unavailable").await
    else {
        return Err("an unreachable Redis endpoint unexpectedly connected".into());
    };
    assert!(!unavailable.to_string().contains(&unavailable_url));

    let record = Record {
        id: Id::generate()?,
        data: serde_json::json!([]),
        expiry_date: time::OffsetDateTime::now_utc() + time::Duration::seconds(30),
    };
    assert_eq!(
        store_a.write(&record, Mode::Insert, MAX_BYTES).await?,
        Outcome::Written
    );
    assert!(
        store_b
            .load(&record.id, MAX_BYTES, empty_expiry)
            .await?
            .is_some()
    );
    assert!(
        isolated_store
            .load(&record.id, MAX_BYTES, empty_expiry)
            .await?
            .is_none()
    );
    let duplicate = record.clone();
    assert_eq!(
        store_b.write(&duplicate, Mode::Insert, MAX_BYTES).await?,
        Outcome::Conflict
    );
    store_a.delete(&record.id).await?;
    assert_eq!(
        store_b.write(&record, Mode::Update, MAX_BYTES).await?,
        Outcome::Missing
    );
    assert!(
        store_b
            .load(&record.id, MAX_BYTES, empty_expiry)
            .await?
            .is_none()
    );

    let expiring_record = Record {
        id: Id::generate()?,
        data: serde_json::json!([]),
        expiry_date: time::OffsetDateTime::now_utc() + time::Duration::milliseconds(80),
    };
    assert_eq!(
        store_a
            .write(&expiring_record, Mode::Insert, MAX_BYTES)
            .await?,
        Outcome::Written
    );
    assert!(
        store_b
            .load(&expiring_record.id, MAX_BYTES, empty_expiry)
            .await?
            .is_some()
    );
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert!(
        store_b
            .load(&expiring_record.id, MAX_BYTES, empty_expiry)
            .await?
            .is_none()
    );
    Ok((store_a, store_b))
}

#[tokio::test]
async fn optional_identity_is_anonymous_only_when_selected_credentials_are_absent() -> TestResult {
    let app = build_app(
        optional_routes,
        AuthServiceProvider::with_authenticator(FakeAuthenticator),
    )
    .await?;
    let router = build_router(&app)?;
    let anonymous = router
        .clone()
        .oneshot(
            Request::get("/optional")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(payload(anonymous).await?["data"], serde_json::Value::Null);

    let malformed = router
        .oneshot(
            Request::get("/optional")
                .header("host", "localhost")
                .header("authorization", "Bearer  ")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(payload(malformed).await?["code"], "unauthorized");
    Ok(())
}

#[tokio::test]
async fn default_database_authenticator_requires_database_at_startup() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder.provider(AuthServiceProvider::new());
    let Err(error) = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)
    else {
        return Err("default auth plugin started without its database".into());
    };
    assert!(error.to_string().contains("openrust::db::Db"), "{error}");
    Ok(())
}

#[tokio::test]
async fn hashing_provider_rejects_invalid_password_capacity_before_registration() -> TestResult {
    let Err(error) = openrust::hashing::HashServiceProvider::with_max_concurrent_operations(0)
    else {
        return Err("hashing provider accepted a zero password operation capacity".into());
    };
    assert_eq!(error.code(), "hashing_capacity_invalid");
    Ok(())
}

#[tokio::test]
async fn duplicate_auth_provider_keeps_first_authenticator() -> TestResult {
    struct RejectAuthenticator;
    impl Authenticator for RejectAuthenticator {
        fn authenticate<'a>(
            &'a self,
            _: AuthCredential<'a>,
        ) -> BoxFuture<'a, Result<AuthPrincipal>> {
            Box::pin(async { Err(Error::Unauthorized) })
        }
    }
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(AuthServiceProvider::with_authenticator(FakeAuthenticator))
        .provider(AuthServiceProvider::with_authenticator(RejectAuthenticator))
        .provider(TestInstance(std::sync::Arc::new(Marker("first"))))
        .provider(TestRoutes::new(current_routes));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let response = build_router(&app)?
        .oneshot(
            Request::get("/current")
                .header("host", "localhost")
                .header("authorization", "Bearer pat-secret")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(payload(response).await?["data"], "42:first:true:true");
    Ok(())
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

struct TestRoutes<F>(std::sync::Arc<F>, u8);
impl<F: Fn(openrust::http::Routes) -> openrust::http::Routes> TestRoutes<F> {
    fn new(routes: F) -> Self {
        Self(std::sync::Arc::new(routes), 0)
    }

    fn with_session(routes: F) -> Self {
        Self(std::sync::Arc::new(routes), 1)
    }

    #[cfg(feature = "encryption")]
    fn with_cookies(routes: F) -> Self {
        Self(std::sync::Arc::new(routes), 2)
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
        let mode = self.1;
        Box::pin(async move {
            app.exceptions(|handler| {
                handler.api_envelope_when(|_, _| true);
            })?;
            app.routes(move |value| match mode {
                1 => value.group(
                    openrust::http::RouteGroup::new().middleware_alias("session"),
                    |value| routes(value),
                ),
                2 => value.group(
                    openrust::http::RouteGroup::new()
                        .middleware_alias("cookies")
                        .middleware_alias("session"),
                    |value| routes(value),
                ),
                _ => routes(value),
            })?;
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

#[cfg(feature = "encryption")]
async fn protected_cookie_app() -> Result<openrust::App> {
    let mut framework = FrameworkConfig::default();
    framework.api.http_status = openrust::config::HttpStatusMode::Semantic;
    let source = figment::Figment::from(figment::providers::Serialized::defaults(framework))
        .merge(("security", serde_json::json!({"encryption":{"active_key_id":"cookie_test","keys":{"cookie_test":base64::engine::general_purpose::STANDARD.encode([42u8;32])}}})));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(AuthServiceProvider::with_authenticator(FakeAuthenticator))
        .provider(
            SessionServiceProvider::with_store(MemoryStore::default()).with_cookie_config(
                SessionConfig {
                    secure: Some(false),
                    cookie: Some("openrust_session".into()),
                    ..SessionConfig::default()
                },
            ),
        )
        .provider(openrust::security::EncryptionServiceProvider)
        .provider(openrust::http::CookieServiceProvider::new())
        .provider(TestInstance(std::sync::Arc::new(Marker("injected"))))
        .provider(TestRoutes::with_cookies(|routes| {
            current_routes(routes)
                .get("/cookie-snapshot", protected_cookie_snapshot)
                .get("/cookie-overflow", oversized_cookie_response)
                .get("/cookie-overflow-attributes", oversized_cookie_attributes)
        }));
    openrust::App::from_provider_runtime(builder.start(source).await?)
}
#[cfg(feature = "encryption")]
async fn oversized_cookie_response(
    jar: openrust::http::CookieJar,
) -> Result<impl axum::response::IntoResponse> {
    use openrust::http::CookieResponseExt;
    Json(serde_json::json!({"ok":true})).with_cookie(jar.factory().make(
        "oversized",
        "x".repeat(60_000),
        0,
        openrust::http::CookieOptions::default(),
    )?)
}

#[cfg(feature = "encryption")]
async fn oversized_cookie_attributes(
    jar: openrust::http::CookieJar,
) -> Result<impl axum::response::IntoResponse> {
    use openrust::http::CookieResponseExt;
    Json(serde_json::json!({"ok":true})).with_cookie(jar.factory().make(
        "oversized",
        "x".repeat(40_000),
        0,
        openrust::http::CookieOptions {
            path: Some(format!("/{}", "p".repeat(20_000))),
            ..openrust::http::CookieOptions::default()
        },
    )?)
}

#[cfg(feature = "encryption")]
async fn protected_cookie_snapshot(
    info: openrust::http::RequestInfo,
    session: Session,
) -> Result<Json<serde_json::Value>> {
    let id = session.id().map(|id| id.to_string());
    Ok(Json(serde_json::json!({
        "same_plaintext": info.cookie("openrust_session") == id.ok().map(serde_json::Value::String),
        "original_wire": info.header("cookie")?.is_some_and(|header| header.starts_with("openrust_session=or1."))
    })))
}
#[cfg(feature = "encryption")]
#[tokio::test]
async fn encrypted_session_reuses_managed_cookie_view_and_rejects_original_wire_duplicates()
-> TestResult {
    let app = protected_cookie_app().await?;
    let result: TestResult = async {
        let router = build_router(&app)?;
        let created = router
            .clone()
            .oneshot(
                Request::get("/session")
                    .header("host", "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        let cookie = created
            .headers()
            .get("set-cookie")
            .ok_or("Session Cookie missing")?
            .to_str()?
            .split(';')
            .next()
            .ok_or("Cookie pair missing")?
            .to_owned();
        assert!(cookie.starts_with("openrust_session=or1.cookie_test."));
        let response = router
            .clone()
            .oneshot(
                Request::get("/current")
                    .header("host", "localhost")
                    .header("cookie", &cookie)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(payload(response).await?["data"], "42:injected:false:false");
        let response = router
            .clone()
            .oneshot(
                Request::get("/cookie-snapshot")
                    .header("host", "localhost")
                    .header("cookie", &cookie)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(
            payload(response).await?["data"],
            serde_json::json!({"same_plaintext":true,"original_wire":true})
        );
        for duplicate in [
            "openrust_session=invalid",
            "openrust.session=invalid",
            "openrust_session[x]=invalid",
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::get("/current")
                        .header("host", "localhost")
                        .header("cookie", &cookie)
                        .header("cookie", duplicate)
                        .body(Body::empty())?,
                )
                .await?;
            assert_eq!(payload(response).await?["code"], "ambiguous_credentials");
        }
        let response = router
            .clone()
            .oneshot(
                Request::post("/current")
                    .header("host", "localhost")
                    .header("cookie", &cookie)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(payload(response).await?["code"], "forbidden");
        let response = router
            .oneshot(
                Request::post("/current")
                    .header("host", "localhost")
                    .header("cookie", &cookie)
                    .header("x-csrf-token", "csrf-secret")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(payload(response).await?["data"], "42:injected:false:false");
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await?;
    result
}

#[cfg(feature = "encryption")]
#[tokio::test]
async fn encrypted_cookie_expansion_fails_without_plaintext_response() -> TestResult {
    let app = protected_cookie_app().await?;
    let result: TestResult = async {
        for path in ["/cookie-overflow", "/cookie-overflow-attributes"] {
            let response = build_router(&app)?
                .oneshot(
                    Request::get(path)
                        .header("host", "localhost")
                        .body(Body::empty())?,
                )
                .await?;
            assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE, "{path}");
            assert!(!response.headers().contains_key("set-cookie"));
            assert_eq!(
                payload(response).await?["code"],
                "input_cookie_limits_exceeded",
                "{path}"
            );
        }
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await?;
    result
}
