//! 根应用显式 JWT API profile 的真实 Router、账户域与无状态生命周期。

use crate::bootstrap::Server;
use openrust::auth::{AuthFlows, JwtTokens, fortify::FortifyJwtApi};
use openrust::config::{ConfigLoader, ConfigSources};
use openrust::core::ProviderId;
use openrust::http::axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Method, Request, StatusCode, header},
};
use openrust::session::{SessionServiceProvider, SessionStoreHandle};
use openrust::testing::{TestApp, TestAppBuilder, TestDatabase};
use openrust::{Db, Error, Result};
use serde_json::{Value, json};
use std::{net::SocketAddr, path::Path, time::Duration};

const PASSWORD: &str = "root-jwt-integration-password";
const EMAIL: &str = "api-member@example.test";
const PREFIX: &str = "/api/auth";
const IP: [u8; 4] = [127, 0, 0, 1];
const TEST_KEY: &str = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
const TIMEOUT: Duration = Duration::from_secs(10);

fn invalid() -> Error {
    Error::custom("root_jwt_api_fixture_invalid", "根 JWT API 测试状态缺失")
}

fn sources(root: &Path, url: &str, api: bool, issuer: &str) -> Result<ConfigSources> {
    crate::modules::example::write_discovery_fixture(root)?;
    for dir in [
        "config",
        "modules/example/resources/views",
        "modules/example/lang",
        "resources/views",
        "bootstrap/cache",
    ] {
        std::fs::create_dir_all(root.join(dir)).map_err(Error::internal)?;
    }
    std::fs::write(
        root.join("config/cache.toml"),
        "default='redis'\n[stores.redis]\ndriver='memory'\nnamespace='root-jwt-api'\n",
    )
    .map_err(Error::internal)?;
    let overrides = api.then(|| ("auth-profile", json!("jwt-api")));
    let source = ConfigLoader::application_code_sources_with_overrides(
        root,
        "ROOT_JWT_API_TEST_",
        Some("local"),
        &crate::config::boot().with_toml(),
        overrides,
    )?;
    source.merge_ordered_entries([
        ("application_root", json!(root)),
        ("modules", json!({"activator":"registered","activators":{"file":{"statuses-file":root.join("module-statuses.json")}}})),
        ("app", json!({"env":"local","url":"http://localhost"})),
        ("database", json!({"url":url,"max_connections":4,"min_connections":0})),
        ("session", json!({"driver":"database","secure":false,"lottery":[0,100]})),
        ("security", json!({"encryption":{"active_key_id":"test","keys":{"test":TEST_KEY}},"signed_links":{"active_key_id":"test","keys":{"test":TEST_KEY}}})),
        ("auth", json!({"jwt":{"issuer":issuer,"active_key_id":"test","keys":{"test":TEST_KEY}}})),
        #[cfg(feature = "realtime")]
        ("realtime", json!({"allowed_origins":["http://localhost"]})),
    ]).map_err(Error::from)
}

fn builder(source: ConfigSources, example: bool, stateless: bool) -> TestAppBuilder<Server> {
    let mut builder = TestApp::<Server>::builder(source);
    if !example {
        builder.remove_module("example");
    }
    if stateless {
        builder
            .remove_provider(ProviderId::of::<openrust::http::CookieServiceProvider>())
            .remove_provider(ProviderId::of::<SessionServiceProvider>());
    }
    builder
}

#[derive(Clone, Copy)]
enum Authorization<'a> {
    None,
    Bearer(&'a str),
    Management(&'a str),
}

struct Client<'a> {
    app: &'a TestApp<Server>,
    ip: [u8; 4],
}
impl<'a> Client<'a> {
    fn new(app: &'a TestApp<Server>) -> Self {
        Self { app, ip: IP }
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        auth: Authorization<'_>,
        input: Option<Value>,
        expected: StatusCode,
    ) -> Result<Value> {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .extension(ConnectInfo(SocketAddr::from((self.ip, 41000))))
            .header(header::ACCEPT, "application/json")
            .header(header::HOST, "localhost");
        request = match auth {
            Authorization::None => request,
            Authorization::Bearer(raw) => {
                request.header(header::AUTHORIZATION, format!("Bearer {raw}"))
            }
            Authorization::Management(raw) => request.header("x-fortify-management", raw),
        };
        let body = if let Some(input) = input {
            request = request.header(header::CONTENT_TYPE, "application/json");
            Body::from(input.to_string())
        } else {
            Body::empty()
        };
        let response = self
            .app
            .request(request.body(body).map_err(Error::internal)?)
            .await?;
        assert_eq!(response.status(), expected, "{path}");
        assert!(
            response.headers().get(header::SET_COOKIE).is_none(),
            "{path}"
        );
        if path.starts_with(PREFIX) && expected != StatusCode::NOT_FOUND {
            response.assert_header("cache-control", "no-store")?;
        }
        if expected.is_success() {
            serde_json::from_slice(response.body()).map_err(Error::internal)
        } else {
            Ok(Value::Null)
        }
    }

    async fn api(
        &self,
        method: Method,
        path: &str,
        auth: Authorization<'_>,
        input: Option<Value>,
        status: StatusCode,
    ) -> Result<Value> {
        self.request(method, &format!("{PREFIX}{path}"), auth, input, status)
            .await
    }
    async fn login(&self) -> Result<Value> {
        self.api(
            Method::POST,
            "/login",
            Authorization::None,
            Some(json!({"account":EMAIL,"password":PASSWORD})),
            StatusCode::OK,
        )
        .await
    }
    async fn user(&self, token: &str, status: StatusCode) -> Result<Value> {
        self.api(
            Method::GET,
            "/user",
            Authorization::Bearer(token),
            None,
            status,
        )
        .await
    }
    async fn grant(&self, access: &str, password: &str, status: StatusCode) -> Result<Value> {
        self.api(
            Method::POST,
            "/two-factor/authorize",
            Authorization::Bearer(access),
            Some(json!({"password":password})),
            status,
        )
        .await
    }
    async fn finish(
        &self,
        challenge: &str,
        code: Option<&str>,
        recovery: Option<&str>,
        status: StatusCode,
    ) -> Result<Value> {
        self.api(
            Method::POST,
            "/two-factor-challenge",
            Authorization::None,
            Some(json!({"challenge":challenge,"code":code,"recovery_code":recovery})),
            status,
        )
        .await
    }
    async fn refresh(&self, path: &str, refresh: &str, status: StatusCode) -> Result<Value> {
        self.api(
            Method::POST,
            path,
            Authorization::None,
            Some(json!({"refresh_token":refresh})),
            status,
        )
        .await
    }
}

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value[field]
        .as_str()
        .filter(|raw| !raw.is_empty())
        .ok_or_else(invalid)
}

async fn register(app: &TestApp<Server>) -> Result<()> {
    let flows = app.app()?.state().resolve::<AuthFlows>().await?;
    let code = flows
        .register(EMAIL, PASSWORD, "127.0.0.1")
        .await?
        .ok_or_else(invalid)?;
    assert!(flows.verify_email(EMAIL, code.code(), "127.0.0.1").await?);
    Ok(())
}

fn challenge(value: &Value) -> Result<&str> {
    assert_eq!(value["two_factor"], true);
    for field in [
        "access_token",
        "refresh_token",
        "user_id",
        "management_token",
    ] {
        assert!(value.get(field).is_none(), "{field}");
    }
    let expiry: chrono::DateTime<chrono::Utc> =
        serde_json::from_value(value["expires_at"].clone()).map_err(Error::internal)?;
    let remaining = (expiry - chrono::Utc::now()).num_seconds();
    assert!((280..=300).contains(&remaining));
    text(value, "challenge")
}

async fn management(
    client: &Client<'_>,
    other: &Client<'_>,
    initial: &Value,
) -> Result<Vec<String>> {
    let access = text(initial, "access_token")?;
    client
        .grant(access, "incorrect-password", StatusCode::UNAUTHORIZED)
        .await?;
    let authorized = client.grant(access, PASSWORD, StatusCode::OK).await?;
    let grant = text(&authorized, "management_token")?;
    client.user(grant, StatusCode::UNAUTHORIZED).await?;
    other.user(access, StatusCode::UNAUTHORIZED).await?;
    other
        .api(
            Method::POST,
            "/two-factor",
            Authorization::Management(grant),
            Some(json!({})),
            StatusCode::UNAUTHORIZED,
        )
        .await?;
    client
        .api(
            Method::POST,
            "/two-factor",
            Authorization::Management("forged"),
            Some(json!({})),
            StatusCode::UNAUTHORIZED,
        )
        .await?;
    let auth = Authorization::Management(grant);
    let enabled = client
        .api(
            Method::POST,
            "/two-factor",
            auth,
            Some(json!({})),
            StatusCode::OK,
        )
        .await?;
    assert_eq!(enabled.as_object().map(serde_json::Map::len), Some(2));
    assert_eq!(enabled["enabled"].as_bool(), Some(true));
    assert_eq!(enabled["confirmation_required"].as_bool(), Some(true));
    client.user(access, StatusCode::UNAUTHORIZED).await?;
    let (secret, codes) = enrollment_material(client, auth).await?;
    let code = super::fortify_flows::code_with_offset(&secret, 0)?;
    let confirmed = client
        .api(
            Method::POST,
            "/two-factor/confirm",
            auth,
            Some(json!({"code":code})),
            StatusCode::OK,
        )
        .await?;
    assert_eq!(confirmed.as_object().map(serde_json::Map::len), Some(2));
    assert_eq!(confirmed["confirmed"].as_bool(), Some(true));
    assert_eq!(confirmed["login_required"].as_bool(), Some(true));
    client
        .api(
            Method::GET,
            "/two-factor/secret",
            auth,
            None,
            StatusCode::UNAUTHORIZED,
        )
        .await?;
    client.user(access, StatusCode::UNAUTHORIZED).await?;
    Ok(codes)
}

async fn enrollment_material(
    client: &Client<'_>,
    auth: Authorization<'_>,
) -> Result<(String, Vec<String>)> {
    let secret = client
        .api(
            Method::GET,
            "/two-factor/secret",
            auth,
            None,
            StatusCode::OK,
        )
        .await?;
    let secret = text(&secret, "secret_key")?.to_owned();
    let qr = client
        .api(
            Method::GET,
            "/two-factor/qr-code",
            auth,
            None,
            StatusCode::OK,
        )
        .await?;
    let url = url::Url::parse(text(&qr, "url")?).map_err(Error::internal)?;
    assert_eq!(url.scheme(), "otpauth");
    assert!(
        url.query_pairs()
            .any(|(key, value)| key == "secret" && value == secret.as_str())
    );
    assert!(text(&qr, "svg")?.contains("<svg"));
    let recovery = client
        .api(
            Method::GET,
            "/two-factor/recovery-codes",
            auth,
            None,
            StatusCode::OK,
        )
        .await?;
    let codes: Vec<String> =
        serde_json::from_value(recovery["recovery_codes"].clone()).map_err(Error::internal)?;
    assert_eq!(
        codes.iter().collect::<std::collections::HashSet<_>>().len(),
        8
    );
    assert_eq!(codes.len(), 8);
    Ok((secret, codes))
}

async fn replay_and_logout(
    client: &Client<'_>,
    other: &Client<'_>,
    codes: &[String],
    id: i64,
) -> Result<()> {
    let challenged = client.login().await?;
    let raw = challenge(&challenged)?;
    client.user(raw, StatusCode::UNAUTHORIZED).await?;
    other
        .finish(raw, None, Some(&codes[0]), StatusCode::UNAUTHORIZED)
        .await?;
    let attacker = Client {
        app: client.app,
        ip: [127, 0, 0, 2],
    };
    attacker
        .finish(raw, None, Some(&codes[0]), StatusCode::UNAUTHORIZED)
        .await?;
    client
        .finish(raw, Some("invalid-totp"), None, StatusCode::UNAUTHORIZED)
        .await?;
    let pair = client
        .finish(raw, None, Some(&codes[0]), StatusCode::OK)
        .await?;
    let access = text(&pair, "access_token")?;
    let refresh = text(&pair, "refresh_token")?;
    assert_eq!(client.user(access, StatusCode::OK).await?["id"], id);
    client
        .finish(raw, None, Some(&codes[0]), StatusCode::UNAUTHORIZED)
        .await?;
    let authorized = client.grant(access, PASSWORD, StatusCode::OK).await?;
    let recovery = client
        .api(
            Method::GET,
            "/two-factor/recovery-codes",
            Authorization::Management(text(&authorized, "management_token")?),
            None,
            StatusCode::OK,
        )
        .await?;
    let remaining: Vec<String> =
        serde_json::from_value(recovery["recovery_codes"].clone()).map_err(Error::internal)?;
    assert_eq!(remaining.len(), 8);
    assert!(!remaining.contains(&codes[0]));
    assert!(codes[1..].iter().all(|code| remaining.contains(code)));
    other
        .refresh("/refresh", refresh, StatusCode::UNAUTHORIZED)
        .await?;
    other
        .refresh("/logout", refresh, StatusCode::UNAUTHORIZED)
        .await?;
    client.user(access, StatusCode::OK).await?;
    let rotated = client.refresh("/refresh", refresh, StatusCode::OK).await?;
    assert!(!text(&rotated, "refresh_token")?.eq(refresh));
    // SQL 期限以微秒持久化；轮换只能继承该绝对时刻。
    let expiry = |value: &Value| -> Result<i64> {
        let date: chrono::DateTime<chrono::Utc> =
            serde_json::from_value(value["refresh_expires_at"].clone()).map_err(Error::internal)?;
        Ok(date.timestamp_micros())
    };
    assert_eq!(expiry(&rotated)?, expiry(&pair)?);
    let next = text(&rotated, "access_token")?;
    // 异域连已轮换的旧 token 也不能借重放检测撤销正确发行域。
    other
        .refresh("/refresh", refresh, StatusCode::UNAUTHORIZED)
        .await?;
    other
        .refresh("/logout", refresh, StatusCode::UNAUTHORIZED)
        .await?;
    client.user(next, StatusCode::OK).await?;
    client
        .refresh("/refresh", refresh, StatusCode::UNAUTHORIZED)
        .await?;
    client.user(next, StatusCode::UNAUTHORIZED).await?;
    client.user(access, StatusCode::UNAUTHORIZED).await?;
    let next_challenge = client.login().await?;
    let next_challenge = challenge(&next_challenge)?;
    client
        .finish(
            next_challenge,
            None,
            Some(&codes[0]),
            StatusCode::UNAUTHORIZED,
        )
        .await?;
    let pair = client
        .finish(next_challenge, None, Some(&codes[1]), StatusCode::OK)
        .await?;
    let signed = text(&pair, "access_token")?;
    client.user(signed, StatusCode::OK).await?;
    let logged_out = client
        .refresh("/logout", text(&pair, "refresh_token")?, StatusCode::OK)
        .await?;
    assert_eq!(logged_out["logged_out"], true);
    client.user(signed, StatusCode::UNAUTHORIZED).await?;
    Ok(())
}

#[derive(Default)]
struct AuditGate {
    armed: std::sync::atomic::AtomicBool,
    reached: tokio::sync::Notify,
}
struct CancellableAudit {
    inner: std::sync::Arc<dyn openrust::auth::ApiLoginAuditSink>,
    gate: std::sync::Arc<AuditGate>,
}
impl openrust::auth::ApiLoginAuditSink for CancellableAudit {
    fn require_store(&self, db: &Db) -> Result<()> {
        self.inner.require_store(db)
    }
    fn record_in_transaction<'a>(
        &'a self,
        tx: &'a openrust::db::DatabaseTransaction,
        id: i64,
    ) -> openrust::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.inner.record_in_transaction(tx, id).await?;
            if self
                .gate
                .armed
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                self.gate.reached.notify_one();
                std::future::pending::<()>().await;
            }
            Ok(())
        })
    }
}

async fn attempt_boundaries(
    client: &Client<'_>,
    gate: &AuditGate,
    db: &Db,
    codes: &[String],
) -> Result<()> {
    let challenged = client.login().await?;
    let raw = challenge(&challenged)?;
    let families = db.table("auth_refresh_tokens")?.count().await?;
    let audits = db
        .table("audit_records")?
        .where_eq("action", "auth.two_factor.login")?;
    let completed_audits = audits.count().await?;
    gate.armed.store(true, std::sync::atomic::Ordering::SeqCst);
    {
        let request = client.finish(raw, None, Some(&codes[2]), StatusCode::OK);
        tokio::pin!(request);
        tokio::select! {
            () = gate.reached.notified() => {},
            result = &mut request => { result?; return Err(invalid()); },
            () = tokio::time::sleep(TIMEOUT) => return Err(invalid()),
        }
    }
    assert_eq!(audits.count().await?, completed_audits);
    assert_eq!(db.table("auth_refresh_tokens")?.count().await?, families);
    let records = db
        .table("auth_factor_challenges")?
        .where_null("consumed_at")?;
    assert_eq!(
        records.clone().where_eq("attempts", 1_i64)?.count().await?,
        1
    );
    client
        .finish(raw, None, Some(&codes[2]), StatusCode::OK)
        .await?;
    assert_eq!(audits.count().await?, completed_audits + 1);
    assert_eq!(
        db.table("auth_refresh_tokens")?.count().await?,
        families + 1
    );
    let challenged = client.login().await?;
    let exhausted = challenge(&challenged)?;
    for attempt in 1_i64..=5 {
        client
            .finish(
                exhausted,
                None,
                Some("wrong-recovery"),
                StatusCode::UNAUTHORIZED,
            )
            .await?;
        assert_eq!(
            records
                .clone()
                .where_eq("attempts", attempt)?
                .count()
                .await?,
            1
        );
    }
    client
        .finish(
            exhausted,
            None,
            Some(&codes[3]),
            StatusCode::TOO_MANY_REQUESTS,
        )
        .await?;
    assert_eq!(audits.count().await?, completed_audits + 1);
    assert_eq!(
        db.table("auth_refresh_tokens")?.count().await?,
        families + 1
    );
    Ok(())
}

async fn exercise(app: &TestApp<Server>, other: &TestApp<Server>, gate: &AuditGate) -> Result<()> {
    let state = app.app()?.state();
    assert!(!state.container().contains::<SessionStoreHandle>());
    assert!(state.container().contains::<FortifyJwtApi>());
    let cache = app
        .app()?
        .section::<openrust::config::CacheConfig>("cache")?;
    assert_eq!(cache.stores.len(), 1);
    assert_eq!(cache.stores[&cache.default].driver, "memory");
    assert_eq!(app.app()?.section::<String>("auth.defaults.guard")?, "api");
    assert_eq!(
        app.app()?.section::<String>("auth.guards.api.provider")?,
        "users"
    );
    assert_eq!(app.app()?.section::<String>("fortify.username")?, "account");
    let client = Client::new(app);
    let other = Client::new(other);
    client
        .request(
            Method::POST,
            "/login",
            Authorization::None,
            None,
            StatusCode::NOT_FOUND,
        )
        .await?;
    register(app).await?;
    let db = state.resolve::<Db>().await?;
    // 模拟已有账户的较低 Argon 成本，经过真实根账户回调触发 rehash。
    let weaker = openrust::auth::Passwords::new(openrust::config::PasswordConfig {
        memory_kib: 19_456,
        iterations: 1,
        parallelism: 1,
    })?
    .hash(PASSWORD)
    .await?;
    let accounts = db.table("users")?.where_eq("email", EMAIL)?;
    assert_eq!(
        accounts
            .update(vec![("password_hash", weaker.as_str().into())])
            .await?,
        1
    );
    let initial = client.login().await?;
    let row = accounts.first().await?.ok_or_else(invalid)?;
    let current: String = row.try_get("", "password_hash").map_err(Error::internal)?;
    assert!(!current.eq(weaker.as_str()));
    let current = openrust::auth::EncodedPasswordHash::parse(current)?;
    assert!(
        !state
            .resolve::<openrust::auth::Passwords>()
            .await?
            .needs_rehash(&current)?
    );
    assert!(initial.get("challenge").is_none());
    assert_eq!(initial["token_type"], "Bearer");
    let id = initial["user_id"]
        .as_i64()
        .filter(|id| *id > 0)
        .ok_or_else(invalid)?;
    let access = text(&initial, "access_token")?;
    text(&initial, "refresh_token")?;
    assert_eq!(client.user(access, StatusCode::OK).await?["id"], id);
    let bare = state.resolve::<JwtTokens>().await?.issue_access_token(id)?;
    client.user(&bare, StatusCode::UNAUTHORIZED).await?;
    let flows = state.resolve::<AuthFlows>().await?;
    let unbound = flows.login(EMAIL, PASSWORD, "127.0.0.1").await?;
    client
        .user(unbound.access_token(), StatusCode::UNAUTHORIZED)
        .await?;
    let codes = management(&client, &other, &initial).await?;
    replay_and_logout(&client, &other, &codes, id).await?;
    attempt_boundaries(&client, gate, &db, &codes).await?;
    assert_eq!(db.table("sessions")?.count().await?, 0);
    Ok(())
}

async fn stop(app: &mut TestApp<Server>) -> Result<()> {
    app.shutdown(tokio::time::Instant::now() + TIMEOUT).await
}

async fn lifecycle(database: &TestDatabase) -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    let other_root = tempfile::tempdir().map_err(Error::internal)?;
    let source = sources(root.path(), database.url(), true, "https://issuer.test")?;
    let other_source = sources(
        other_root.path(),
        database.url(),
        true,
        "https://other-issuer.test",
    )?;
    let mut selected = builder(source, false, true);
    let gate = std::sync::Arc::new(AuditGate::default());
    let observed = gate.clone();
    selected.configure_after_register(move |bindings| {
        let gate = observed.clone();
        Box::pin(async move {
            let inner = bindings
                .resolve::<dyn openrust::auth::ApiLoginAuditSink>()
                .await?;
            bindings
                .instance::<dyn openrust::auth::ApiLoginAuditSink>(std::sync::Arc::new(
                    CancellableAudit { inner, gate },
                ))
                .await
        })
    });
    let mut primary = selected.build().await?;
    let result: Result<()> = async {
        let mut other = builder(other_source, false, true).build().await?;
        let result = exercise(&primary, &other, &gate).await;
        let stopped = stop(&mut other).await;
        result.and(stopped)
    }
    .await;
    let stopped = stop(&mut primary).await;
    result.and(stopped)
}

async fn run(database: TestDatabase) -> Result<()> {
    let result = lifecycle(&database).await;
    let closed = database.close().await;
    result.and(closed)
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_root_jwt_api_without_session_or_cookie_provider() -> Result<()> {
    run(TestDatabase::sqlite_file::<crate::database::Migrator>().await?).await
}

#[cfg(feature = "db-mysql")]
#[tokio::test]
#[ignore = "需要 OPENRUST_TEST_DATABASE_URL 指向真实 MySQL"]
async fn mysql_root_jwt_api_without_session_or_cookie_provider() -> Result<()> {
    use openrust::db::sea_orm::DbBackend;
    let url = std::env::var("OPENRUST_TEST_DATABASE_URL").map_err(Error::internal)?;
    let db = TestDatabase::create::<crate::database::Migrator>(&url).await?;
    assert_eq!(db.connection()?.get_database_backend(), DbBackend::MySql);
    run(db).await
}

#[cfg(feature = "db-postgres")]
#[tokio::test]
#[ignore = "需要 OPENRUST_TEST_DATABASE_URL 指向真实 PostgreSQL"]
async fn postgres_root_jwt_api_without_session_or_cookie_provider() -> Result<()> {
    use openrust::db::sea_orm::DbBackend;
    let url = std::env::var("OPENRUST_TEST_DATABASE_URL").map_err(Error::internal)?;
    let db = TestDatabase::create::<crate::database::Migrator>(&url).await?;
    assert_eq!(db.connection()?.get_database_backend(), DbBackend::Postgres);
    run(db).await
}

#[cfg(feature = "db-sqlite")]
struct ChangeRegisteredProfile;

#[cfg(feature = "db-sqlite")]
impl openrust::core::ServiceProvider for ChangeRegisteredProfile {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::core::RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.config_repository()
                .set("auth-profile", json!("jwt-api"))?;
            Ok(())
        })
    }
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn default_root_uses_web_and_rejects_unknown_profiles() -> Result<()> {
    let db = TestDatabase::sqlite_file::<crate::database::Migrator>().await?;
    let result: Result<()> = async {
        let root = tempfile::tempdir().map_err(Error::internal)?;
        let source = sources(root.path(), db.url(), false, "https://issuer.test")?;
        let bad = source
            .clone()
            .merge_ordered_entries([("auth-profile", json!("unknown"))])?;
        let error = builder(bad, false, false)
            .build()
            .await
            .err()
            .ok_or_else(invalid)?;
        assert_eq!(error.code(), "application_auth_profile_invalid");
        let config = source
            .figment()
            .extract::<crate::bootstrap::Configuration>()
            .map_err(Error::internal)?;
        let mut changed = openrust::core::ProviderBuilder::for_application::<Server>(&config);
        changed
            .remove_module("example")
            .provider(ChangeRegisteredProfile);
        let error = changed
            .start(source.clone())
            .await
            .err()
            .ok_or_else(invalid)?;
        assert_eq!(error.code(), "application_auth_profile_changed");
        let mut app = builder(source, false, false).build().await?;
        let result: Result<()> = async {
            assert_eq!(app.app()?.section::<String>("auth-profile")?, "web");
            assert!(!app.app()?.state().container().contains::<FortifyJwtApi>());
            assert_eq!(app.app()?.section::<String>("auth.defaults.guard")?, "web");
            Client::new(&app)
                .api(
                    Method::POST,
                    "/login",
                    Authorization::None,
                    None,
                    StatusCode::NOT_FOUND,
                )
                .await?;
            // 官方 Web POST /login 已挂载且受真实本地 CSRF 保护。
            app.request(
                Request::post("/login")
                    .header(header::HOST, "localhost")
                    .header(header::ACCEPT, "application/json")
                    .body(Body::empty())
                    .map_err(Error::internal)?,
            )
            .await?
            .assert_status(StatusCode::from_u16(419).map_err(Error::internal)?)?;
            Ok(())
        }
        .await;
        result.and(stop(&mut app).await)
    }
    .await;
    result.and(db.close().await)
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn example_account_resource_requires_the_selected_api_guard() -> Result<()> {
    let db = TestDatabase::sqlite_file::<crate::database::Migrator>().await?;
    let result: Result<()> = async {
        let root = tempfile::tempdir().map_err(Error::internal)?;
        let source = sources(root.path(), db.url(), true, "https://issuer.test")?;
        let mut app = builder(source, true, false).build().await?;
        let result: Result<()> = async {
            register(&app).await?;
            let client = Client::new(&app);
            let pair = client.login().await?;
            let id = pair["user_id"].as_i64().ok_or_else(invalid)?;
            let path = "/api/example/account/me";
            client
                .request(
                    Method::GET,
                    path,
                    Authorization::None,
                    None,
                    StatusCode::UNAUTHORIZED,
                )
                .await?;
            let result = client
                .request(
                    Method::GET,
                    path,
                    Authorization::Bearer(text(&pair, "access_token")?),
                    None,
                    StatusCode::OK,
                )
                .await?;
            assert_eq!(result["data"]["user_id"], id);
            let bare = app
                .app()?
                .state()
                .resolve::<JwtTokens>()
                .await?
                .issue_access_token(id)?;
            client
                .request(
                    Method::GET,
                    path,
                    Authorization::Bearer(&bare),
                    None,
                    StatusCode::UNAUTHORIZED,
                )
                .await?;
            for path in [
                "/api/example/account/session/login",
                "/api/example/account/login",
            ] {
                client
                    .request(
                        Method::POST,
                        path,
                        Authorization::None,
                        Some(json!({"account":EMAIL,"password":PASSWORD})),
                        StatusCode::NOT_FOUND,
                    )
                    .await?;
            }
            Ok(())
        }
        .await;
        result.and(stop(&mut app).await)
    }
    .await;
    result.and(db.close().await)
}

#[cfg(feature = "db-sqlite")]
struct ApiShortCircuit;
#[cfg(feature = "db-sqlite")]
impl openrust::core::ServiceProvider for ApiShortCircuit {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.middleware_alias(
                "test-api-failure",
                openrust::http::axum::middleware::from_fn(
                    |request: openrust::http::axum::extract::Request,
                     next: openrust::http::axum::middleware::Next| async move {
                        use openrust::http::axum::response::IntoResponse as _;
                        if request.headers().contains_key("x-test-delay") {
                            tokio::time::sleep(Duration::from_millis(1_500)).await;
                        }
                        let code = request
                            .headers()
                            .get("x-test-error")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.parse::<u16>().ok());
                        match code {
                            Some(code @ (401 | 408 | 413 | 429 | 500)) => {
                                StatusCode::from_u16(code).map_or_else(
                                    |_| StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                                    openrust::http::axum::response::IntoResponse::into_response,
                                )
                            }
                            _ => next.run(request).await,
                        }
                    },
                ),
            )?;
            app.middleware_group("api", ["test-api-failure"])?;
            Ok(())
        })
    }
}
#[cfg(feature = "db-sqlite")]
struct ApiGroupApplication;
#[cfg(feature = "db-sqlite")]
impl openrust::core::Application for ApiGroupApplication {
    type Config = <Server as openrust::core::Application>::Config;
    fn config() -> openrust::config::ConfigManifest {
        <Server as openrust::core::Application>::config()
    }
    fn default_providers() -> openrust::core::DefaultProviders {
        <Server as openrust::core::Application>::default_providers()
    }
    fn additional_providers() -> Vec<Box<dyn openrust::core::ServiceProvider>> {
        vec![Box::new(ApiShortCircuit)]
    }
    fn providers(config: &Self::Config) -> Vec<Box<dyn openrust::core::ServiceProvider>> {
        <Server as openrust::core::Application>::providers(config)
    }
    fn modules(config: &Self::Config) -> openrust::core::ModuleManifest {
        <Server as openrust::core::Application>::modules(config)
    }
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn api_group_short_circuit_errors_are_not_cached_or_given_cookies() -> Result<()> {
    let database = TestDatabase::sqlite_file::<crate::database::Migrator>().await?;
    let outcome: Result<()> = async {
        let root = tempfile::tempdir().map_err(Error::internal)?;
        let source = sources(root.path(), database.url(), true, "https://api-error.test")?
            .merge_ordered_entries([
                ("server", json!({"request_timeout_secs":1})),
                ("api", json!({"prefix":"/api/{tenant}"})),
            ])?;
        let mut builder = TestApp::<ApiGroupApplication>::builder(source);
        builder
            .remove_module("example")
            .remove_provider(ProviderId::of::<openrust::http::CookieServiceProvider>())
            .remove_provider(ProviderId::of::<SessionServiceProvider>());
        let mut app = builder.build().await?;
        let outcome: Result<()> = async {
            assert!(
                !app.app()?
                    .state()
                    .container()
                    .contains::<SessionStoreHandle>()
            );
            for code in [401, 408, 413, 429, 500] {
                for (method, path) in [
                    (Method::POST, "/api/acme/auth/login"),
                    (Method::GET, "/api/acme/auth/user"),
                ] {
                    let response = app
                        .request(
                            Request::builder()
                                .method(method)
                                .uri(path)
                                .header(header::HOST, "localhost")
                                .header(header::ACCEPT, "application/json")
                                .header("x-test-error", code.to_string())
                                .body(Body::empty())
                                .map_err(Error::internal)?,
                        )
                        .await?;
                    response.assert_status(StatusCode::from_u16(code).map_err(Error::internal)?)?;
                    assert_eq!(
                        response
                            .headers()
                            .get("cache-control")
                            .and_then(|v| v.to_str().ok()),
                        Some("no-store"),
                        "{path} status={code} cache-control"
                    );
                    assert_eq!(
                        response
                            .headers()
                            .get("pragma")
                            .and_then(|v| v.to_str().ok()),
                        Some("no-cache"),
                        "{path} status={code} pragma"
                    );
                    assert!(response.headers().get(header::SET_COOKIE).is_none());
                }
            }
            // 真实全局超时会丢弃路由future，错误策略仍须在最终异常渲染后生效。
            let response = app
                .request(
                    Request::post("/api/acme/auth/login")
                        .header(header::HOST, "localhost")
                        .header(header::ACCEPT, "application/json")
                        .header("x-test-delay", "1")
                        .body(Body::empty())
                        .map_err(Error::internal)?,
                )
                .await?;
            response.assert_status(StatusCode::REQUEST_TIMEOUT)?;
            response.assert_header("cache-control", "no-store")?;
            response.assert_header("pragma", "no-cache")?;
            assert!(response.headers().get(header::SET_COOKIE).is_none());
            Ok(())
        }
        .await;
        outcome.and(app.shutdown(tokio::time::Instant::now() + TIMEOUT).await)
    }
    .await;
    outcome.and(database.close().await)
}
