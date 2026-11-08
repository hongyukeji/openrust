//! 生产Web保护、真实SessionGuard及公开响应/路由扩展的HTTP组合回归。
use super::*;
use crate::auth::AuthCredential;
use crate::auth::AuthDrivers;
use crate::auth::AuthPrincipal;
use crate::auth::AuthServiceProvider;
use crate::auth::Authenticatable;
use crate::auth::Authenticator;
use crate::auth::GenericUser;
use crate::auth::UserProvider;
use crate::auth::fortify::FortifyServiceProvider;
use crate::auth::fortify::TwoFactorAuthenticatable;
use crate::auth::fortify::TwoFactorState;
use crate::cache::CacheServiceProvider;
use crate::config::FrameworkConfig;
use crate::config::HttpStatusMode;
use crate::config::NoConfig;
use crate::core::RateLimitBucket;
use crate::core::RateLimitFuture;
use crate::core::RateLimitStore;
use crate::core::RateLimiter;
use crate::events::EventsServiceProvider;
use crate::foundation::ApplicationDefinition;
use crate::foundation::BootContext;
use crate::foundation::RegisterContext;
use crate::http::CookieServiceProvider;
use crate::http::HttpServiceProvider;
use crate::http::Request as ParsedRequest;
use crate::http::RequestInfo;
use crate::session::MemorySessionStore;
use crate::session::Session;
use crate::session::SessionServiceProvider;
use crate::support::{BoxFuture, DefaultProviders, ServiceProvider};
use crate::testing::TestApp;
use crate::testing::TestResponse;
use axum::{
    extract::{Path, State},
    http::{Method, Request as RawRequest, header},
    middleware::Next,
    response::Html,
};
use figment::{Figment, providers::Serialized};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(5);
const USER: &str = "http:user-42";
fn fixture_error() -> Error {
    Error::custom("fortify_http_fixture_failed", "Fortify HTTP测试状态错误")
}
struct Account {
    attributes: GenericUser,
    factor: RwLock<TwoFactorState>,
    writes: AtomicUsize,
}
impl Authenticatable for Account {
    fn as_two_factor(&self) -> Option<&dyn TwoFactorAuthenticatable> {
        Some(self)
    }
    fn auth_attribute(&self, name: &str) -> Result<Option<Value>> {
        self.attributes.attribute(name)
    }
    fn auth_identifier_name(&self) -> &'static str {
        "id"
    }
    fn auth_identifier(&self) -> Result<Value> {
        self.attributes.auth_identifier()
    }
    fn auth_password_name(&self) -> &'static str {
        "password"
    }
    fn auth_password(&self) -> Result<Option<String>> {
        self.attributes.auth_password()
    }
    fn remember_token(&self) -> Result<Option<String>> {
        self.attributes.remember_token()
    }
    fn remember_token_for_cookie(&self) -> Result<Option<String>> {
        self.attributes.remember_token_for_cookie()
    }
    fn set_remember_token(
        &self,
        token: Option<String>,
    ) -> crate::support::BoxFuture<'_, Result<()>> {
        self.attributes.set_remember_token(token)
    }
    fn begin_remember_token_update<'a>(
        &'a self,
        expected: Option<&'a str>,
        replacement: Option<String>,
    ) -> crate::support::BoxFuture<'a, Result<Option<crate::auth::RememberTokenUpdate>>> {
        self.attributes
            .begin_remember_token_update(expected, replacement)
    }
    fn remember_token_name(&self) -> &'static str {
        "remember_token"
    }
}
impl TwoFactorAuthenticatable for Account {
    fn two_factor_state(&self) -> BoxFuture<'_, Result<TwoFactorState>> {
        Box::pin(async { Ok(self.factor.read().map_err(|_| fixture_error())?.clone()) })
    }
    fn compare_and_set_two_factor_state<'a>(
        &'a self,
        expected: &'a TwoFactorState,
        replacement: &'a TwoFactorState,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let mut state = self.factor.write().map_err(|_| fixture_error())?;
            if &*state != expected {
                return Ok(false);
            }
            *state = replacement.clone();
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        })
    }
    fn two_factor_account_name(&self, _: &str) -> Result<Option<String>> {
        Ok(Some("alice".into()))
    }
}
struct Accounts {
    user: Arc<Account>,
    credentials: AtomicUsize,
}
impl UserProvider for Accounts {
    fn retrieve_by_id<'a>(&'a self, id: &'a Value) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async move { Ok((id == &json!(USER)).then(|| self.user.clone() as AuthUser)) })
    }
    fn retrieve_by_token<'a>(
        &'a self,
        _: &'a Value,
        _: &'a str,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async { Ok(None) })
    }
    fn update_remember_token<'a>(
        &'a self,
        _: &'a dyn Authenticatable,
        _: &'a str,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn retrieve_by_credentials<'a>(
        &'a self,
        credentials: &'a Credentials,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async move {
            self.credentials.fetch_add(1, Ordering::SeqCst);
            Ok((credentials.get("handle") == Some(&json!("alice")))
                .then(|| self.user.clone() as AuthUser))
        })
    }
    fn validate_credentials<'a>(
        &'a self,
        _: &'a dyn Authenticatable,
        credentials: &'a Credentials,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move { Ok(credentials.get("password") == Some(&json!("secret"))) })
    }
    fn rehash_password_if_required<'a>(
        &'a self,
        _: &'a dyn Authenticatable,
        _: &'a Credentials,
        _: bool,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
struct Stats {
    accounts: Arc<Accounts>,
    views: AtomicUsize,
    login_responses: AtomicUsize,
    enabled_responses: AtomicUsize,
    aliases: Mutex<Vec<(String, Vec<String>)>>,
}
impl Stats {
    fn new() -> Self {
        let user = Arc::new(Account {
            attributes: GenericUser::new(serde_json::Map::from_iter([
                ("id".into(), json!(USER)),
                ("handle".into(), json!("alice")),
                ("password".into(), json!("encoded-hash")),
            ])),
            factor: RwLock::new(TwoFactorState::default()),
            writes: AtomicUsize::new(0),
        });
        Self {
            accounts: Arc::new(Accounts {
                user,
                credentials: AtomicUsize::new(0),
            }),
            views: AtomicUsize::new(0),
            login_responses: AtomicUsize::new(0),
            enabled_responses: AtomicUsize::new(0),
            aliases: Mutex::new(Vec::new()),
        }
    }
}
#[derive(Default)]
struct MemoryRates(Mutex<BTreeMap<[u8; 32], (u64, Instant)>>);
impl RateLimitStore for MemoryRates {
    fn hit<'a>(
        &'a self,
        key: &'a str,
        decay: Duration,
    ) -> RateLimitFuture<'a, Result<RateLimitBucket>> {
        Box::pin(async move {
            let now = Instant::now();
            let digest: [u8; 32] = Sha256::digest(key.as_bytes()).into();
            let mut entries = self.0.lock().map_err(|_| fixture_error())?;
            let deadline = now.checked_add(decay).ok_or_else(fixture_error)?;
            let entry = entries.entry(digest).or_insert((0, deadline));
            if entry.1 <= now {
                *entry = (0, deadline);
            }
            entry.0 = entry.0.checked_add(1).ok_or_else(fixture_error)?;
            Ok(RateLimitBucket::new(
                entry.0,
                entry.1.saturating_duration_since(now).as_secs(),
            ))
        })
    }
    fn attempts<'a>(
        &'a self,
        key: &'a str,
    ) -> RateLimitFuture<'a, Result<Option<RateLimitBucket>>> {
        Box::pin(async move {
            let now = Instant::now();
            let digest: [u8; 32] = Sha256::digest(key.as_bytes()).into();
            Ok(self
                .0
                .lock()
                .map_err(|_| fixture_error())?
                .get(&digest)
                .filter(|(_, until)| *until > now)
                .map(|(count, until)| {
                    RateLimitBucket::new(*count, until.saturating_duration_since(now).as_secs())
                }))
        })
    }
    fn clear<'a>(&'a self, key: &'a str) -> RateLimitFuture<'a, Result<()>> {
        Box::pin(async move {
            let digest: [u8; 32] = Sha256::digest(key.as_bytes()).into();
            self.0.lock().map_err(|_| fixture_error())?.remove(&digest);
            Ok(())
        })
    }
    fn consume<'a>(
        &'a self,
        key: &'a str,
        window: Duration,
    ) -> RateLimitFuture<'a, Result<RateLimitBucket>> {
        self.hit(key, window)
    }
}
struct RejectLegacy;
impl Authenticator for RejectLegacy {
    fn authenticate<'a>(&'a self, _: AuthCredential<'a>) -> BoxFuture<'a, Result<AuthPrincipal>> {
        Box::pin(async { Err(Error::Unauthorized) })
    }
}
struct Views(Arc<Stats>);
macro_rules! view_response {
    ($contract:ident) => {
        impl $contract for Views {
            fn to_response<'a>(&'a self, _: &'a ParsedRequest) -> BoxFuture<'a, Result<Response>> {
                Box::pin(async move {
                    self.0.views.fetch_add(1, Ordering::SeqCst);
                    Ok(Html("<h1>fixture-view</h1>").into_response())
                })
            }
        }
    };
}
view_response!(LoginViewResponse);
view_response!(ConfirmPasswordViewResponse);
view_response!(TwoFactorChallengeViewResponse);
struct CustomResponses(Arc<Stats>);
impl LoginResponse for CustomResponses {
    fn to_response<'a>(&'a self, _: &'a ParsedRequest) -> BoxFuture<'a, Result<Response>> {
        Box::pin(async {
            self.0.login_responses.fetch_add(1, Ordering::SeqCst);
            Ok((StatusCode::ACCEPTED, Json(json!({"binding":"login"}))).into_response())
        })
    }
}
impl TwoFactorEnabledResponse for CustomResponses {
    fn to_response<'a>(&'a self, _: &'a ParsedRequest) -> BoxFuture<'a, Result<Response>> {
        Box::pin(async {
            self.0.enabled_responses.fetch_add(1, Ordering::SeqCst);
            Ok((StatusCode::ACCEPTED, Json(json!({"binding":"enabled"}))).into_response())
        })
    }
}
struct App<const MODE: u8>;
impl<const MODE: u8> ApplicationDefinition for App<MODE> {
    type Config = NoConfig;
    fn default_providers() -> DefaultProviders {
        DefaultProviders::empty()
    }
    fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
        vec![
            Box::new(HttpServiceProvider),
            Box::new(crate::encryption::EncryptionServiceProvider),
            Box::new(CookieServiceProvider::contextual()),
            Box::new(SessionServiceProvider::with_store(
                MemorySessionStore::default(),
            )),
            Box::new(AuthServiceProvider::with_authenticator(RejectLegacy)),
            Box::new(EventsServiceProvider::new()),
            Box::new(CacheServiceProvider::new()),
            Box::new(Support::<MODE>),
            Box::new(FortifyServiceProvider::new()),
        ]
    }
}
struct Support<const MODE: u8>;
impl<const MODE: u8> ServiceProvider for Support<MODE> {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let stats = Arc::new(Stats::new());
            app.instance(stats.clone()).await?;
            app.instance(Arc::new(RateLimiter::new(MemoryRates::default())))
                .await?;
            app.instance::<dyn LoginViewResponse>(Arc::new(Views(stats.clone())))
                .await?;
            app.instance::<dyn ConfirmPasswordViewResponse>(Arc::new(Views(stats.clone())))
                .await?;
            app.instance::<dyn TwoFactorChallengeViewResponse>(Arc::new(Views(stats.clone())))
                .await?;
            if MODE == 2 {
                app.instance::<dyn LoginResponse>(Arc::new(CustomResponses(stats.clone())))
                    .await?;
                app.instance::<dyn TwoFactorEnabledResponse>(Arc::new(CustomResponses(stats)))
                    .await?;
            }
            Ok(())
        })
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let stats = app.resolve::<Stats>().await?;
            let accounts = stats.accounts.clone();
            app.resolve::<AuthDrivers>()
                .await?
                .provider("http-accounts", move |_, _| {
                    let accounts = accounts.clone();
                    Box::pin(async move { Ok(accounts as Arc<dyn UserProvider>) })
                })?;
            if MODE == 3 {
                for name in ["guest", "auth", "password.confirm"] {
                    let stats = stats.clone();
                    app.middleware_alias_with_parameters(name, move |arguments| {
                        Ok(axum::middleware::from_fn_with_state(
                            Alias {
                                name,
                                arguments,
                                stats: stats.clone(),
                            },
                            alias,
                        ))
                    })?;
                }
            }
            app.routes(|routes| {
                routes.group(RouteGroup::new().middleware_group("web"), |r| {
                    r.get("/fixture/session", bootstrap)
                        .get("/fixture/age/{age}", age)
                })
            })?;
            Ok(())
        })
    }
}
#[derive(Clone)]
struct Alias {
    name: &'static str,
    arguments: Vec<String>,
    stats: Arc<Stats>,
}
async fn alias(
    State(rule): State<Alias>,
    request: RawRequest<axum::body::Body>,
    next: Next,
) -> Response {
    if let Ok(mut calls) = rule.stats.aliases.lock() {
        calls.push((rule.name.into(), rule.arguments));
    }
    if request
        .headers()
        .get("x-reject-alias")
        .and_then(|value| value.to_str().ok())
        == Some(rule.name)
    {
        return (StatusCode::IM_A_TEAPOT, [("x-contract-alias", rule.name)]).into_response();
    }
    next.run(request).await
}
async fn bootstrap(
    session: Session,
    info: RequestInfo,
    Inject(fortify): Inject<Fortify>,
) -> Result<Json<Value>> {
    let urls = info.route_urls()?;
    let mut routes = serde_json::Map::new();
    for name in [
        "login",
        "login.store",
        "logout",
        "password.confirm",
        "password.confirm.store",
        "password.confirmation",
        "two-factor.login",
        "two-factor.login.store",
        "two-factor.enable",
        "two-factor.disable",
        "two-factor.confirm",
        "two-factor.regenerate-recovery-codes",
        "two-factor.qr-code",
        "two-factor.secret-key",
        "two-factor.recovery-codes",
    ] {
        routes.insert(
            name.into(),
            if urls.has(name) {
                json!(urls.route(name, &BTreeMap::new())?)
            } else {
                Value::Null
            },
        );
    }
    Ok(Json(
        json!({"csrf":session.token()?,"authenticated":fortify.guard().await?.check().await?,"routes":routes}),
    ))
}
async fn age(session: Session, Path(seconds): Path<i64>) -> Result<Json<Value>> {
    session.put_typed(
        "auth.password_confirmed_at",
        chrono::Utc::now().timestamp() - seconds,
    )?;
    Ok(Json(json!({"seeded":true})))
}
// TestApp带TestingBoot标记，env=local使本组请求仍经过真实CSRF验证。
fn sources(mode: u8) -> Figment {
    let mut defaults = FrameworkConfig::default();
    defaults.api.http_status = HttpStatusMode::Semantic;
    let mut input = json!({"app":{"env":"local","url":"http://fortify.test","name":"Fortify fixture","key":"0123456789abcdef0123456789abcdef"},"auth":{"guards":{"web":{"driver":"session","provider":"users"}},"providers":{"users":{"driver":"http-accounts"}}},"cache":{"default":"fixture","stores":{"fixture":{"driver":"memory"}}},"fortify":{"username":"handle"},"fortify-options":{"two-factor-authentication":{"confirm":false,"confirmPassword":mode == 3 || mode == 4}},"session":{"cookie":"fortify_http_session","secure":false,"lottery":[0,100]},"security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}});
    if mode == 1 {
        input["fortify"]["views"] = json!(false);
        input["fortify"]["prefix"] = json!("security");
        input["fortify"]["domain"] = json!("accounts.example.test");
        input["fortify"]["paths"] = json!({"login":"/sign-in","password":{"confirm":"/password/recheck","confirmation":"/password/status"},"two-factor":{"login":"/mfa/challenge","enable":"/mfa/setup"}});
    }
    Figment::from(Serialized::defaults(defaults)).merge(Serialized::defaults(input))
}
#[derive(Default)]
struct Browser {
    cookies: BTreeMap<String, String>,
    token: Option<String>,
}
impl Browser {
    async fn send<const MODE: u8>(
        &mut self,
        app: &TestApp<App<MODE>>,
        method: Method,
        path: &str,
        body: Option<Value>,
        csrf: bool,
        extra: &[(&str, &str)],
    ) -> Result<TestResponse> {
        let host = if MODE == 1 {
            "accounts.example.test"
        } else {
            "fortify.test"
        };
        let mut request = RawRequest::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, host)
            .header(header::ACCEPT, "application/json");
        if !self.cookies.is_empty() {
            request = request.header(
                header::COOKIE,
                self.cookies
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            );
        }
        if csrf {
            request = request.header(
                "x-csrf-token",
                self.token.as_deref().ok_or_else(fixture_error)?,
            );
        }
        for (key, value) in extra {
            request = request.header(*key, *value);
        }
        let body = match body {
            Some(value) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                axum::body::Body::from(serde_json::to_vec(&value).map_err(Error::internal)?)
            }
            None => axum::body::Body::empty(),
        };
        let response = app
            .request_with_limits(
                request.body(body).map_err(Error::internal)?,
                128 * 1024,
                TIMEOUT,
            )
            .await?;
        for cookie in response.headers().get_all(header::SET_COOKIE) {
            if let Some((key, value)) = cookie
                .to_str()
                .ok()
                .and_then(|cookie| cookie.split(';').next())
                .and_then(|cookie| cookie.split_once('='))
            {
                if value.is_empty() {
                    self.cookies.remove(key);
                } else {
                    self.cookies.insert(key.into(), value.into());
                }
            }
        }
        Ok(response)
    }
    async fn refresh<const MODE: u8>(&mut self, app: &TestApp<App<MODE>>) -> Result<Value> {
        let response = self
            .send(app, Method::GET, "/fixture/session", None, false, &[])
            .await?;
        response.assert_status(StatusCode::OK)?;
        let body = response.json::<Value>()?;
        self.token = Some(body["csrf"].as_str().ok_or_else(fixture_error)?.into());
        Ok(body)
    }
    async fn login<const MODE: u8>(&mut self, app: &TestApp<App<MODE>>) -> Result<TestResponse> {
        self.refresh(app).await?;
        let path = if MODE == 1 {
            "/security/sign-in"
        } else {
            "/login"
        };
        self.send(
            app,
            Method::POST,
            path,
            Some(json!({"handle":"alice","password":"secret"})),
            true,
            &[],
        )
        .await
    }
}

#[tokio::test]
async fn views_false_prefix_domain_paths_and_named_urls_share_the_actual_http_routes() -> Result<()>
{
    let mut app = TestApp::<App<1>>::builder(sources(1)).build().await?;
    let result: Result<()> = async {
        let mut browser = Browser::default();
        let info = browser.refresh(&app).await?;
        for name in ["login", "password.confirm", "two-factor.login"] {
            assert!(info["routes"][name].is_null());
        }
        for (name, suffix) in [
            ("login.store", "/sign-in"),
            ("password.confirm.store", "/password/recheck"),
            ("password.confirmation", "/password/status"),
            ("two-factor.enable", "/mfa/setup"),
            ("two-factor.login.store", "/mfa/challenge"),
        ] {
            assert_eq!(
                info["routes"][name],
                format!("http://accounts.example.test/security{suffix}")
            );
        }
        assert_eq!(
            info["routes"]["two-factor.disable"],
            "http://accounts.example.test/security/user/two-factor-authentication"
        );
        assert_eq!(
            info["routes"]["two-factor.regenerate-recovery-codes"],
            "http://accounts.example.test/security/user/two-factor-recovery-codes"
        );
        for path in [
            "/security/sign-in",
            "/security/password/recheck",
            "/security/mfa/challenge",
        ] {
            browser
                .send(&app, Method::GET, path, None, false, &[])
                .await?
                .assert_status(StatusCode::METHOD_NOT_ALLOWED)?;
        }
        let response = browser.login(&app).await?;
        response.assert_status(StatusCode::OK)?;
        assert_eq!(response.json::<Value>()?, json!({"two_factor":false}));
        assert_eq!(
            app.app()?
                .state()
                .resolve::<Stats>()
                .await?
                .views
                .load(Ordering::SeqCst),
            0
        );
        let request = RawRequest::post("/security/sign-in")
            .header(header::HOST, "fortify.test")
            .header(header::ACCEPT, "application/json")
            .body(axum::body::Body::empty())
            .map_err(Error::internal)?;
        app.request_with_limits(request, 4096, TIMEOUT)
            .await?
            .assert_status(StatusCode::NOT_FOUND)?;
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    result?;
    shutdown
}

#[tokio::test]
async fn default_success_payloads_are_bare_and_real_web_csrf_rejection_has_no_account_side_effect()
-> Result<()> {
    let mut app = TestApp::<App<0>>::builder(sources(0)).build().await?;
    let result: Result<()> = async {
        let mut browser = Browser::default();
        browser.refresh(&app).await?;
        let response = browser
            .send(
                &app,
                Method::POST,
                "/login",
                Some(json!({"handle":"alice","password":"secret"})),
                false,
                &[],
            )
            .await?;
        response.assert_status(StatusCode::from_u16(419).map_err(Error::internal)?)?;
        let stats = app.app()?.state().resolve::<Stats>().await?;
        assert_eq!(stats.accounts.credentials.load(Ordering::SeqCst), 0);
        assert_eq!(stats.accounts.user.writes.load(Ordering::SeqCst), 0);
        assert_eq!(browser.refresh(&app).await?["authenticated"], false);
        let response = browser.login(&app).await?;
        response.assert_status(StatusCode::OK)?;
        assert_eq!(response.json::<Value>()?, json!({"two_factor":false}));
        browser.refresh(&app).await?;
        let response = browser
            .send(
                &app,
                Method::POST,
                "/user/two-factor-authentication",
                Some(json!({})),
                true,
                &[],
            )
            .await?;
        response.assert_status(StatusCode::OK)?;
        assert_eq!(response.body(), b"\"\"");
        assert!(
            stats
                .accounts
                .user
                .two_factor_state()
                .await?
                .secret
                .is_some()
        );
        let response = browser
            .send(
                &app,
                Method::GET,
                "/user/two-factor-secret-key",
                None,
                false,
                &[],
            )
            .await?;
        response.assert_status(StatusCode::OK)?;
        let body = response.json::<Value>()?;
        assert_eq!(body.as_object().ok_or_else(fixture_error)?.len(), 1);
        assert_eq!(
            body["secretKey"].as_str().ok_or_else(fixture_error)?.len(),
            16
        );
        let response = browser
            .send(&app, Method::POST, "/logout", Some(json!({})), true, &[])
            .await?;
        response.assert_status(StatusCode::NO_CONTENT)?;
        assert_eq!(response.body(), b"");
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    result?;
    shutdown
}

#[tokio::test]
async fn application_response_bindings_are_invoked_after_real_authentication_and_enable_action()
-> Result<()> {
    let mut app = TestApp::<App<2>>::builder(sources(2)).build().await?;
    let result: Result<()> = async {
        let mut browser = Browser::default();
        let response = browser.login(&app).await?;
        response.assert_status(StatusCode::ACCEPTED)?;
        assert_eq!(response.json::<Value>()?, json!({"binding":"login"}));
        assert_eq!(browser.refresh(&app).await?["authenticated"], true);
        let response = browser
            .send(
                &app,
                Method::POST,
                "/user/two-factor-authentication",
                Some(json!({})),
                true,
                &[],
            )
            .await?;
        response.assert_status(StatusCode::ACCEPTED)?;
        assert_eq!(response.json::<Value>()?, json!({"binding":"enabled"}));
        let stats = app.app()?.state().resolve::<Stats>().await?;
        assert_eq!(stats.login_responses.load(Ordering::SeqCst), 1);
        assert_eq!(stats.enabled_responses.load(Ordering::SeqCst), 1);
        assert_eq!(stats.accounts.user.writes.load(Ordering::SeqCst), 1);
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    result?;
    shutdown
}

#[tokio::test]
async fn application_guest_auth_and_password_confirmation_aliases_override_fortify_route_guards()
-> Result<()> {
    let mut app = TestApp::<App<3>>::builder(sources(3)).build().await?;
    let result: Result<()> = async {
        let mut browser = Browser::default();
        browser.refresh(&app).await?;
        for (name, path) in [
            ("guest", "/login"),
            ("auth", "/user/confirmed-password-status"),
            ("password.confirm", "/user/two-factor-secret-key"),
        ] {
            let response = browser
                .send(
                    &app,
                    Method::GET,
                    path,
                    None,
                    false,
                    &[("x-reject-alias", name)],
                )
                .await?;
            response.assert_status(StatusCode::IM_A_TEAPOT)?;
            assert_eq!(
                response
                    .headers()
                    .get("x-contract-alias")
                    .and_then(|value| value.to_str().ok()),
                Some(name)
            );
        }
        let stats = app.app()?.state().resolve::<Stats>().await?;
        let calls = stats.aliases.lock().map_err(|_| fixture_error())?;
        assert!(calls.contains(&("guest".into(), vec!["web".into()])));
        assert!(calls.contains(&("auth".into(), vec!["web".into()])));
        assert!(calls.contains(&("password.confirm".into(), vec![])));
        assert_eq!(stats.views.load(Ordering::SeqCst), 0);
        assert_eq!(stats.accounts.user.writes.load(Ordering::SeqCst), 0);
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    result?;
    shutdown
}

fn status(response: &TestResponse, expected: StatusCode, step: &str) {
    assert_eq!(response.status(), expected, "Fortify HTTP步骤：{step}");
}

#[tokio::test]
async fn management_password_confirmation_timeout_uses_shared_session_and_blocks_mutation()
-> Result<()> {
    let mut app = TestApp::<App<4>>::builder(sources(4)).build().await?;
    let result: Result<()> = async {
        let mut browser = Browser::default();
        let response = browser.login(&app).await?;
        status(&response, StatusCode::OK, "使用正确凭据登录");
        let response = browser
            .send(&app, Method::GET, "/fixture/session", None, false, &[])
            .await?;
        status(&response, StatusCode::OK, "登录后刷新Session/CSRF");
        browser.token = Some(
            response.json::<Value>()?["csrf"]
                .as_str()
                .ok_or_else(fixture_error)?
                .into(),
        );
        let stats = app.app()?.state().resolve::<Stats>().await?;
        let response = browser
            .send(
                &app,
                Method::POST,
                "/user/two-factor-authentication",
                Some(json!({})),
                true,
                &[],
            )
            .await?;
        status(&response, StatusCode::LOCKED, "未确认密码时拒绝首次启用");
        assert_eq!(stats.accounts.user.writes.load(Ordering::SeqCst), 0);
        let response = browser
            .send(
                &app,
                Method::POST,
                "/user/confirm-password",
                Some(json!({"password":"secret"})),
                true,
                &[],
            )
            .await?;
        status(&response, StatusCode::CREATED, "正确密码写入确认时间");
        assert_eq!(response.body(), b"\"\"");
        let response = browser
            .send(
                &app,
                Method::POST,
                "/user/two-factor-authentication",
                Some(json!({})),
                true,
                &[],
            )
            .await?;
        status(&response, StatusCode::OK, "确认后同Session启用");
        assert_eq!(stats.accounts.user.writes.load(Ordering::SeqCst), 1);
        for (seconds, expected) in [(10_860, StatusCode::LOCKED), (10_740, StatusCode::OK)] {
            let response = browser
                .send(
                    &app,
                    Method::GET,
                    &format!("/fixture/age/{seconds}"),
                    None,
                    false,
                    &[],
                )
                .await?;
            status(
                &response,
                StatusCode::OK,
                &format!("设置确认时间距今{seconds}秒"),
            );
            let before = stats.accounts.user.two_factor_state().await?;
            let count = stats.accounts.user.writes.load(Ordering::SeqCst);
            let response = browser
                .send(
                    &app,
                    Method::POST,
                    "/user/two-factor-authentication",
                    Some(json!({"force":true})),
                    true,
                    &[],
                )
                .await?;
            status(
                &response,
                expected,
                &format!("确认时间距今{seconds}秒时强制启用"),
            );
            if expected == StatusCode::LOCKED {
                assert_eq!(stats.accounts.user.two_factor_state().await?, before);
                assert_eq!(stats.accounts.user.writes.load(Ordering::SeqCst), count);
            } else {
                assert_eq!(stats.accounts.user.writes.load(Ordering::SeqCst), count + 1);
            }
        }
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    result?;
    shutdown
}

async fn assert_boot_password_confirmation_topology<const MODE: u8>(
    runtime_required: bool,
    expected: StatusCode,
) -> Result<()> {
    let mut app = TestApp::<App<MODE>>::builder(sources(MODE)).build().await?;
    let result: Result<()> = async {
        let mut browser = Browser::default();
        browser.login(&app).await?.assert_status(StatusCode::OK)?;
        browser.refresh(&app).await?;
        app.app()?.config_repository().set(
            "fortify-options.two-factor-authentication.confirmPassword",
            json!(runtime_required),
        )?;
        let stats = app.app()?.state().resolve::<Stats>().await?;
        let response = browser
            .send(
                &app,
                Method::POST,
                "/user/two-factor-authentication",
                Some(json!({})),
                true,
                &[],
            )
            .await?;
        response.assert_status(expected)?;
        if expected == StatusCode::LOCKED {
            assert_eq!(stats.accounts.user.writes.load(Ordering::SeqCst), 0);
            browser
                .send(
                    &app,
                    Method::POST,
                    "/user/confirm-password",
                    Some(json!({"password":"secret"})),
                    true,
                    &[],
                )
                .await?
                .assert_status(StatusCode::CREATED)?;
            browser
                .send(
                    &app,
                    Method::POST,
                    "/user/two-factor-authentication",
                    Some(json!({})),
                    true,
                    &[],
                )
                .await?
                .assert_status(StatusCode::OK)?;
        }
        assert_eq!(stats.accounts.user.writes.load(Ordering::SeqCst), 1);
        assert!(
            stats
                .accounts
                .user
                .factor
                .read()
                .map_err(|_| fixture_error())?
                .secret
                .is_some()
        );
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    result?;
    shutdown
}
#[tokio::test]
async fn password_confirmation_middleware_is_boot_topology_not_runtime_option() -> Result<()> {
    assert_boot_password_confirmation_topology::<4>(false, StatusCode::LOCKED).await?;
    assert_boot_password_confirmation_topology::<0>(true, StatusCode::OK).await
}

fn raw_option_sources(confirm: &Value, features: &Value) -> Figment {
    sources(0).merge(Serialized::globals(json!({
        "fortify":{"features":features},
        "fortify-options":{"two-factor-authentication":{
            "confirm":confirm,
            "confirmPassword":"true",
            "window":"not-an-integer",
            "secret-length":"32tail"
        }}
    })))
}
async fn assert_raw_options_reach_real_provider_and_actions(
    confirm: &Value,
    features: &Value,
) -> Result<()> {
    // build本身必须成功：只测Repo getter无法发现config_section::<TwoFactorOptions>的启动错误。
    let mut app = TestApp::<App<0>>::builder(raw_option_sources(confirm, features))
        .build()
        .await?;
    let outcome: Result<()> = async {
        let mut browser = Browser::default();
        browser.login(&app).await?.assert_status(StatusCode::OK)?;
        browser.refresh(&app).await?;
        let stats = app.app()?.state().resolve::<Stats>().await?;
        assert_eq!(stats.accounts.user.writes.load(Ordering::SeqCst), 0);
        // 字符串"true"不是bool true，boot不应附加password.confirm；不能先调用confirm-password掩盖问题。
        browser
            .send(
                &app,
                Method::POST,
                "/user/two-factor-authentication",
                Some(json!({})),
                true,
                &[],
            )
            .await?
            .assert_status(StatusCode::OK)?;
        assert_eq!(stats.accounts.user.writes.load(Ordering::SeqCst), 1);
        assert!(
            stats
                .accounts
                .user
                .factor
                .read()
                .map_err(|_| fixture_error())?
                .confirmed_at
                .is_none()
        );
        let response = browser
            .send(
                &app,
                Method::GET,
                "/user/two-factor-secret-key",
                None,
                false,
                &[],
            )
            .await?;
        response.assert_status(StatusCode::OK)?;
        let body = response.json::<Value>()?;
        let secret = body["secretKey"].as_str().ok_or_else(fixture_error)?;
        assert_eq!(secret.len(), 32);
        browser
            .send(&app, Method::POST, "/logout", Some(json!({})), true, &[])
            .await?
            .assert_status(StatusCode::NO_CONTENT)?;
        let pending = browser.login(&app).await?;
        pending.assert_status(StatusCode::OK)?;
        // confirm=1/"true"/null都不是strict true；Pending须真正挑战，不能被强制转成true跳过。
        assert_eq!(pending.json::<Value>()?["two_factor"], true);
        assert_eq!(browser.refresh(&app).await?["authenticated"], false);
        // window非int须保留默认1；完成一次真实Google校验，避免只证明未触发verify的启动成功。
        let decoded = crate::auth::fortify::engine::decode_secret(secret)?;
        let step = u64::try_from(chrono::Utc::now().timestamp()).map_err(Error::internal)? / 30;
        let code = crate::auth::totp::totp_code(&decoded, step, 6)?;
        browser
            .send(
                &app,
                Method::POST,
                "/two-factor-challenge",
                Some(json!({"code":code})),
                true,
                &[],
            )
            .await?
            .assert_status(StatusCode::NO_CONTENT)?;
        // 固定TwoFactorLoginResponse的JSON成功状态为204，管理动作仍为200。
        assert_eq!(browser.refresh(&app).await?["authenticated"], true);
        assert_eq!(stats.accounts.user.writes.load(Ordering::SeqCst), 1);
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    outcome?;
    shutdown
}
#[tokio::test]
async fn raw_nonboolean_options_survive_real_provider_registration_freeze_and_http_actions()
-> Result<()> {
    for confirm in [json!(1), json!("true"), Value::Null] {
        assert_raw_options_reach_real_provider_and_actions(
            &confirm,
            &json!(["two-factor-authentication"]),
        )
        .await?;
    }
    Ok(())
}
#[tokio::test]
async fn associative_feature_values_preserve_php_selection_through_actual_boot() -> Result<()> {
    assert_raw_options_reach_real_provider_and_actions(
        &json!(1),
        &json!({"optional-package-toggle":true}),
    )
    .await
}
#[tokio::test]
async fn nonarray_features_fail_explicitly_during_real_fortify_boot() -> Result<()> {
    for features in [Value::Null, json!("two-factor-authentication"), json!(true)] {
        match TestApp::<App<0>>::builder(raw_option_sources(&json!(1), &features))
            .build()
            .await
        {
            Err(error) => {
                assert_eq!(error.code(), "fortify_features_invalid");
                assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
            }
            Ok(mut app) => {
                app.shutdown(tokio::time::Instant::now() + TIMEOUT).await?;
                return Err(fixture_error());
            }
        }
    }
    Ok(())
}
