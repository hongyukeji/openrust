//! 根应用组合账号与官方Fortify HTTP闭环；不替换账户Provider或Guard。
#![cfg(all(
    feature = "auth-fortify",
    feature = "encryption",
    any(feature = "db-sqlite", feature = "db-mysql", feature = "db-postgres")
))]

use openrust::auth::{AuthLayer, AuthManager, OptionalUser};
use openrust::container::Scope;
use openrust::database::{
    Db,
    sea_orm::{ConnectionTrait, Statement},
};
use openrust::foundation::ApplicationDefinition;
use openrust::foundation::BootContext;
use openrust::http::Json;
use openrust::http::axum::{
    Extension,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use openrust::routing::RouteGroup;
use openrust::session::Session;
use openrust::support::BoxFuture;
use openrust::support::ServiceProvider;
use openrust::testing::{TestApp, TestDatabase};
use openrust::{Error, Result};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

#[cfg(feature = "realtime")]
#[path = "fortify_socket.rs"]
mod native_socket;

const PASSWORD: &str = "fortify-real-provider-password";

fn note(phase: &'static str, error: &Error) {
    eprintln!(
        "Fortify fixture failure: stage={phase} code={} status={}",
        error.code(),
        error.status()
    );
}

struct FortifyMigrations;
impl openrust::database::sea_orm_migration::MigratorTrait for FortifyMigrations {
    fn migrations() -> Vec<Box<dyn openrust::database::sea_orm_migration::MigrationTrait>> {
        #[allow(unused_mut, reason = "模块实时业务迁移仅在其feature下进入本fixture")]
        let mut migrations = <crate::database::Migrator as openrust::database::sea_orm_migration::MigratorTrait>::migrations();
        #[cfg(feature = "realtime")]
        migrations.extend(<crate::modules::example::database::migrations::Migrator as openrust::database::sea_orm_migration::MigratorTrait>::migrations());
        migrations
    }
}

struct RootApplication;
impl ApplicationDefinition for RootApplication {
    type Config = crate::bootstrap::Configuration;
    fn config() -> openrust::config::ConfigManifest {
        crate::config::boot()
    }
    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers = crate::bootstrap::Server::providers(config);
        providers.push(Box::new(Observe));
        providers
    }
    fn configure_providers(
        config: &Self::Config,
        builder: &mut openrust::foundation::ProviderBuilder,
    ) {
        crate::bootstrap::Server::configure_providers(config, builder);
    }
}

// 普通端点观察真实Guard/Session；配置探针复用已认证账户与正式Actions，不替换生产绑定或签发证明。
struct Observe;
impl ServiceProvider for Observe {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let config = app.config_repository();
            app.routes(move |routes| {
                let config = config.clone();
                routes.group(
                    RouteGroup::new()
                        .middleware_group("web")
                        .middleware(AuthLayer::optional_guard("web")),
                    |routes| {
                        routes
                            .get("/__fortify-test/state", observe)
                            .get("/__fortify-test/csrf", csrf)
                            .get("/__fortify-test/protected", protected)
                            .get(
                                "/__fortify-test/dynamic-options",
                                move |Extension(scope): Extension<Arc<Scope>>| {
                                    let config = config.clone();
                                    async move { dynamic_options(scope, config).await }
                                },
                            )
                    },
                )
            })?;
            Ok(())
        })
    }
}
// 同一请求先解析scoped对象，再更改仓储；动作须在官方执行时点读取配置。
async fn dynamic_options(
    scope: Arc<Scope>,
    config: openrust::config::repository::ConfigRepository,
) -> Result<Json<Value>> {
    use openrust::auth::fortify::{Fortify, TwoFactorActions};
    let fortify = scope.resolve::<Fortify>().await?;
    let actions = scope.resolve::<TwoFactorActions>().await?;
    let user = fortify
        .guard()
        .await?
        .user()
        .await?
        .ok_or(Error::Unauthorized)?;
    let keys = [
        "fortify.features",
        "fortify-options.two-factor-authentication.confirm",
        "fortify-options.two-factor-authentication.secret-length",
    ];
    let saved = keys
        .iter()
        .map(|key| config.get(key)?.ok_or(Error::Unauthorized))
        .collect::<Result<Vec<_>>>()?;
    let outcome: Result<()> = async {
        config.set(keys[0], json!(["two-factor-authentication"]))?;
        config.set(keys[1], json!(true))?;
        config.set(keys[2], json!(32))?;
        assert!(fortify.two_factor_options()?.confirm);
        assert_eq!(fortify.two_factor_options()?.secret_length, 32);
        actions.enable(&user, true).await?;
        assert_eq!(actions.secret_key(&user).await?.len(), 32);
        assert!(
            user.as_two_factor()
                .ok_or(Error::Unauthorized)?
                .two_factor_state()
                .await?
                .confirmed_at
                .is_none()
        );
        config.set(keys[0], json!([]))?;
        assert!(!fortify.two_factor_options()?.confirm);
        config.set(keys[0], json!(["two-factor-authentication"]))?;
        config.set(keys[1], json!(false))?;
        config.set(keys[2], json!(16))?;
        assert!(!fortify.two_factor_options()?.confirm);
        assert_eq!(fortify.two_factor_options()?.secret_length, 16);
        actions.enable(&user, true).await?;
        assert_eq!(actions.secret_key(&user).await?.len(), 16);
        Ok(())
    }
    .await;
    let cleanup = actions.disable(&user).await;
    for (key, value) in keys.into_iter().zip(saved) {
        config.set(key, value)?;
    }
    outcome?;
    cleanup?;
    Ok(Json(json!({"observed":true})))
}

async fn csrf(session: Session) -> Result<Json<Value>> {
    Ok(Json(json!({"token":session.token()?})))
}
async fn protected(Extension(scope): Extension<Arc<Scope>>) -> Result<Json<Value>> {
    let manager = scope.resolve::<AuthManager>().await?;
    let guard = manager.guard(Some("web")).await?;
    let fortify = scope.resolve::<openrust::auth::fortify::Fortify>().await?;
    let confirm = fortify.confirms_two_factor_authentication()?;
    openrust::auth::fortify::authorize_session_factor(guard.as_ref(), "web", confirm).await?;
    Ok(Json(json!({"allowed":true})))
}
async fn observe(
    Extension(scope): Extension<Arc<Scope>>,
    OptionalUser(current): OptionalUser,
    session: Session,
) -> Result<Json<Value>> {
    let manager = scope.resolve::<AuthManager>().await?;
    let guard = manager.guard(Some("web")).await?;
    let user = guard.user().await?;
    let factor_verified = match guard.as_stateful() {
        Some(stateful) => stateful.session_factor_proof().await?.is_some(),
        None => false,
    };
    if let Some(model) = user.as_ref().and_then(|user| user.as_two_factor()) {
        let before = model.two_factor_state().await?;
        let denied = model
            .compare_and_set_two_factor_state(&before, &before)
            .await;
        assert_eq!(
            denied.err().ok_or(Error::Unauthorized)?.code(),
            "app_factor_authority_required"
        );
    }
    Ok(Json(json!({
        "id":user.map(|user| user.auth_identifier()).transpose()?,
        "pending":session.get_json("login.id")?,
        "guard":current.as_ref().and_then(|user| user.guard_name()), "provider":guard.provider_name(),
        "factor_verified":factor_verified,
    })))
}

struct Browser<'a> {
    app: &'a TestApp<RootApplication>,
    cookies: BTreeMap<String, String>,
    token: Option<String>,
}
struct Reply {
    status: StatusCode,
    headers: openrust::http::axum::http::HeaderMap,
    body: Vec<u8>,
}
impl Reply {
    fn json(&self) -> Result<Value> {
        serde_json::from_slice(&self.body)
            .map_err(Error::internal)
            .inspect_err(|error| note("response_json", error))
    }
}
impl<'a> Browser<'a> {
    fn new(app: &'a TestApp<RootApplication>) -> Self {
        Self {
            app,
            cookies: BTreeMap::new(),
            token: None,
        }
    }
    async fn send(
        &mut self,
        method: Method,
        path: &str,
        body: Option<Value>,
        accept: &str,
        csrf: bool,
    ) -> Result<Reply> {
        let mut request = Request::builder()
            .method(method)
            .extension(openrust::http::axum::extract::ConnectInfo(
                std::net::SocketAddr::from(([127, 0, 0, 1], 42000)),
            ))
            .uri(path)
            .header(header::HOST, "localhost")
            .header(header::ACCEPT, accept)
            .header(header::REFERER, "http://localhost/settings");
        if path.starts_with("/api/example/account/") {
            let url =
                url::Url::parse(&self.app.app()?.config().app.url).map_err(Error::internal)?;
            request = request
                .header(header::ORIGIN, url.origin().ascii_serialization())
                .header("x-example-request", "same-origin");
        }
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
        if csrf && let Some(token) = &self.token {
            request = request.header("x-csrf-token", token);
        }
        let body = match body {
            Some(value) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(value.to_string())
            }
            None => Body::empty(),
        };
        let response: Response = self
            .app
            .raw_response(request.body(body).map_err(Error::internal)?)
            .await?;
        let status = response.status();
        let headers = response.headers().clone();
        for cookie in headers.get_all(header::SET_COOKIE) {
            let cookie = cookie.to_str().map_err(Error::internal)?;
            let Some((name, value)) = cookie
                .split(';')
                .next()
                .and_then(|cookie| cookie.split_once('='))
            else {
                continue;
            };
            if value.is_empty() || cookie.to_ascii_lowercase().contains("max-age=0") {
                self.cookies.remove(name);
            } else {
                self.cookies.insert(name.into(), value.into());
            }
        }
        Ok(Reply {
            status,
            headers,
            body: to_bytes(response.into_body(), 1024 * 1024)
                .await
                .map_err(Error::internal)?
                .to_vec(),
        })
    }
    async fn get(&mut self, path: &str) -> Result<Reply> {
        self.send(Method::GET, path, None, "application/json", true)
            .await
    }
    async fn post(&mut self, path: &str, body: Value) -> Result<Reply> {
        self.send(Method::POST, path, Some(body), "application/json", true)
            .await
    }
    async fn refresh_csrf(&mut self) -> Result<()> {
        let response = self.get("/__fortify-test/csrf").await?;
        assert_eq!(response.status, StatusCode::OK);
        self.token = Some({
            let envelope = response.json()?;
            assert_eq!(envelope["success"], json!(true));
            envelope["data"]["token"]
                .as_str()
                .ok_or(Error::Unauthorized)
                .inspect_err(|error| note("csrf_envelope_token", error))?
                .to_owned()
        });
        Ok(())
    }
    async fn state(&mut self) -> Result<Value> {
        let response = self.get("/__fortify-test/state").await?;
        assert_eq!(response.status, StatusCode::OK);
        let envelope = response.json()?;
        assert_eq!(envelope["success"], json!(true));
        let data = envelope
            .get("data")
            .cloned()
            .ok_or(Error::Unauthorized)
            .inspect_err(|error| note("state_envelope_data", error))?;
        assert!(data.is_object());
        Ok(data)
    }
    async fn logout(&mut self) -> Result<()> {
        self.refresh_csrf().await?;
        assert_eq!(
            self.post("/logout", json!({})).await?.status,
            StatusCode::NO_CONTENT
        );
        self.refresh_csrf().await?;
        Ok(())
    }
    fn has_recaller(&self) -> bool {
        self.cookies
            .keys()
            .any(|name| name.starts_with("remember_"))
    }
}

fn sources(root: &std::path::Path, url: &str) -> Result<openrust::config::ConfigSources> {
    // 先完整求值真实代码配置，再覆写测试所需叶子；不以不完整父段替代Frozen默认。
    // 保留app.name/url，JWT和模块配置继续使用同次代码求值的依赖结果。
    // 与既有成功SQLite根宿主fixture一致：显式文件层完整替换redis条目，
    // 避免未选store仍被校验或将原dsn遗留给memory driver。文件仅在自有临时根。
    let config_directory = root.join("config");
    std::fs::create_dir_all(&config_directory).map_err(Error::internal)?;
    std::fs::write(config_directory.join("cache.toml"),
        "default='test'\n[stores.test]\ndriver='memory'\nnamespace='fortify-http'\n[stores.redis]\ndriver='memory'\nnamespace='fortify-http-redis'\n",
    ).map_err(Error::internal)?;
    let sources = openrust::config::ConfigLoader::application_code_sources(
        root,
        "FORTIFY_HTTP_TEST_",
        Some("local"),
        &crate::config::boot().with_toml(),
    )
    .map_err(|_| {
        Error::custom(
            "fortify_fixture_code_sources",
            "根代码配置求值失败（源值未公开）",
        )
    })?;
    let sources = sources.merge_ordered_entries([
        ("application_root", json!(root)),
        ("modules", json!({"activator":"registered","activators":{"file":{"statuses-file":root.join("module-statuses.json")}}})),
        ("app", json!({"env":"local","url":"http://localhost","key":"0123456789abcdef0123456789abcdef"})),
        ("database", json!({"url":url,"max_connections":4,"min_connections":0})),
        ("session", json!({"driver":"database","cookie":"fortify_session","secure":false,"lottery":[0,100]})),
        ("auth", json!({"jwt":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}})),
        ("security", json!({"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}},"signed_links":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}})),
        ("fortify", json!({"views":false})),
        #[cfg(feature = "realtime")]
        ("realtime", json!({"allowed_origins":["http://localhost"]})),
    ]).map_err(|_| Error::custom("fortify_fixture_leaf_overrides", "测试叶子覆盖失败（源值未公开）"))?;
    let fortify: openrust::auth::fortify::FortifyConfig = sources
        .figment()
        .extract_inner("fortify")
        .map_err(|_| Error::custom("fortify_fixture_fortify_config", "根Fortify配置不完整"))?;
    assert_eq!(fortify.guard, "web");
    assert_eq!(fortify.username, "account");
    assert!(!fortify.views);
    let driver: String = sources
        .figment()
        .extract_inner("auth.providers.users.driver")
        .map_err(|_| Error::custom("fortify_fixture_provider_config", "根账户Provider配置缺失"))?;
    assert_eq!(driver, "app-users");
    let cache: openrust::config::CacheConfig = sources
        .figment()
        .extract_inner("cache")
        .map_err(|_| Error::custom("fortify_fixture_cache_config", "测试缓存配置不完整"))?;
    assert_eq!(cache.default, "test");
    for name in ["test", "redis"] {
        let store = cache
            .stores
            .get(name)
            .ok_or(Error::Unauthorized)
            .inspect_err(|error| note("cache_store_presence", error))?;
        assert_eq!(store.driver, "memory");
        assert!(store.options.is_empty());
    }
    Ok(sources)
}

async fn seed(app: &openrust::Application) -> Result<()> {
    let passwords = app
        .state()
        .resolve::<openrust::hashing::HashManager>()
        .await
        .inspect_err(|error| note("seed_passwords_resolve", error))?;
    let db = app
        .state()
        .resolve::<Db>()
        .await
        .inspect_err(|error| note("seed_database_resolve", error))?;
    let encoded = openrust::auth::EncodedPasswordHash::make(passwords.as_ref(), PASSWORD)
        .await
        .inspect_err(|error| note("seed_password_hash", error))?;
    for (id, username, email, phone) in [
        (41, Some("local-only"), None, None),
        (
            42,
            Some("Member"),
            Some("Member@example.test"),
            Some("+8613800000000"),
        ),
        (43, Some("collision@example.test"), None, None),
        (44, Some("other"), Some("collision@example.test"), None),
    ] {
        db.table("users")?
            .insert(vec![
                ("id", id.into()),
                ("username", username.map(str::to_owned).into()),
                ("email", email.map(str::to_owned).into()),
                ("phone", phone.map(str::to_owned).into()),
                ("password_hash", encoded.as_str().into()),
                ("created_at", chrono::Utc::now().into()),
                ("updated_at", chrono::Utc::now().into()),
            ])
            .await
            .inspect_err(|error| note("seed_account_insert", error))?;
    }
    Ok(())
}

// 独立RFC4226计算仅生成测试客户端验证码，不替换服务端TOTP Provider。
fn code(secret: &str) -> Result<String> {
    code_with_offset(secret, 0)
}

pub(super) fn code_with_offset(secret: &str, offset: u64) -> Result<String> {
    use hmac::{Hmac, KeyInit, Mac};
    let mut bytes = Vec::new();
    let mut buffer = 0_u32;
    let mut bits = 0_u32;
    for value in secret.bytes() {
        let value = match value {
            b'A'..=b'Z' => value - b'A',
            b'2'..=b'7' => value - b'2' + 26,
            _ => return Err(Error::Unauthorized),
        };
        buffer = (buffer << 5) | u32::from(value);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            bytes.push(u8::try_from((buffer >> bits) & 255).map_err(Error::internal)?);
            buffer &= (1 << bits) - 1;
        }
    }
    let step = u64::try_from(chrono::Utc::now().timestamp()).map_err(Error::internal)? / 30;
    let step = step.checked_add(offset).ok_or(Error::Unauthorized)?;
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(&bytes).map_err(Error::internal)?;
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[19] & 15);
    let value = (u32::from(digest[offset] & 127) << 24)
        | (u32::from(digest[offset + 1]) << 16)
        | (u32::from(digest[offset + 2]) << 8)
        | u32::from(digest[offset + 3]);
    Ok(format!("{:06}", value % 1_000_000))
}

// 使用根应用真实登记，核单例Provider跨Scope、scoped Actions与共享配置即时读取。
async fn provider_lifecycle_and_dynamic_window(app: &openrust::Application) -> Result<()> {
    use openrust::auth::fortify::{TwoFactorActions, TwoFactorAuthenticationProvider};
    let config = app.config_repository();
    let key = "fortify-options.two-factor-authentication.window";
    let original = config.get(key)?.ok_or(Error::Unauthorized)?;
    let reset_window = original.as_i64().unwrap_or(1);
    config.set(key, json!(0))?;
    let first_scope = app.state().container().scope()?;
    let second_scope = app.state().container().scope()?;
    let first = first_scope
        .resolve::<dyn TwoFactorAuthenticationProvider>()
        .await?;
    let second = second_scope
        .resolve::<dyn TwoFactorAuthenticationProvider>()
        .await?;
    let first_actions = first_scope.resolve::<TwoFactorActions>().await?;
    let outcome: Result<()> = async {
        assert!(Arc::ptr_eq(&first, &second));
        config.set(key, json!(3))?;
        let second_actions = second_scope.resolve::<TwoFactorActions>().await?;
        assert!(!Arc::ptr_eq(&first_actions, &second_actions));
        let secret = "JBSWY3DPEHPK3PXP";
        let candidate = code_with_offset(secret, 3)?;
        let fact = first.verify_with_context(secret, &candidate).await?;
        assert!(fact.is_valid());
        assert_eq!(fact.window(), Some(3));
        // null不会重置已经存在的引擎；换Scope亦仍引用同一Provider。
        config.set(key, Value::Null)?;
        let third_scope = app.state().container().scope()?;
        let third = third_scope
            .resolve::<dyn TwoFactorAuthenticationProvider>()
            .await?;
        assert!(Arc::ptr_eq(&first, &third));
        Ok(())
    }
    .await;
    // 用非法code触发配置读取但不消费验证码，恢复引擎与配置后才运行真实账户流程。
    config.set(key, json!(reset_window))?;
    let restored = first
        .verify_with_context("JBSWY3DPEHPK3PXP", "window-reset")
        .await;
    config.set(key, original)?;
    outcome?;
    assert!(!restored?.is_valid());
    Ok(())
}

async fn login(browser: &mut Browser<'_>, account: &str, remember: bool) -> Result<Reply> {
    let reply = browser
        .post(
            "/login",
            json!({"account":account,"password":PASSWORD,"remember":remember}),
        )
        .await?;
    if reply.status == StatusCode::OK {
        // 登录轮换Session及CSRF；客户端必须取得新token再提交后续表单。
        browser.refresh_csrf().await?;
    }
    Ok(reply)
}

async fn bearer_status(
    fixture: &TestApp<RootApplication>,
    path: &str,
    token: &str,
) -> Result<StatusCode> {
    let response = fixture
        .raw_response(
            Request::builder()
                .uri(path)
                .header(header::HOST, "localhost")
                .header(header::ACCEPT, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .map_err(Error::internal)?,
        )
        .await?;
    Ok(response.status())
}

async fn assert_read_token_deadline(fixture: &TestApp<RootApplication>) -> Result<()> {
    let tokens = fixture
        .app()?
        .state()
        .resolve::<openrust::auth::PersonalAccessTokens>()
        .await?;
    let lifetime = fixture
        .app()?
        .section::<openrust::config::SessionConfig>("session")?
        .lifetime;
    let bound = chrono::Utc::now()
        + chrono::Duration::minutes(i64::try_from(lifetime).map_err(Error::internal)?);
    let metadata = tokens.list_for_user(41).await?;
    let issued = metadata
        .iter()
        .find(|token| token.name == "example-read")
        .ok_or(Error::Unauthorized)?;
    assert!(issued.expires_at.is_some_and(|expiry| expiry <= bound));
    Ok(())
}

async fn exercise_managed_credentials(
    browser: &mut Browser<'_>,
    recovery_code: &str,
) -> Result<()> {
    let fixture = browser.app;
    let signed = browser
        .post("/api/example/account/session/credentials", json!({}))
        .await?;
    assert_eq!(signed.status, StatusCode::OK);
    let envelope = signed.json()?;
    let credentials = envelope.get("data").ok_or(Error::Unauthorized)?;
    let access = credentials["access_token"]
        .as_str()
        .ok_or(Error::Unauthorized)?;
    let refresh = credentials["refresh_token"]
        .as_str()
        .ok_or(Error::Unauthorized)?;
    assert_eq!(
        bearer_status(fixture, "/api/example/me", access).await?,
        StatusCode::OK
    );
    let bare = fixture
        .app()?
        .state()
        .resolve::<openrust::auth::JwtTokens>()
        .await?
        .issue_access_token(41)?;
    assert_eq!(
        bearer_status(fixture, "/api/example/me", &bare).await?,
        StatusCode::UNAUTHORIZED
    );
    let personal = browser
        .post("/api/example/account/session/read-token", json!({}))
        .await?;
    assert_eq!(personal.status, StatusCode::OK);
    assert_read_token_deadline(fixture).await?;
    let personal = personal.json()?;
    let token = personal["data"]["token"]
        .as_str()
        .ok_or(Error::Unauthorized)?;
    assert_eq!(
        bearer_status(fixture, "/api/example/me/pat", token).await?,
        StatusCode::OK
    );
    let rotated = browser
        .post(
            "/api/example/auth/refresh",
            json!({"refresh_token":refresh}),
        )
        .await?;
    assert_eq!(
        rotated.status,
        StatusCode::OK,
        "refresh error code: {:?}",
        rotated.json()?.get("code")
    );
    let rotated = rotated.json()?;
    let next_access = rotated["data"]["access_token"]
        .as_str()
        .ok_or(Error::Unauthorized)?;
    assert_eq!(
        bearer_status(fixture, "/api/example/me", next_access).await?,
        StatusCode::OK
    );
    assert_eq!(
        bearer_status(fixture, "/api/example/me", access).await?,
        StatusCode::OK
    );
    assert_eq!(
        browser
            .post(
                "/api/example/auth/refresh",
                json!({"refresh_token":refresh})
            )
            .await?
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        bearer_status(fixture, "/api/example/me", access).await?,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        bearer_status(fixture, "/api/example/me", next_access).await?,
        StatusCode::UNAUTHORIZED
    );
    // 家族撤销不擅自撤销独立PAT；该PAT仍受自身撤销及当前factor代际约束。
    assert_eq!(
        bearer_status(fixture, "/api/example/me/pat", token).await?,
        StatusCode::OK
    );
    exercise_direct_credentials(browser, recovery_code).await
}

async fn exercise_direct_credentials(browser: &mut Browser<'_>, recovery_code: &str) -> Result<()> {
    let fixture = browser.app;
    // 既有直接账户增强保留显式email域；这里验证它真正消费因子后才签发凭据。
    let db = fixture.app()?.state().resolve::<Db>().await?;
    db.table("users")?
        .where_eq("id", 41_i64)?
        .update(vec![
            ("email", "direct-41@example.test".into()),
            ("email_verified_at", chrono::Utc::now().into()),
        ])
        .await?;
    let direct = browser
        .post(
            "/api/example/account/login",
            json!({"email":"direct-41@example.test","password":PASSWORD}),
        )
        .await?;
    assert_eq!(direct.status, StatusCode::OK);
    let direct = direct.json()?;
    let challenge = direct["data"]["challenge"]
        .as_str()
        .ok_or(Error::Unauthorized)?;
    assert_eq!(direct["data"]["status"], "two_factor_required");
    assert!(direct["data"].get("credentials").is_none());
    let completed = browser
        .post(
            "/api/example/account/login/two-factor",
            json!({"challenge":challenge,"recovery_code":recovery_code}),
        )
        .await?;
    assert_eq!(completed.status, StatusCode::OK);
    let completed = completed.json()?;
    let direct_token = completed["data"]["access_token"]
        .as_str()
        .ok_or(Error::Unauthorized)?;
    assert_eq!(
        bearer_status(fixture, "/api/example/me", direct_token).await?,
        StatusCode::OK
    );
    assert_eq!(
        browser
            .post(
                "/api/example/account/login/two-factor",
                json!({"challenge":challenge,"recovery_code":recovery_code})
            )
            .await?
            .status,
        StatusCode::UNAUTHORIZED
    );
    #[cfg(feature = "realtime")]
    crate::modules::example::managed_realtime::exercise(
        fixture,
        41,
        direct_token,
        completed["data"]["refresh_token"]
            .as_str()
            .ok_or(Error::Unauthorized)?,
    )
    .await?;
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "同一真实浏览器流程验证密码确认、绑定、退出、挑战和Session过期的连续状态"
)]
async fn exercise(fixture: &TestApp<RootApplication>) -> Result<()> {
    let app = fixture.app()?;
    for account in ["Member", "Member@example.test", "+8613800000000"] {
        let mut browser = Browser::new(fixture);
        browser.refresh_csrf().await?;
        let response = login(&mut browser, account, false).await?;
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.json()?, json!({"two_factor":false}));
        let state = browser.state().await?;
        assert_eq!(state["id"], json!(42));
        assert_eq!(state["guard"], "web");
        assert_eq!(state["provider"], "users");
        browser.logout().await?;
    }
    let mut ambiguous = Browser::new(fixture);
    ambiguous.refresh_csrf().await?;
    assert_eq!(
        login(&mut ambiguous, "collision@example.test", true)
            .await?
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(ambiguous.state().await?["id"], Value::Null);
    assert!(!ambiguous.has_recaller());

    let mut browser = Browser::new(fixture);
    browser.refresh_csrf().await?;
    let csrf_failure = browser
        .send(
            Method::POST,
            "/login",
            Some(json!({"account":"local-only","password":PASSWORD})),
            "application/json",
            false,
        )
        .await?;
    assert_eq!(csrf_failure.status.as_u16(), 419);
    assert_eq!(browser.state().await?["id"], Value::Null);
    assert_eq!(
        browser
            .post("/login", json!({"account":"local-only","password":"wrong"}))
            .await?
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(browser.state().await?["id"], Value::Null);
    assert_eq!(
        login(&mut browser, "local-only", false).await?.json()?,
        json!({"two_factor":false})
    );
    assert_eq!(browser.state().await?["id"], json!(41));
    assert_eq!(
        browser.get("/__fortify-test/protected").await?.status,
        StatusCode::OK
    );
    let mut old_password_session = Browser::new(fixture);
    old_password_session.refresh_csrf().await?;
    assert_eq!(
        login(&mut old_password_session, "local-only", false)
            .await?
            .status,
        StatusCode::OK
    );
    assert_eq!(
        old_password_session
            .post("/user/confirm-password", json!({"password":PASSWORD}))
            .await?
            .status,
        StatusCode::CREATED
    );
    assert_eq!(
        browser
            .post("/user/two-factor-authentication", json!({}))
            .await?
            .status,
        StatusCode::LOCKED
    );
    assert_eq!(
        browser
            .post("/user/confirm-password", json!({"password":"wrong"}))
            .await?
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        browser
            .get("/user/confirmed-password-status")
            .await?
            .json()?,
        json!({"confirmed":false})
    );
    assert_eq!(
        browser
            .post("/user/confirm-password", json!({"password":PASSWORD}))
            .await?
            .status,
        StatusCode::CREATED
    );
    assert_eq!(
        browser
            .get("/user/confirmed-password-status")
            .await?
            .json()?,
        json!({"confirmed":true})
    );
    let enabled = browser
        .send(
            Method::POST,
            "/user/two-factor-authentication",
            Some(json!({})),
            "text/html",
            true,
        )
        .await?;
    assert_eq!(enabled.status, StatusCode::FOUND);
    assert!(
        enabled.headers[header::LOCATION]
            .to_str()
            .map_err(Error::internal)?
            .ends_with("/settings")
    );
    let secret_reply = browser.get("/user/two-factor-secret-key").await?;
    assert_eq!(secret_reply.status, StatusCode::OK);
    let secret = secret_reply.json()?["secretKey"]
        .as_str()
        .ok_or(Error::Unauthorized)?
        .to_owned();
    let codes_reply = browser.get("/user/two-factor-recovery-codes").await?;
    assert_eq!(codes_reply.status, StatusCode::OK);
    let codes: Vec<String> =
        serde_json::from_value(codes_reply.json()?).map_err(Error::internal)?;
    assert_eq!(codes.len(), 8);
    let qr = browser.get("/user/two-factor-qr-code").await?.json()?;
    assert!(
        qr["svg"]
            .as_str()
            .ok_or(Error::Unauthorized)?
            .starts_with("<svg ")
    );
    assert!(
        qr["url"]
            .as_str()
            .ok_or(Error::Unauthorized)?
            .starts_with("otpauth://totp/")
    );
    assert_eq!(
        browser
            .post(
                "/user/confirmed-two-factor-authentication",
                json!({"code":code(&secret)?})
            )
            .await?
            .status,
        StatusCode::OK
    );
    exercise_managed_credentials(&mut browser, &codes[7]).await?;
    browser.logout().await?;
    assert!(!browser.has_recaller());
    let challenge = login(&mut browser, "local-only", true).await?;
    assert_eq!(challenge.status, StatusCode::OK);
    assert_eq!(challenge.json()?, json!({"two_factor":true}));
    let state = browser.state().await?;
    assert_eq!(state["id"], Value::Null);
    assert_eq!(state["pending"], json!(41));
    assert_eq!(
        browser.get("/__fortify-test/protected").await?.status,
        StatusCode::UNAUTHORIZED
    );
    assert!(!browser.has_recaller());
    assert_eq!(
        browser
            .post("/two-factor-challenge", json!({"code":"invalid"}))
            .await?
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(browser.state().await?["id"], Value::Null);
    assert!(!browser.has_recaller());
    assert_eq!(
        browser
            .post("/two-factor-challenge", json!({"recovery_code":"invalid"}))
            .await?
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(browser.state().await?["id"], Value::Null);
    assert_eq!(
        browser
            .post("/two-factor-challenge", json!({"recovery_code":codes[0]}))
            .await?
            .status,
        StatusCode::NO_CONTENT
    );
    let state = browser.state().await?;
    assert_eq!(state["id"], json!(41));
    assert_eq!(state["pending"], Value::Null);
    assert_eq!(state["factor_verified"], true);
    assert_eq!(
        browser.get("/__fortify-test/protected").await?.status,
        StatusCode::OK
    );
    assert!(browser.has_recaller());
    browser.refresh_csrf().await?;
    assert_eq!(
        browser
            .post("/user/confirm-password", json!({"password":PASSWORD}))
            .await?
            .status,
        StatusCode::CREATED
    );
    assert_eq!(
        browser
            .post("/user/two-factor-recovery-codes", json!({}))
            .await?
            .status,
        StatusCode::OK
    );
    assert_eq!(
        browser
            .get("/user/two-factor-recovery-codes")
            .await?
            .json()?
            .as_array()
            .ok_or(Error::Unauthorized)?
            .len(),
        8
    );
    assert_eq!(
        browser.get("/__fortify-test/protected").await?.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        browser
            .post("/user/two-factor-authentication", json!({"force":true}))
            .await?
            .status,
        StatusCode::OK
    );
    let replacement = browser.get("/user/two-factor-secret-key").await?;
    assert_eq!(replacement.status, StatusCode::OK);
    let replacement_secret = replacement.json()?["secretKey"]
        .as_str()
        .ok_or(Error::Unauthorized)?
        .to_owned();
    assert_ne!(replacement_secret, secret);
    assert!(
        browser.get("/user/two-factor-qr-code").await?.json()?["svg"]
            .as_str()
            .is_some()
    );
    assert_eq!(
        old_password_session
            .get("/user/two-factor-secret-key")
            .await?
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        browser
            .post(
                "/user/confirmed-two-factor-authentication",
                json!({"code":code(&replacement_secret)?})
            )
            .await?
            .status,
        StatusCode::OK
    );
    assert_eq!(
        browser.get("/__fortify-test/protected").await?.status,
        StatusCode::OK
    );
    browser.logout().await?;

    let mut expired = Browser::new(fixture);
    expired.refresh_csrf().await?;
    assert_eq!(
        login(&mut expired, "local-only", false).await?.json()?,
        json!({"two_factor":true})
    );
    let db = app.state().resolve::<Db>().await?;
    let table = db.quoted_table_name("sessions")?;
    db.connection()?
        .execute_raw(Statement::from_string(
            db.backend()?,
            format!("UPDATE {table} SET last_activity=0"),
        ))
        .await
        .map_err(Error::internal)?;
    expired.refresh_csrf().await?;
    assert_eq!(
        expired
            .post("/two-factor-challenge", json!({"recovery_code":codes[1]}))
            .await?
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(expired.state().await?["id"], Value::Null);
    assert!(!expired.has_recaller());
    Ok(())
}

async fn exercise_without_confirmation(fixture: &TestApp<RootApplication>) -> Result<()> {
    let mut browser = Browser::new(fixture);
    browser.refresh_csrf().await?;
    assert_eq!(
        login(&mut browser, "local-only", false).await?.status,
        StatusCode::OK
    );
    assert_eq!(
        browser
            .post("/user/confirm-password", json!({"password":PASSWORD}))
            .await?
            .status,
        StatusCode::CREATED
    );
    assert_eq!(
        browser
            .post("/user/two-factor-authentication", json!({}))
            .await?
            .status,
        StatusCode::OK
    );
    let secret = browser.get("/user/two-factor-secret-key").await?;
    assert_eq!(secret.status, StatusCode::OK);
    let key = secret.json()?["secretKey"]
        .as_str()
        .ok_or(Error::Unauthorized)?
        .to_owned();
    assert_eq!(
        browser
            .get("/user/two-factor-recovery-codes")
            .await?
            .json()?
            .as_array()
            .ok_or(Error::Unauthorized)?
            .len(),
        8
    );
    assert!(
        browser.get("/user/two-factor-qr-code").await?.json()?["svg"]
            .as_str()
            .is_some()
    );
    assert_eq!(
        browser.get("/__fortify-test/protected").await?.status,
        StatusCode::UNAUTHORIZED
    );
    browser.logout().await?;
    assert_eq!(
        login(&mut browser, "local-only", false).await?.json()?,
        json!({"two_factor":true})
    );
    assert_eq!(
        browser
            .post("/two-factor-challenge", json!({"code":code(&key)?}))
            .await?
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        browser.get("/__fortify-test/protected").await?.status,
        StatusCode::OK
    );
    Ok(())
}

async fn dynamic_options_and_boot_routes(fixture: &TestApp<RootApplication>) -> Result<()> {
    let mut browser = Browser::new(fixture);
    browser.refresh_csrf().await?;
    assert_eq!(
        login(&mut browser, "+8613800000000", false).await?.status,
        StatusCode::OK
    );
    assert_eq!(
        browser.get("/__fortify-test/dynamic-options").await?.status,
        StatusCode::OK
    );
    assert_eq!(
        browser
            .post("/user/confirm-password", json!({"password":PASSWORD}))
            .await?
            .status,
        StatusCode::CREATED
    );
    let config = fixture.app()?.config_repository();
    let original = config.get("fortify.features")?.ok_or(Error::Unauthorized)?;
    config.set("fortify.features", json!([]))?;
    let outcome: Result<()> = async {
        // 路由在boot时已安装；运行期feature切换不会删除现有管理入口。
        assert_eq!(
            browser.get("/user/two-factor-qr-code").await?.status,
            StatusCode::OK
        );
        Ok(())
    }
    .await;
    config.set("fortify.features", original)?;
    outcome
}

async fn assert_runtime_store(app: &openrust::Application) -> Result<()> {
    if let Ok(expected) = std::env::var("OPENRUST_FORTIFY_EXPECT_SQLITE_VERSION") {
        let db = app.state().resolve::<Db>().await?;
        assert_eq!(
            db.connection()?.get_database_backend(),
            openrust::database::sea_orm::DbBackend::Sqlite
        );
        let row = db
            .connection()?
            .query_one_raw(Statement::from_string(
                openrust::database::sea_orm::DbBackend::Sqlite,
                "SELECT sqlite_version() AS version, sqlite_source_id() AS source_id".to_owned(),
            ))
            .await
            .map_err(Error::internal)?
            .ok_or(Error::Unauthorized)?;
        assert_eq!(
            row.try_get::<String>("", "version")
                .map_err(Error::internal)?,
            expected
        );
        let source_id = row
            .try_get::<String>("", "source_id")
            .map_err(Error::internal)?;
        assert_eq!(
            source_id,
            std::env::var("OPENRUST_FORTIFY_EXPECT_SQLITE_SOURCE_ID").map_err(Error::internal)?
        );
        eprintln!("FORTIFY_SQLITE_RUNTIME_VERIFIED version={expected} source_id={source_id}");
    }
    let store = app
        .state()
        .resolve::<openrust::session::SessionStoreHandle>()
        .await?;
    assert_eq!(
        store.guarantees(),
        openrust::session::SessionStoreGuarantees::NativeDatabase
    );
    Ok(())
}

async fn run(database: TestDatabase, confirm: bool) -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    crate::modules::example::write_discovery_fixture(root.path())?;
    for directory in [
        "modules/example/resources/views",
        "modules/example/lang",
        "resources/views",
        "bootstrap/cache",
    ] {
        std::fs::create_dir_all(root.path().join(directory)).map_err(Error::internal)?;
    }
    let sources = sources(root.path(), database.url())
        .inspect_err(|error| note("sources", error))?
        .merge_ordered_entries([(
            "fortify-options",
            json!({"two-factor-authentication":{"confirm":confirm}}),
        )])?;
    // env=local保证TestApp的TestingBoot不会绕过真实CSRF校验。
    let mut fixture = TestApp::<RootApplication>::builder(sources)
        .build()
        .await
        .inspect_err(|error| {
            // 只标注白名单阶段及稳定错误码，原Error/source原样返回，不输出配置详情。
            eprintln!(
                "Fortify根宿主build失败：stage=provider_build code={} status={}",
                error.code(),
                error.status()
            );
        })?;
    let outcome: Result<()> = async {
        assert_runtime_store(fixture.app()?).await?;
        provider_lifecycle_and_dynamic_window(fixture.app()?).await?;
        seed(fixture.app()?)
            .await
            .inspect_err(|error| note("seed", error))?;
        dynamic_options_and_boot_routes(&fixture).await?;
        if confirm {
            exercise(&fixture)
                .await
                .inspect_err(|error| note("exercise", error))?;
            #[cfg(feature = "realtime")]
            native_socket::exercise(&fixture).await?;
            Ok(())
        } else {
            exercise_without_confirmation(&fixture)
                .await
                .inspect_err(|error| note("exercise_without_confirmation", error))
        }
    }
    .await;
    let stopped = fixture
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    let closed = database.close().await;
    outcome?;
    stopped?;
    closed
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_root_provider_optional_accounts_fortify_and_real_web_csrf() -> Result<()> {
    run(
        TestDatabase::sqlite_file::<FortifyMigrations>().await?,
        true,
    )
    .await
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_confirm_false_keeps_setup_reads_without_publishing_resource_proof() -> Result<()> {
    run(
        TestDatabase::sqlite_file::<FortifyMigrations>().await?,
        false,
    )
    .await
}

#[cfg(feature = "db-mysql")]
#[tokio::test]
#[ignore = "需要真实MySQL及OPENRUST_TEST_DATABASE_URL；复用根应用Provider/配置/迁移/Session全HTTP链"]
async fn mysql_root_provider_optional_accounts_fortify_and_real_web_csrf() -> Result<()> {
    let url = std::env::var("OPENRUST_TEST_DATABASE_URL").map_err(Error::internal)?;
    let database = TestDatabase::create::<FortifyMigrations>(&url).await?;
    assert_eq!(
        database.connection()?.get_database_backend(),
        openrust::database::sea_orm::DbBackend::MySql
    );
    run(database, true).await
}

#[cfg(feature = "db-postgres")]
#[tokio::test]
#[ignore = "需要真实PostgreSQL及OPENRUST_TEST_DATABASE_URL；复用同根应用完整HTTP链"]
async fn postgres_root_provider_optional_accounts_fortify_and_real_web_csrf() -> Result<()> {
    let url = std::env::var("OPENRUST_TEST_DATABASE_URL").map_err(Error::internal)?;
    let database = TestDatabase::create::<FortifyMigrations>(&url).await?;
    assert_eq!(
        database.connection()?.get_database_backend(),
        openrust::database::sea_orm::DbBackend::Postgres
    );
    run(database, true).await
}

#[cfg(feature = "db-mysql")]
#[tokio::test]
#[ignore = "需要真实数据库；验证原生Session下confirm=false的管理读取和资源证明边界"]
async fn mysql_confirm_false_keeps_setup_reads_without_publishing_resource_proof() -> Result<()> {
    let url = std::env::var("OPENRUST_TEST_DATABASE_URL").map_err(Error::internal)?;
    let database = TestDatabase::create::<FortifyMigrations>(&url).await?;
    assert_eq!(
        database.connection()?.get_database_backend(),
        openrust::database::sea_orm::DbBackend::MySql
    );
    run(database, false).await
}

#[cfg(feature = "db-postgres")]
#[tokio::test]
#[ignore = "需要真实数据库；验证原生Session下confirm=false的管理读取和资源证明边界"]
async fn postgres_confirm_false_keeps_setup_reads_without_publishing_resource_proof() -> Result<()>
{
    let url = std::env::var("OPENRUST_TEST_DATABASE_URL").map_err(Error::internal)?;
    let database = TestDatabase::create::<FortifyMigrations>(&url).await?;
    assert_eq!(
        database.connection()?.get_database_backend(),
        openrust::database::sea_orm::DbBackend::Postgres
    );
    run(database, false).await
}
