//! 真实请求Scope、Session、Cookie和公开UserProvider组合的具名认证回归。
#![cfg(feature = "auth")]

use openrust::auth::{
    AuthCredential, AuthDrivers, AuthLayer, AuthManager, AuthPrincipal, AuthServiceProvider,
    AuthUser, Authenticatable, Authenticator, Credentials, CurrentUser, EncodedPasswordHash,
    GenericUser, OptionalUser, UserProvider,
};
use openrust::config::{FrameworkConfig, NoConfig, SessionConfig};
use openrust::container::Scope;
use openrust::foundation::ApplicationDefinition;
use openrust::foundation::BootContext;
use openrust::http::CookieServiceProvider;
use openrust::http::HttpServiceProvider;
use openrust::http::Json;
use openrust::http::axum::{
    Extension,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use openrust::routing::RouteGroup;
use openrust::session::{MemorySessionStore, Session, SessionServiceProvider};
use openrust::support::DefaultProviders;
use openrust::support::ServiceProvider;
use openrust::testing::TestApp;
use openrust::{BoxFuture, Error, Result};
use serde_json::{Map, Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use subtle::ConstantTimeEq;

const ID: &str = "550e8400-e29b-41d4-a716-446655440000";

// 旧显式凭据路径不参与本测试；具名Guard必须走实际注册的UserProvider。
struct RejectLegacy;
impl Authenticator for RejectLegacy {
    fn authenticate<'a>(&'a self, _: AuthCredential<'a>) -> BoxFuture<'a, Result<AuthPrincipal>> {
        Box::pin(async { Err(Error::Unauthorized) })
    }
}

struct UsernameProvider {
    account: Arc<GenericUser>,
    passwords: std::sync::Arc<openrust::hashing::HashManager>,
}
impl UserProvider for UsernameProvider {
    fn retrieve_by_id<'a>(&'a self, id: &'a Value) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async move {
            Ok((id == &self.account.auth_identifier()?)
                .then(|| Arc::clone(&self.account) as AuthUser))
        })
    }
    fn retrieve_by_token<'a>(
        &'a self,
        id: &'a Value,
        token: &'a str,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async move {
            let user = self.retrieve_by_id(id).await?;
            let matches = self.account.remember_token()?.is_some_and(|stored| {
                !stored.is_empty()
                    && stored.len() == token.len()
                    && bool::from(stored.as_bytes().ct_eq(token.as_bytes()))
            });
            Ok(user.filter(|_| matches))
        })
    }
    fn update_remember_token<'a>(
        &'a self,
        _: &'a dyn Authenticatable,
        token: &'a str,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.account.set_remember_token(Some(token.into())).await })
    }
    fn retrieve_by_credentials<'a>(
        &'a self,
        credentials: &'a Credentials,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async move {
            Ok(
                (credentials.get("username") == self.account.attribute("username")?.as_ref())
                    .then(|| Arc::clone(&self.account) as AuthUser),
            )
        })
    }
    fn validate_credentials<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        credentials: &'a Credentials,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let Some(plain) = credentials.get("password").and_then(Value::as_str) else {
                return Ok(false);
            };
            let hash = user.auth_password()?.ok_or(Error::Unauthorized)?;
            EncodedPasswordHash::from_encoded(hash)
                .verify(self.passwords.as_ref(), plain)
                .await
        })
    }
    fn rehash_password_if_required<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        credentials: &'a Credentials,
        force: bool,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let encoded = EncodedPasswordHash::from_encoded(
                user.auth_password()?.ok_or(Error::Unauthorized)?,
            );
            if force || encoded.needs_rehash(self.passwords.as_ref())? {
                let plain = credentials
                    .get("password")
                    .and_then(Value::as_str)
                    .ok_or(Error::Unauthorized)?;
                let replacement = EncodedPasswordHash::make(self.passwords.as_ref(), plain).await?;
                self.account
                    .set_attribute("password", json!(replacement.as_str()))?;
            }
            Ok(())
        })
    }
}

struct NamedApp;
impl ApplicationDefinition for NamedApp {
    type Config = NoConfig;
    fn default_providers() -> DefaultProviders {
        DefaultProviders::empty()
    }
    fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers: Vec<Box<dyn ServiceProvider>> = vec![
            Box::new(HttpServiceProvider),
            Box::new(AuthServiceProvider::with_authenticator(RejectLegacy)),
            Box::new(
                SessionServiceProvider::with_store(MemorySessionStore::default())
                    .with_cookie_config(SessionConfig {
                        secure: Some(false),
                        cookie: Some("named_session".into()),
                        ..SessionConfig::default()
                    }),
            ),
        ];
        #[cfg(feature = "encryption")]
        {
            providers.push(Box::new(openrust::encryption::EncryptionServiceProvider));
            providers.push(Box::new(CookieServiceProvider::new()));
        }
        #[cfg(not(feature = "encryption"))]
        providers.push(Box::new(CookieServiceProvider::plain()));
        providers.push(Box::new(NamedRoutes));
        providers
    }
}

#[cfg(feature = "db-sqlite")]
struct NamedDatabaseApp;
#[cfg(feature = "db-sqlite")]
impl ApplicationDefinition for NamedDatabaseApp {
    type Config = NoConfig;
    fn default_providers() -> DefaultProviders {
        DefaultProviders::empty()
    }
    fn providers(config: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers = NamedApp::providers(config);
        providers[2] = Box::new(SessionServiceProvider::new());
        providers.insert(0, Box::new(openrust::database::DatabaseServiceProvider));
        providers
    }
}

struct NamedRoutes;
impl ServiceProvider for NamedRoutes {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let passwords = app.resolve::<openrust::hashing::HashManager>().await?;
            let mut providers = BTreeMap::new();
            for (realm, plain) in [("accounts", "web-secret"), ("other", "admin-secret")] {
                let encoded = EncodedPasswordHash::make(passwords.as_ref(), plain).await?;
                let account = GenericUser::new(Map::from_iter([
                    ("id".into(), json!(ID)),
                    ("username".into(), json!("username-only")),
                    ("password".into(), json!(encoded.as_str())),
                    ("remember_token".into(), Value::Null),
                ]));
                assert!(account.attribute("email")?.is_none());
                assert!(account.attribute("phone")?.is_none());
                providers.insert(
                    realm.to_owned(),
                    Arc::new(UsernameProvider {
                        account: Arc::new(account),
                        passwords: passwords.clone(),
                    }),
                );
            }
            let drivers = app.resolve::<AuthDrivers>().await?;
            drivers.provider("test-users", move |_, config| {
                let provider = config
                    .options
                    .get("realm")
                    .and_then(Value::as_str)
                    .and_then(|realm| providers.get(realm))
                    .cloned();
                Box::pin(async move {
                    Ok(provider.ok_or(Error::Unauthorized)? as Arc<dyn UserProvider>)
                })
            })?;
            app.exceptions(|handler| {
                handler.api_envelope_when(|_, _| true);
            })?;
            app.routes(|routes| {
                #[cfg(feature = "encryption")]
                let group = RouteGroup::new().middleware_group("web");
                // 无encryption时显式验证plain组合，不声称这是默认受保护web组。
                #[cfg(not(feature = "encryption"))]
                let group = RouteGroup::new()
                    .middleware_alias("cookies")
                    .middleware_alias("session");
                routes.group(group, |routes| {
                    routes
                        .group(RouteGroup::new(), |r| {
                            r.post("/login", login_web)
                                .post("/once", once_web)
                                .post("/remember", remember_web)
                                .get("/optional", optional_profile)
                                .post("/logout", logout_web)
                                .layer(AuthLayer::optional_guard("web"))
                        })
                        .group(RouteGroup::new(), |r| {
                            r.post("/admin-login", login_admin)
                                .layer(AuthLayer::optional_guard("admin"))
                        })
                        .group(RouteGroup::new(), |r| {
                            r.get("/profile", profile)
                                .get("/deadline", deadline)
                                .layer(AuthLayer::guard("web"))
                        })
                        .group(RouteGroup::new(), |r| {
                            r.get("/admin-profile", profile)
                                .layer(AuthLayer::guard("admin"))
                        })
                        .group(RouteGroup::new().middleware(AuthLayer::guard("web")), |r| {
                            r.group(
                                RouteGroup::new().middleware(AuthLayer::optional_guard("admin")),
                                |r| r.get("/nested-guest", nested_guest),
                            )
                        })
                        .get("/save", save_metadata)
                        .get("/automatic", || async { Json(json!({"automatic": true})) })
                        .get("/csrf", csrf)
                })
            })?;
            Ok(())
        })
    }
}

async fn csrf(session: Session) -> Result<Json<Value>> {
    if session.token()?.is_none() {
        session.regenerate_token()?;
    }
    Ok(Json(json!({"token": session.token()?})))
}
async fn profile(
    user: CurrentUser,
    Extension(scope): Extension<Arc<Scope>>,
) -> Result<Json<Value>> {
    let manager = scope.resolve::<AuthManager>().await?;
    let first = manager.guard(user.guard_name()).await?;
    let second = manager.guard(user.guard_name()).await?;
    Ok(Json(
        json!({"id": user.id(), "guard": user.guard_name(), "provider": user.provider_name(), "same_request": Arc::ptr_eq(&first, &second), "guard_id": first.id().await?}),
    ))
}
async fn deadline(
    user: CurrentUser,
    session: Session,
    Extension(scope): Extension<Arc<Scope>>,
) -> Result<Json<Value>> {
    let manager = scope.resolve::<AuthManager>().await?;
    let selected = manager.guard(user.guard_name()).await?;
    let store = scope
        .resolve::<openrust::auth::SessionStoreHandle>()
        .await?;
    let expires = store
        .authorized_until_guard(&session, selected.as_stateful().ok_or(Error::Unauthorized)?)
        .await?;
    Ok(Json(
        json!({"id": user.id(), "future": expires > std::time::SystemTime::now()}),
    ))
}

async fn save_metadata(
    session: Session,
    Extension(scope): Extension<Arc<Scope>>,
) -> Result<Json<Value>> {
    session.save().await?;
    let id = session.id()?;
    #[cfg(feature = "db-sqlite")]
    let user_id = persisted_user(
        scope.resolve::<openrust::database::Db>().await?.as_ref(),
        id.as_str(),
    )
    .await?;
    #[cfg(not(feature = "db-sqlite"))]
    let user_id: Option<String> = {
        let _ = scope;
        None
    };
    Ok(Json(json!({"session_id": id.as_str(), "user_id": user_id})))
}
async fn logout_web(Extension(scope): Extension<Arc<Scope>>) -> Result<Json<Value>> {
    let manager = scope.resolve::<AuthManager>().await?;
    let guard = manager.guard(Some("web")).await?;
    guard
        .as_stateful()
        .ok_or(Error::Unauthorized)?
        .logout()
        .await?;
    Ok(Json(json!({"id": guard.id().await?})))
}
async fn optional_profile(OptionalUser(user): OptionalUser) -> Json<Value> {
    Json(json!({"id": user.map(|user| user.id())}))
}
// 读取原始扩展，覆盖限流等中间件直接读取CurrentUser的同一观察面。
async fn nested_guest(OptionalUser(user): OptionalUser, request: Request<Body>) -> Json<Value> {
    Json(json!({
        "optional_guest": user.is_none(),
        "extension_present": request.extensions().get::<CurrentUser>().is_some(),
    }))
}
async fn sign_in(
    scope: Arc<Scope>,
    body: Value,
    name: &str,
    once: bool,
    remember: bool,
) -> Result<Json<Value>> {
    let manager = scope.resolve::<AuthManager>().await?;
    let guard = manager.guard(Some(name)).await?;
    let credentials = Credentials::new(body.as_object().cloned().ok_or(Error::Unauthorized)?);
    let stateful = guard.as_stateful().ok_or(Error::Unauthorized)?;
    let accepted = if once {
        stateful.once(&credentials).await?
    } else {
        stateful.attempt(&credentials, remember).await?
    };
    let other_name = if name == "web" { "admin" } else { "web" };
    Ok(Json(
        json!({"accepted": accepted, "id": guard.id().await?, "other_id": manager.guard(Some(other_name)).await?.id().await?, "via_remember": stateful.via_remember()?}),
    ))
}
async fn login_web(
    Extension(scope): Extension<Arc<Scope>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    sign_in(scope, body, "web", false, false).await
}
async fn login_admin(
    Extension(scope): Extension<Arc<Scope>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    sign_in(scope, body, "admin", false, false).await
}
async fn once_web(
    Extension(scope): Extension<Arc<Scope>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    sign_in(scope, body, "web", true, false).await
}
async fn remember_web(
    Extension(scope): Extension<Arc<Scope>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    sign_in(scope, body, "web", false, true).await
}

async fn fixture() -> Result<TestApp<NamedApp>> {
    fixture_with_provider_fallback(false).await
}

async fn fixture_with_provider_fallback(fallback: bool) -> Result<TestApp<NamedApp>> {
    TestApp::<NamedApp>::builder(named_sources(fallback))
        .build()
        .await
}

fn named_sources(fallback: bool) -> figment::Figment {
    let mut config = FrameworkConfig::default();
    config.api.http_status = openrust::config::HttpStatusMode::Semantic;
    figment::Figment::from(figment::providers::Serialized::defaults(config))
        .merge(figment::providers::Serialized::defaults(json!({
            "app": {"env": "test", "key": "named-guard-fixture-key"},
            "auth": {"defaults": {"guard": "web", "passwords": "accounts", "provider": if fallback { json!("accounts") } else { Value::Null }},

                "guards": {"web": {"driver": "session", "provider": if fallback { Value::Null } else { json!("accounts") }}, "admin": {"driver": "session", "provider": "other"}},
                "providers": {"accounts": {"driver": "test-users", "realm": "accounts"}, "other": {"driver": "test-users", "realm": "other"}}},
            "hashing": {"driver":"argon2id","argon":{"memory":19456,"time":1,"threads":1,"verify":true}},
            "security": {"encryption": {"active_key_id": "test", "keys": {"test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
        })))
}

#[derive(Default)]
struct Browser(BTreeMap<String, String>);
impl Browser {
    async fn request<A: ApplicationDefinition>(
        &mut self,
        app: &TestApp<A>,
        method: Method,
        path: &str,
        body: Option<Value>,
        csrf: Option<&str>,
    ) -> Result<(StatusCode, Value)> {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "localhost");
        if !self.0.is_empty() {
            request = request.header(
                "cookie",
                self.0
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            );
        }
        if let Some(token) = csrf {
            request = request.header("x-csrf-token", token);
        }
        let body = match body {
            Some(body) => {
                request = request.header("content-type", "application/json");
                Body::from(serde_json::to_vec(&body).map_err(Error::internal)?)
            }
            None => Body::empty(),
        };
        let response = app
            .raw_response(request.body(body).map_err(Error::internal)?)
            .await?;
        let status = response.status();
        for header in response.headers().get_all("set-cookie") {
            let value = header.to_str().map_err(Error::internal)?;
            if let Some((name, value)) = value
                .split(';')
                .next()
                .and_then(|pair| pair.split_once('='))
            {
                self.0.insert(name.to_owned(), value.to_owned());
            }
        }
        let bytes = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .map_err(Error::internal)?;
        Ok((
            status,
            serde_json::from_slice(&bytes).map_err(Error::internal)?,
        ))
    }
    async fn post<A: ApplicationDefinition>(
        &mut self,
        app: &TestApp<A>,
        path: &str,
        password: &str,
    ) -> Result<(StatusCode, Value)> {
        let (status, payload) = self.request(app, Method::GET, "/csrf", None, None).await?;
        assert_eq!(status, StatusCode::OK, "{payload}");
        let token = payload["data"]["token"]
            .as_str()
            .ok_or(Error::Unauthorized)?;
        self.request(
            app,
            Method::POST,
            path,
            Some(json!({"username": "username-only", "password": password})),
            Some(token),
        )
        .await
    }
}

#[tokio::test]
async fn username_only_uuid_login_reuses_cookie_but_isolates_guard_provider_and_request()
-> Result<()> {
    let mut app = fixture().await?;
    let result: Result<()> = async {
        let mut browser = Browser::default();
        let (status, _) = browser.request(&app, Method::GET, "/profile", None, None).await?;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (_, guest) = browser.request(&app, Method::GET, "/optional", None, None).await?;
        assert!(guest["data"]["id"].is_null());
        let (_, failed) = browser.post(&app, "/login", "admin-secret").await?;
        assert_eq!(failed["data"]["accepted"], false);
        let (status, login) = browser.post(&app, "/login", "web-secret").await?;
        assert_eq!(status, StatusCode::OK, "{login}");
        assert_eq!(login["data"]["accepted"], true);
        assert_eq!(login["data"]["id"], ID);
        assert!(login["data"]["other_id"].is_null());
        assert!(browser.0.contains_key("named_session"));
        let (status, user) = browser.request(&app, Method::GET, "/profile", None, None).await?;
        assert_eq!(status, StatusCode::OK, "{user}");
        assert_eq!(user["data"], json!({"id": ID, "guard": "web", "provider": "accounts", "same_request": true, "guard_id": ID}));
        let (status, _) = browser.request(&app, Method::GET, "/admin-profile", None, None).await?;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let mut stranger = Browser::default();
        assert_eq!(stranger.request(&app, Method::GET, "/profile", None, None).await?.0, StatusCode::UNAUTHORIZED);
        let (_, failed) = browser.post(&app, "/admin-login", "web-secret").await?;
        assert_eq!(failed["data"]["accepted"], false);
        let (_, admin_login) = browser.post(&app, "/admin-login", "admin-secret").await?;
        assert_eq!(admin_login["data"]["accepted"], true);
        let (_, admin) = browser.request(&app, Method::GET, "/admin-profile", None, None).await?;
        assert_eq!(admin["data"]["id"], ID);
        assert_eq!(admin["data"]["provider"], "other");
        assert_eq!(admin["data"]["guard"], "admin");
        let (_, web) = browser.request(&app, Method::GET, "/profile", None, None).await?;
        assert_eq!(web["data"]["provider"], "accounts");
        Ok(())
    }.await;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await?;
    result
}

#[tokio::test]
async fn undefined_guard_and_driver_configuration_propagates_http_500() -> Result<()> {
    let mut app = fixture().await?;
    let result: Result<()> = async {
        let config = app.app()?.config_repository();
        let guards = config.section::<Value>("auth.guards")?;
        let providers = config.section::<Value>("auth.providers")?;
        let scope = app.app()?.state().container().scope()?;
        let manager = scope.resolve::<AuthManager>().await?;
        let Err(error) = manager.guard(Some("web")).await else {
            return Err(Error::custom(
                "named_missing_request",
                "无请求上下文意外创建Session Guard",
            ));
        };
        assert_eq!(error.code(), "auth_request_missing");
        assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let mut browser = Browser::default();
        assert_eq!(
            browser
                .request(&app, Method::GET, "/profile", None, None)
                .await?
                .0,
            StatusCode::UNAUTHORIZED
        );
        for (guards, providers, code) in [
            (json!({}), providers.clone(), "auth_guard_undefined"),
            (
                json!({"web":{"driver":"unregistered","provider":"accounts"}}),
                providers.clone(),
                "auth_guard_driver_undefined",
            ),
            (
                json!({"web":{"driver":"session","provider":"broken"}}),
                json!({"broken":{"driver":"unregistered"}}),
                "auth_user_provider_driver_undefined",
            ),
            (
                json!({"web":{"driver":"session","provider":null}}),
                providers.clone(),
                "auth_guard_provider_missing",
            ),
            (
                json!({"web":{"driver":"session","provider":"missing"}}),
                providers.clone(),
                "auth_guard_provider_missing",
            ),
            (
                json!({"web":{"driver":"session","provider":"broken"}}),
                json!({"broken":{"driver":"database"}}),
                "auth_provider_table_missing",
            ),
            (
                json!({"web":{"driver":"session","provider":"broken"}}),
                json!({"broken":{"driver":"database","table":"accounts","connection":42}}),
                "auth_provider_connection_invalid",
            ),
            (
                json!({"web":{"driver":"session","provider":"accounts","remember":"invalid"}}),
                providers.clone(),
                "auth_remember_duration_invalid",
            ),
        ] {
            config.set("auth.guards", guards)?;
            config.set("auth.providers", providers)?;
            let (status, body) = browser
                .request(&app, Method::GET, "/profile", None, None)
                .await?;
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(body["code"], code);
        }
        config.set("auth.guards", guards)?;
        config.set("auth.providers", providers)?;
        assert_eq!(
            browser
                .request(&app, Method::GET, "/profile", None, None)
                .await?
                .0,
            StatusCode::UNAUTHORIZED
        );
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await?;
    result
}

#[tokio::test]
async fn once_authenticates_only_the_current_request() -> Result<()> {
    let mut app = fixture().await?;
    let result: Result<()> = async {
        let mut browser = Browser::default();
        let (status, once) = browser.post(&app, "/once", "web-secret").await?;
        assert_eq!(status, StatusCode::OK, "{once}");
        assert_eq!(once["data"]["accepted"], true);
        assert_eq!(once["data"]["id"], ID);
        assert_eq!(
            browser
                .request(&app, Method::GET, "/profile", None, None)
                .await?
                .0,
            StatusCode::UNAUTHORIZED
        );
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await?;
    result
}

#[cfg(feature = "encryption")]
#[tokio::test]
async fn protected_recaller_restores_only_its_named_guard_without_a_session_cookie() -> Result<()> {
    let mut app = fixture().await?;
    let result: Result<()> = async {
        let mut browser = Browser::default();
        let (status, login) = browser.post(&app, "/remember", "web-secret").await?;
        assert_eq!(status, StatusCode::OK, "{login}");
        assert_eq!(login["data"]["accepted"], true);
        browser
            .0
            .retain(|name, _| name.starts_with("remember_web_"));
        assert_eq!(browser.0.len(), 1);
        let (status, profile) = browser
            .request(&app, Method::GET, "/profile", None, None)
            .await?;
        assert_eq!(status, StatusCode::OK, "{profile}");
        assert_eq!(profile["data"]["id"], ID);
        assert_eq!(profile["data"]["provider"], "accounts");
        assert_eq!(
            browser
                .request(&app, Method::GET, "/admin-profile", None, None)
                .await?
                .0,
            StatusCode::UNAUTHORIZED
        );
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await?;
    result
}

#[tokio::test]
async fn profile_reports_the_provider_selected_from_auth_defaults() -> Result<()> {
    let mut app = fixture_with_provider_fallback(true).await?;
    let result: Result<()> = async {
        let mut browser = Browser::default();
        let (status, login) = browser.post(&app, "/login", "web-secret").await?;
        assert_eq!(status, StatusCode::OK, "{login}");
        assert_eq!(login["data"]["accepted"], true);
        let (status, profile) = browser
            .request(&app, Method::GET, "/profile", None, None)
            .await?;
        assert_eq!(status, StatusCode::OK, "{profile}");
        assert_eq!(profile["data"]["id"], ID);
        assert_eq!(profile["data"]["provider"], "accounts");
        assert_eq!(profile["data"]["guard"], "web");
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await?;
    result
}

#[tokio::test]
async fn nested_optional_guest_clears_the_outer_guards_current_user_extension() -> Result<()> {
    let mut app = fixture().await?;
    let result: Result<()> = async {
        let mut browser = Browser::default();
        // 外层必需web认证确实运行，未登录不能直接落入内层guest处理器。
        assert_eq!(
            browser
                .request(&app, Method::GET, "/nested-guest", None, None)
                .await?
                .0,
            StatusCode::UNAUTHORIZED
        );
        let (_, login) = browser.post(&app, "/login", "web-secret").await?;
        assert_eq!(login["data"]["accepted"], true);
        let (status, profile) = browser
            .request(&app, Method::GET, "/profile", None, None)
            .await?;
        assert_eq!(status, StatusCode::OK, "{profile}");
        assert_eq!(profile["data"]["guard"], "web");
        let (status, guest) = browser
            .request(&app, Method::GET, "/nested-guest", None, None)
            .await?;
        assert_eq!(status, StatusCode::OK, "{guest}");
        assert_eq!(
            guest["data"],
            json!({"optional_guest": true, "extension_present": false})
        );
        // 内层guest投影也不能撤销web的持久登录。
        assert_eq!(
            browser
                .request(&app, Method::GET, "/profile", None, None)
                .await?
                .0,
            StatusCode::OK
        );
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await?;
    result
}

#[cfg(feature = "db-sqlite")]
struct SessionMigrator;
#[cfg(feature = "db-sqlite")]
impl openrust::database::sea_orm_migration::MigratorTrait for SessionMigrator {
    fn migrations() -> Vec<Box<dyn openrust::database::sea_orm_migration::MigrationTrait>> {
        vec![Box::new(openrust::session::SessionMigration)]
    }
}

#[cfg(feature = "db-sqlite")]
async fn persisted_user(db: &openrust::database::Db, id: &str) -> Result<Option<String>> {
    use openrust::database::sea_orm::{ConnectionTrait, DbBackend, Statement};
    let row = db
        .connection()?
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT user_id FROM sessions WHERE id=?",
            [id.into()],
        ))
        .await
        .map_err(Error::internal)?
        .ok_or(Error::Unauthorized)?;
    row.try_get("", "user_id").map_err(Error::internal)
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn native_database_metadata_uses_actual_guard_on_manual_save_and_logout() -> Result<()> {
    let database = openrust::testing::TestDatabase::sqlite_file::<SessionMigrator>().await?;
    let sources = named_sources(false).merge(figment::providers::Serialized::defaults(json!({
        "database": {"url": database.url()},
        "session": {"driver": "database", "secure": false, "cookie": "named_session", "lottery": [0,100]}
    })));
    let mut app = TestApp::<NamedDatabaseApp>::builder(sources)
        .build()
        .await?;
    let result: Result<()> = async {
        let db = app
            .app()?
            .state()
            .container()
            .resolve::<openrust::database::Db>()
            .await?;
        let mut browser = Browser::default();
        let (status, login) = browser.post(&app, "/login", "web-secret").await?;
        assert_eq!(status, StatusCode::OK, "{login}");
        let (status, saved) = browser
            .request(&app, Method::GET, "/save", None, None)
            .await?;
        assert_eq!(status, StatusCode::OK, "{saved}");
        assert_eq!(
            saved["data"]["user_id"], ID,
            "手动保存必须已投影，不等待自动保存"
        );
        let id = saved["data"]["session_id"]
            .as_str()
            .ok_or(Error::Unauthorized)?;
        assert_eq!(persisted_user(&db, id).await?.as_deref(), Some(ID));
        #[cfg(feature = "encryption")]
        let recalled_id = {
            let (status, remembered) = browser.post(&app, "/remember", "web-secret").await?;
            assert_eq!(status, StatusCode::OK, "{remembered}");
            browser
                .0
                .retain(|name, _| name.starts_with("remember_web_"));
            assert_eq!(browser.0.len(), 1);
            let (status, automatic) = browser
                .request(&app, Method::GET, "/automatic", None, None)
                .await?;
            assert_eq!(status, StatusCode::OK, "{automatic}");
            // /automatic 不读取Guard或手动保存，只有自动save触发recall；出站Cookie须指向新ID。
            browser.0.retain(|name, _| name == "named_session");
            assert_eq!(browser.0.len(), 1, "必须只用最终Session Cookie验证恢复");
            let (status, profile) = browser
                .request(&app, Method::GET, "/profile", None, None)
                .await?;
            assert_eq!(status, StatusCode::OK, "{profile}");
            assert_eq!(profile["data"]["id"], ID);
            let (status, recalled) = browser
                .request(&app, Method::GET, "/save", None, None)
                .await?;
            assert_eq!(status, StatusCode::OK, "{recalled}");
            assert_eq!(
                recalled["data"]["user_id"], ID,
                "普通Web手动save须解析recaller且不死锁"
            );
            recalled["data"]["session_id"]
                .as_str()
                .ok_or(Error::Unauthorized)?
                .to_owned()
        };
        #[cfg(feature = "encryption")]
        let id = recalled_id.as_str();
        let (status, logged_out) = browser.post(&app, "/logout", "ignored").await?;
        assert_eq!(status, StatusCode::OK, "{logged_out}");
        assert!(logged_out["data"]["id"].is_null());
        assert_eq!(persisted_user(&db, id).await?, None);
        Ok(())
    }
    .await;
    let stopped = app
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await;
    let closed = database.close().await;
    result?;
    stopped?;
    closed
}

#[tokio::test]
async fn persistent_named_guard_exposes_the_actual_server_deadline_for_uuid_user() -> Result<()> {
    let mut app = fixture().await?;
    let result: Result<()> = async {
        let mut browser = Browser::default();
        assert_eq!(
            browser.post(&app, "/login", "web-secret").await?.0,
            StatusCode::OK
        );
        let (status, value) = browser
            .request(&app, Method::GET, "/deadline", None, None)
            .await?;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["data"]["id"], ID);
        assert_eq!(value["data"]["future"], true);
        Ok(())
    }
    .await;
    let stopped = app
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await;
    result?;
    stopped
}

#[cfg(feature = "db-sqlite")]
mod provider_connection_tests {
    use super::*;
    use figment::{Figment, providers::Serialized};
    use openrust::database::sea_orm::ConnectionTrait;
    use openrust::database::sea_orm_migration::{MigrationTrait, MigratorTrait};
    use openrust::database::{DatabaseServiceProvider, Databases, Db};
    use openrust::testing::TestDatabase;

    struct EmptyMigrator;
    impl MigratorTrait for EmptyMigrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            Vec::new()
        }
    }

    struct ConnectionApp;
    impl ApplicationDefinition for ConnectionApp {
        type Config = NoConfig;
        fn default_providers() -> DefaultProviders {
            DefaultProviders::empty()
        }
        fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
            vec![
                Box::new(HttpServiceProvider),
                Box::new(DatabaseServiceProvider),
                Box::new(AuthServiceProvider::with_authenticator(RejectLegacy)),
            ]
        }
    }

    fn ensure(condition: bool, message: &'static str) -> Result<()> {
        if condition {
            Ok(())
        } else {
            Err(Error::custom("named_connection_contract", message))
        }
    }

    async fn seed(db: &Db, marker: &str) -> Result<()> {
        let table = db.quoted_table_name("guard_accounts")?;
        db.connection()?
            .execute_unprepared(&format!(
                "CREATE TABLE {table} (id INTEGER PRIMARY KEY, password TEXT, remember_token TEXT)"
            ))
            .await
            .map_err(Error::internal)?;
        // 合成内容只用来识别真实返回行，不参与密码校验。
        db.table("guard_accounts")?
            .insert(vec![("id", 42.into()), ("password", marker.into())])
            .await?;
        Ok(())
    }

    async fn verify_selection(app: &TestApp<ConnectionApp>) -> Result<()> {
        let scope = app.app()?.state().container().scope()?;
        let default = scope.resolve::<Db>().await?;
        let databases = scope.resolve::<Databases>().await?;
        seed(&default, "from-default-database").await?;
        seed(&databases.get("secondary")?, "from-secondary-database").await?;
        let manager = scope.resolve::<AuthManager>().await?;
        for (name, expected) in [
            ("omitted", "from-default-database"),
            ("null", "from-default-database"),
            ("empty", "from-default-database"),
            ("named", "from-secondary-database"),
        ] {
            let provider = manager
                .create_user_provider(Some(name))
                .await?
                .ok_or(Error::Unauthorized)?;
            let user = provider
                .retrieve_by_id(&json!(42))
                .await?
                .ok_or(Error::Unauthorized)?;
            ensure(
                user.auth_identifier()? == json!(42),
                "Provider改变了数字主键",
            )?;
            ensure(
                user.auth_password()?.as_deref() == Some(expected),
                "Provider查询了错误的物理数据库",
            )?;
        }
        for (name, code) in [
            ("unknown", "database_instance_not_found"),
            ("invalid", "auth_provider_connection_invalid"),
        ] {
            let Err(error) = manager.create_user_provider(Some(name)).await else {
                return Err(Error::custom(
                    "named_connection_contract",
                    "非法来源静默回退默认库",
                ));
            };
            ensure(error.code() == code, "连接配置错误未按规定拒绝")?;
            ensure(
                error.status() == StatusCode::INTERNAL_SERVER_ERROR,
                "Provider配置故障被误映射为客户端错误",
            )?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn database_provider_selects_actual_named_connection_and_rejects_invalid_selection()
    -> Result<()> {
        let primary = TestDatabase::sqlite_file::<EmptyMigrator>().await?;
        let secondary = match TestDatabase::sqlite_file::<EmptyMigrator>().await {
            Ok(database) => database,
            Err(error) => {
                let closed = primary.close().await;
                closed?;
                return Err(error);
            }
        };
        let outcome: Result<()> = async {
            let source = Figment::from(Serialized::defaults(FrameworkConfig::default()))
                .merge(Serialized::defaults(json!({
                    "database": {
                        "url": primary.url(), "table_prefix": "", "min_connections": 0,
                        "instances": {"secondary": {
                            "url": secondary.url(), "table_prefix": "", "min_connections": 0
                        }}
                    },
                    "auth": {"providers": {
                        "omitted": {"driver":"database", "table":"guard_accounts"},
                        "null": {"driver":"database", "table":"guard_accounts", "connection":null},
                        "empty": {"driver":"database", "table":"guard_accounts", "connection":""},
                        "named": {"driver":"database", "table":"guard_accounts", "connection":"secondary"},
                        "unknown": {"driver":"database", "table":"guard_accounts", "connection":"missing"},
                        "invalid": {"driver":"database", "table":"guard_accounts", "connection":42}
                    }}
                })));
            let mut app = TestApp::<ConnectionApp>::builder(source).build().await?;
            // 返回Result而非panic，断言失败也必须等待App与两个自有文件库回收。
            let result = verify_selection(&app).await;
            let shutdown = app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5)).await;
            result?;
            shutdown
        }.await;
        let primary_closed = primary.close().await;
        let secondary_closed = secondary.close().await;
        outcome?;
        primary_closed?;
        secondary_closed
    }
}

#[cfg(feature = "encryption")]
#[tokio::test]
async fn named_session_and_recaller_reject_duplicate_wire_credentials_after_cookie_decryption()
-> Result<()> {
    let mut app = fixture().await?;
    let result: Result<()> = async {
        let mut browser = Browser::default();
        let (status, body) = browser.post(&app, "/remember", "web-secret").await?;
        assert_eq!(status, StatusCode::OK, "{body}");
        let session = browser.0.get("named_session").ok_or(Error::Unauthorized)?;
        let (recaller_name, recaller) = browser
            .0
            .iter()
            .find(|(name, _)| name.starts_with("remember_web_"))
            .ok_or(Error::Unauthorized)?;
        for wire in [
            format!("named_session={session}; named_session={session}"),
            format!("named_session={session}; named.session={session}"),
            format!("{recaller_name}={recaller}; {recaller_name}={recaller}"),
        ] {
            let response = app
                .raw_response(
                    Request::get("/profile")
                        .header("host", "localhost")
                        .header("cookie", wire)
                        .body(Body::empty())
                        .map_err(Error::internal)?,
                )
                .await?;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert!(
                !response
                    .headers()
                    .get_all("set-cookie")
                    .iter()
                    .any(|value| {
                        value
                            .to_str()
                            .is_ok_and(|value| value.starts_with("remember_"))
                    }),
                "歧义凭据不得签发记住我Cookie"
            );
            let body = to_bytes(response.into_body(), 4096)
                .await
                .map_err(Error::internal)?;
            let body: Value = serde_json::from_slice(&body).map_err(Error::internal)?;
            assert_eq!(body["code"], "ambiguous_credentials");
        }
        let response = app
            .raw_response(
                Request::get("/profile")
                    .header("host", "localhost")
                    .header(
                        "cookie",
                        format!("named_session={session}; ordinary=one; ordinary=two"),
                    )
                    .body(Body::empty())
                    .map_err(Error::internal)?,
            )
            .await?;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "普通非认证Cookie保留PHP重复名语义"
        );
        Ok(())
    }
    .await;
    let stopped = app
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await;
    result?;
    stopped
}
