//! 验证路由认证与身份提取的运行时契约。
#![cfg(feature = "auth")]

use std::error::Error as StdError;

use base64::Engine as _;
#[cfg(feature = "session-redis")]
use openrust::auth::RedisSessionStore;
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
#[cfg(feature = "session-redis")]
use tower_sessions::session::{Id, Record};
#[cfg(feature = "session-redis")]
use tower_sessions::session_store::SessionStore as _;

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

async fn delete_test_session(session: Session) -> Result<openrust::http::Json<&'static str>> {
    session.flush().await.map_err(Error::internal)?;
    Ok(openrust::http::Json("deleted"))
}

async fn rotate_test_session(session: Session) -> Result<openrust::http::Json<&'static str>> {
    session.cycle_id().await.map_err(Error::internal)?;
    session
        .insert("test.user_id", 42_i64)
        .await
        .map_err(Error::internal)?;
    session
        .insert("test.csrf_hash", "csrf-secret")
        .await
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
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment::Figment::new());
    builder
        .plugin(HttpPlugin)
        .plugin(AuthPlugin::with_authenticator(FakeAuthenticator))
        .plugin(
            SessionPlugin::with_store(store).with_cookie_config(SessionConfig {
                secure: false,
                ttl_secs: 30,
                ..SessionConfig::default()
            }),
        )
        .insert(Marker("injected"))
        .routes(current_routes);
    finalize(builder).await
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
        .oneshot(Request::get("/session").body(Body::empty())?)
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
    let session_id: tower_sessions::session::Id = cookie_value.parse()?;
    let loaded_record = store_b
        .load(&session_id)
        .await?
        .ok_or("Redis Session record was not shared")?;
    assert!(loaded_record.data.contains_key("test.user_id"));
    let session = Session::new(Some(session_id), std::sync::Arc::new(store_b.clone()), None);
    assert_eq!(session.get::<i64>("test.user_id").await?, Some(42));

    let shared = router_b
        .clone()
        .oneshot(
            Request::get("/current")
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
                .header("cookie", &cookie)
                .header("x-csrf-token", "csrf-secret")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(accepted.status(), StatusCode::OK);

    let deleted = router_a
        .oneshot(
            Request::delete("/session")
                .header("cookie", &cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(deleted.status(), StatusCode::OK);
    let revoked = router_b
        .oneshot(
            Request::get("/current")
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
        .parse::<tower_sessions::session::Id>()?;
    assert!(store.load(&old_id).await?.is_none());
    Ok(rotated_cookie)
}

#[cfg(feature = "session-redis")]
async fn verify_redis_session_store_lifecycle(
    dsn: &str,
) -> Result<(RedisSessionStore, RedisSessionStore), Box<dyn StdError>> {
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

    let mut record = Record {
        id: Id::default(),
        data: std::collections::HashMap::new(),
        expiry_date: time::OffsetDateTime::now_utc() + time::Duration::seconds(30),
    };
    store_a.create(&mut record).await?;
    assert!(store_b.load(&record.id).await?.is_some());
    assert!(isolated_store.load(&record.id).await?.is_none());
    let mut duplicate = record.clone();
    assert!(store_b.create(&mut duplicate).await.is_err());
    store_a.delete(&record.id).await?;
    store_b.save(&record).await?;
    assert!(store_b.load(&record.id).await?.is_none());

    let mut expiring_record = Record {
        id: Id::default(),
        data: std::collections::HashMap::new(),
        expiry_date: time::OffsetDateTime::now_utc() + time::Duration::milliseconds(80),
    };
    store_a.create(&mut expiring_record).await?;
    assert!(store_b.load(&expiring_record.id).await?.is_some());
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert!(store_b.load(&expiring_record.id).await?.is_none());
    Ok((store_a, store_b))
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

#[tokio::test]
async fn auth_plugin_rejects_invalid_password_capacity_during_startup() -> TestResult {
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment::Figment::new());
    builder.plugin(
        AuthPlugin::with_authenticator(FakeAuthenticator).with_password_operation_capacity(0),
    );
    let Err(error) = finalize(builder).await else {
        return Err("auth plugin accepted a zero password operation capacity".into());
    };
    assert!(error.to_string().contains("max_concurrent_operations"));
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
