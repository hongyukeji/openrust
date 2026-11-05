//! 实际HTTP scope、参数alias、CurrentUser投影与密码确认存储保护回归。
use super::*;
use crate::{
    auth::{
        AuthCredential, AuthDrivers, AuthPrincipal, AuthServiceProvider, AuthUser, Authenticator,
        Credentials, CurrentUser, GenericUser, Guard,
    },
    config::{FrameworkConfig, HttpStatusMode, NoConfig},
    core::{
        Application, BootContext, BoxFuture, DefaultProviders, RegisterContext, ServiceProvider,
        container::Scope,
    },
    http::{
        CookieFactory, CookieOptions, CookieServiceProvider, HttpServiceProvider, Inject,
        RouteGroup,
    },
    session::{MemorySessionStore, Session, SessionServiceProvider},
    testing::{TestApp, TestResponse},
};
use axum::{
    Extension, Json,
    body::Body,
    extract::Path,
    http::{
        Request, StatusCode,
        header::{COOKIE, LOCATION, SET_COOKIE},
    },
};
use figment::{Figment, providers::Serialized};
use serde_json::{Map, Value, json};
use std::{
    sync::{
        Mutex, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

const NAME: &str = "middleware_contract_session";
const FORGED_ID: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const TIMEOUT: Duration = Duration::from_secs(5);
fn fixture_error() -> Error {
    Error::custom(
        "auth_middleware_fixture_failed",
        "认证middleware测试状态错误",
    )
}
struct RejectLegacy;
impl Authenticator for RejectLegacy {
    fn authenticate<'a>(&'a self, _: AuthCredential<'a>) -> BoxFuture<'a, Result<AuthPrincipal>> {
        Box::pin(async { Err(Error::Unauthorized) })
    }
}
struct HeaderGuard(RwLock<Option<AuthUser>>);
impl Guard for HeaderGuard {
    fn user(&self) -> BoxFuture<'_, Result<Option<AuthUser>>> {
        Box::pin(async { Ok(self.0.read().map_err(|_| fixture_error())?.clone()) })
    }
    fn validate<'a>(&'a self, _: &'a Credentials) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async { Ok(false) })
    }
    fn has_user(&self) -> Result<bool> {
        Ok(self.0.read().map_err(|_| fixture_error())?.is_some())
    }
    fn set_user(&self, user: AuthUser) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            *self.0.write().map_err(|_| fixture_error())? = Some(user);
            Ok(())
        })
    }
    fn provider_name(&self) -> Option<&str> {
        Some("header-realm")
    }
}
#[derive(Default)]
struct Calls {
    profiles: AtomicUsize,
    protected: AtomicUsize,
    request_contexts: Mutex<Vec<(String, bool)>>,
}
struct NullGuestRedirect;
impl ServiceProvider for NullGuestRedirect {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.instance(Arc::new(
                AuthenticationRedirects::new().redirect_guests_using(|_| Ok(None)),
            ))
            .await
        })
    }
}
struct App<const MODE: u8>;
impl<const MODE: u8> Application for App<MODE> {
    type Config = NoConfig;
    fn default_providers() -> DefaultProviders {
        DefaultProviders::empty()
    }
    fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers: Vec<Box<dyn ServiceProvider>> = vec![Box::new(HttpServiceProvider)];
        if MODE == 3 {
            providers.push(Box::new(NullGuestRedirect));
        }
        providers.push(Box::new(AuthServiceProvider::with_authenticator(
            RejectLegacy,
        )));
        if MODE == 1 || MODE == 2 {
            providers.push(Box::new(SessionServiceProvider::new()));
        } else {
            providers.push(Box::new(SessionServiceProvider::with_store(
                MemorySessionStore::default(),
            )));
        }
        #[cfg(feature = "encryption")]
        if MODE == 2 {
            providers.push(Box::new(crate::security::EncryptionServiceProvider));
            providers.push(Box::new(
                CookieServiceProvider::new().except([NAME, FORGED_ID]),
            ));
        } else {
            providers.push(Box::new(CookieServiceProvider::plain()));
        }
        #[cfg(not(feature = "encryption"))]
        providers.push(Box::new(CookieServiceProvider::plain()));
        providers.push(Box::new(Routes));
        providers
    }
}
struct Routes;
impl ServiceProvider for Routes {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { app.instance(Arc::new(Calls::default())).await })
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let drivers = app.resolve::<AuthDrivers>().await?;
            drivers.extend("header", |context| {
                Box::pin(async move {
                    // 这条正式公开扩展必须由auth alias准备上下文，不能依赖Session层兜底。
                    let info = context.request().await?;
                    let calls = context.resolver.resolve::<Calls>().await?;
                    calls
                        .request_contexts
                        .lock()
                        .map_err(|_| fixture_error())?
                        .push((context.name.clone(), info.has_session()));
                    let header = if context.name == "web" {
                        "x-web-user"
                    } else {
                        "x-api-user"
                    };
                    let user = info.header(header)?.map(|id| {
                        Arc::new(GenericUser::new(Map::from_iter([("id".into(), json!(id))])))
                            as AuthUser
                    });
                    Ok(Arc::new(HeaderGuard(RwLock::new(user))) as Arc<dyn Guard>)
                })
            })?;
            app.exceptions(|handler| {
                handler.api_envelope_when(|request, _| request.path() == "/sensitive");
            })?;
            app.routes(|routes| {
                routes
                    .group(RouteGroup::new().middleware_alias("auth:api"), |r| {
                        r.get("/auth", current)
                    })
                    .group(RouteGroup::new().middleware_alias("auth:"), |r| {
                        r.get("/empty-auth", current)
                    })
                    .group(RouteGroup::new().middleware_alias("auth:,api"), |r| {
                        r.get("/fallback-auth", current)
                    })
                    .group(RouteGroup::new().middleware_alias("guest:"), |r| {
                        r.get("/empty-guest", public)
                    })
                    .group(RouteGroup::new().middleware_alias("guest:,api"), |r| {
                        r.get("/mixed-guest", public)
                    })
                    .group(
                        RouteGroup::new()
                            .middleware_alias("cookies")
                            .middleware_alias("session"),
                        |r| {
                            r.get("/confirmed/{age}", confirmed)
                                .get("/inspect-confirmation", inspect_confirmation)
                                .group(
                                    RouteGroup::new()
                                        .middleware_alias("auth:api")
                                        .middleware_alias("password.confirm"),
                                    |r| r.get("/sensitive", sensitive),
                                )
                        },
                    )
                    .group(RouteGroup::new().domain("other.example.test"), |r| {
                        r.get("/dashboard", public)
                    })
                    .get("/home", public)
            })?;
            Ok(())
        })
    }
}
async fn current(
    user: CurrentUser,
    info: RequestInfo,
    Extension(scope): Extension<Arc<Scope>>,
    Inject(calls): Inject<Calls>,
) -> Result<Json<Value>> {
    calls.profiles.fetch_add(1, Ordering::SeqCst);
    let manager = scope.resolve::<AuthManager>().await?;
    let first = manager.guard(None).await?;
    let second = manager.guard(user.guard_name()).await?;
    if !Arc::ptr_eq(&first, &second) {
        return Err(fixture_error());
    }
    if !matches!(user.source(), crate::auth::AuthSource::Guard) {
        return Err(fixture_error());
    }
    Ok(Json(
        json!({"id":user.id(), "guard":user.guard_name(), "provider":user.provider_name(), "default":manager.default_driver()?, "has_session":info.has_session()}),
    ))
}
async fn public() -> Json<Value> {
    Json(json!({"public":true}))
}
async fn confirmed(session: Session, Path(age): Path<i64>) -> Result<Json<Value>> {
    session.put_typed(
        "auth.password_confirmed_at",
        chrono::Utc::now().timestamp() - age,
    )?;
    Ok(Json(json!({"seeded":true})))
}
async fn inspect_confirmation(session: Session) -> Result<Json<Value>> {
    Ok(Json(
        json!({"confirmed":session.get_json("auth.password_confirmed_at")?}),
    ))
}
async fn sensitive(user: CurrentUser, Inject(calls): Inject<Calls>) -> Json<Value> {
    calls.protected.fetch_add(1, Ordering::SeqCst);
    Json(json!({"id":user.id()}))
}
fn sources(mode: u8) -> Figment {
    let mut defaults = FrameworkConfig::default();
    defaults.api.http_status = HttpStatusMode::Semantic;
    Figment::from(Serialized::defaults(defaults)).merge(Serialized::defaults(json!({
        "app":{"env":"test","url":"http://middleware.test"},
        "auth":{"defaults":{"guard":"web","passwords":"users","provider":null},"guards":{"web":{"driver":"header","provider":null},"api":{"driver":"header","provider":null}},"password_timeout":10800},
        "session":{"driver":if mode == 1 || mode == 2 { "cookie" } else { "array" },"cookie":NAME,"secure":false,"lifetime":120,"lottery":[0,100]},
        "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
    })))
}
async fn get<const MODE: u8>(
    app: &TestApp<App<MODE>>,
    path: &str,
    headers: &[(&str, &str)],
    json: bool,
) -> Result<TestResponse> {
    let mut request = Request::get(path).header("host", "middleware.test").header(
        "accept",
        if json {
            "application/json"
        } else {
            "text/html"
        },
    );
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    app.request_with_limits(
        request.body(Body::empty()).map_err(Error::internal)?,
        16 * 1024,
        TIMEOUT,
    )
    .await
}
fn session_cookie(response: &TestResponse) -> Result<String> {
    response
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|header| header.to_str().ok())
        .find(|value| value.starts_with(&format!("{NAME}=")))
        .and_then(|value| value.split(';').next())
        .map(str::to_owned)
        .ok_or_else(fixture_error)
}

#[tokio::test]
async fn auth_alias_prepares_header_driver_without_session_and_projects_current_user_in_same_scope()
-> Result<()> {
    let mut app = TestApp::<App<0>>::builder(sources(0)).build().await?;
    let result: Result<()> = async {
        let response = get(&app, "/auth", &[("x-api-user", "tenant:api-42")], true).await?;
        response.assert_status(StatusCode::OK)?;
        let body = response.json::<Value>()?;
        assert_eq!(body, json!({"id":"tenant:api-42","guard":"api","provider":"header-realm","default":"api","has_session":false}));
        let unauthenticated = get(&app, "/auth", &[], true).await?;
        unauthenticated.assert_status(StatusCode::UNAUTHORIZED)?;
        assert_eq!(unauthenticated.json::<Value>()?["message"], "Unauthenticated.");
        let calls = app.app()?.state().resolve::<Calls>().await?;
        assert_eq!(calls.profiles.load(Ordering::SeqCst), 1);
        assert!(calls.request_contexts.lock().map_err(|_| fixture_error())?.iter().all(|(_, has_session)| !has_session));
        Ok(())
    }.await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    result?;
    shutdown
}

#[tokio::test]
async fn empty_guard_aliases_fall_back_and_guest_ignores_dashboard_on_another_domain() -> Result<()>
{
    let mut app = TestApp::<App<0>>::builder(sources(0)).build().await?;
    let result: Result<()> = async {
        let response = get(&app, "/empty-auth", &[("x-web-user", "web-42")], true).await?;
        response.assert_status(StatusCode::OK)?;
        assert_eq!(response.json::<Value>()?["guard"], "web");
        let response = get(&app, "/fallback-auth", &[("x-api-user", "api-42")], true).await?;
        response.assert_status(StatusCode::OK)?;
        assert_eq!(response.json::<Value>()?["default"], "api");
        get(&app, "/empty-guest", &[], true)
            .await?
            .assert_status(StatusCode::OK)?;
        for (path, header) in [
            ("/empty-guest", "x-web-user"),
            ("/mixed-guest", "x-api-user"),
        ] {
            let response = get(&app, path, &[(header, "authenticated")], true).await?;
            response.assert_status(StatusCode::FOUND)?;
            assert_eq!(
                response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok()),
                Some("http://middleware.test/home")
            );
        }
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    result?;
    shutdown
}
async fn forged_confirmation_case<const MODE: u8>() -> Result<()> {
    let mut app = TestApp::<App<MODE>>::builder(sources(MODE)).build().await?;
    let result: Result<()> = async {
        get(&app, "/auth", &[("x-api-user", "stolen-api-token")], true)
            .await?
            .assert_status(StatusCode::OK)?;
        let confirmed_at = chrono::Utc::now().timestamp();
        let data = serde_json::to_string(&json!({"auth":{"password_confirmed_at":confirmed_at}}))
            .map_err(Error::internal)?;
        let payload = serde_json::to_string(
            &json!({"data":data,"expires":time::OffsetDateTime::now_utc().unix_timestamp() + 300}),
        )
        .map_err(Error::internal)?;
        let header = CookieFactory::default()
            .make(FORGED_ID, payload, 5, CookieOptions::default())?
            .to_header(false)?;
        let pair = header
            .to_str()
            .map_err(Error::internal)?
            .split(';')
            .next()
            .ok_or_else(fixture_error)?;
        let cookie = format!("{NAME}={FORGED_ID}; {pair}");
        let observed = get(
            &app,
            "/inspect-confirmation",
            &[(COOKIE.as_str(), &cookie)],
            true,
        )
        .await?;
        observed.assert_status(StatusCode::OK)?;
        assert_eq!(
            observed.json::<Value>()?["confirmed"],
            confirmed_at,
            "伪造时间必须实际载入Session，不能用被忽略的Cookie冒充负例"
        );
        let response = get(
            &app,
            "/sensitive",
            &[
                ("x-api-user", "stolen-api-token"),
                (COOKIE.as_str(), &cookie),
            ],
            true,
        )
        .await?;
        response.assert_status(StatusCode::INTERNAL_SERVER_ERROR)?;
        assert_eq!(
            response.json::<Value>()?["code"],
            "session_cookie_protection_required",
            "必须确实拒绝不可信确认时间，不能由坏Cookie/缺Guard制造假负例"
        );
        assert_eq!(
            app.app()?
                .state()
                .resolve::<Calls>()
                .await?
                .protected
                .load(Ordering::SeqCst),
            0
        );
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    result?;
    shutdown
}
#[tokio::test]
async fn forged_plain_or_except_cookie_session_cannot_skip_password_confirmation() -> Result<()> {
    forged_confirmation_case::<1>().await?;
    #[cfg(feature = "encryption")]
    forged_confirmation_case::<2>().await?;
    Ok(())
}

#[tokio::test]
async fn normal_server_session_uses_three_hour_timeout_with_non_racy_margin() -> Result<()> {
    let mut app = TestApp::<App<0>>::builder(sources(0)).build().await?;
    let result: Result<()> = async {
        for (age, status) in [(10_740, StatusCode::OK), (10_860, StatusCode::LOCKED)] {
            let response = get(&app, &format!("/confirmed/{age}"), &[], true).await?;
            response.assert_status(StatusCode::OK)?;
            let cookie = session_cookie(&response)?;
            let response = get(
                &app,
                "/sensitive",
                &[("x-api-user", "api-42"), (COOKIE.as_str(), &cookie)],
                true,
            )
            .await?;
            response.assert_status(status)?;
            if status == StatusCode::LOCKED {
                assert_eq!(
                    response.json::<Value>()?["message"],
                    "Password confirmation required."
                );
            }
        }
        assert_eq!(
            app.app()?
                .state()
                .resolve::<Calls>()
                .await?
                .protected
                .load(Ordering::SeqCst),
            1
        );
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    result?;
    shutdown
}

#[tokio::test]
async fn explicit_null_guest_redirect_returns_empty_401_without_login_route_fallback() -> Result<()>
{
    let mut app = TestApp::<App<3>>::builder(sources(3)).build().await?;
    let result: Result<()> = async {
        let response = get(&app, "/auth", &[], false).await?;
        response.assert_status(StatusCode::UNAUTHORIZED)?;
        assert_eq!(response.body(), b"");
        assert!(response.headers().get(LOCATION).is_none());
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    result?;
    shutdown
}
