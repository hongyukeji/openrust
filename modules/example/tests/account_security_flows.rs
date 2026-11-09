//! 生产模块账户安全 HTTP 回归；进程内请求不冒充 TCP/Redis 验收。
#![cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]

use figment::{Figment, providers::Serialized};
use openrust::auth::AuthFlows;
#[cfg(feature = "auth-fortify")]
use openrust::auth::JwtTokens;
use openrust::database::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use openrust::testing::{TestApp, TestDatabase, TestResponse};
use openrust::{Error, Result};
use std::time::Duration;

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const BASE: &str = "/api/example/account";
const EMAIL: &str = "account-security@example.test";
const PASSWORD: &str = "correct horse battery staple";
const ORIGIN: &str = "http://example.test";
struct Migrator;
#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = super::selected_test_auth::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}
struct Fixture {
    app: TestApp<crate::bootstrap::Server>,
    database: TestDatabase,
    #[cfg(feature = "auth-fortify")]
    user_id: i64,
}
impl Fixture {
    async fn create() -> TestResult<Option<Self>> {
        Self::create_with_providers(|_| {}).await
    }

    async fn create_with_providers(
        configure: impl Fn(&mut openrust::foundation::ProviderBuilder) + Send + Sync + 'static,
    ) -> TestResult<Option<Self>> {
        let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
            eprintln!("skipping account security DB test: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(None);
        };
        let database = TestDatabase::create::<Migrator>(&admin).await?;
        let mut framework = openrust::config::FrameworkConfig::default();
        framework.api.http_status = openrust::config::HttpStatusMode::Semantic;
        let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
        let source=Figment::from(Serialized::defaults(framework)).merge(Serialized::defaults(serde_json::json!({
            "app":{"env":"test","url":ORIGIN,"key":"0123456789abcdef0123456789abcdef"},"database":{"url":database.url()},
            "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
            "auth":{"defaults":{"guard":"web","passwords":"users","provider":null},"guards":{"web":{"driver":"session","provider":"users"}},"providers":{"users":{"driver":"app-users","table":"users"}},"jwt":{"issuer":ORIGIN,"audience":"account-tests","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":ORIGIN,"active_key_id":"test","keys":{"test":key}}},
            "session":{"secure":false,"cookie":"example_account_session","same_site":"strict"}
        })));
        let mut builder = TestApp::<crate::bootstrap::Server>::builder(
            crate::modules::example::test_sources(&(source))?,
        );
        builder.configure_providers(configure);
        let app = builder.build().await?;
        let flows = app.app()?.state().resolve::<AuthFlows>().await?;
        let code = flows
            .register(EMAIL, PASSWORD, "fixture-seed")
            .await?
            .ok_or("missing verification code")?;
        assert!(
            flows
                .verify_email(EMAIL, code.code(), "fixture-seed")
                .await?
        );
        #[cfg(feature = "auth-fortify")]
        let credentials = flows.login(EMAIL, PASSWORD, "fixture-seed").await?;
        #[cfg(feature = "auth-fortify")]
        {
            let raw = app
                .app()?
                .state()
                .resolve::<JwtTokens>()
                .await?
                .issue_access_token(credentials.user_id())?;
            app.request(
                Request::get("/api/example/me")
                    .header("host", "example.test")
                    .header("authorization", format!("Bearer {raw}"))
                    .body(Body::empty())?,
            )
            .await?
            .assert_status(StatusCode::UNAUTHORIZED)?;
        }
        Ok(Some(Self {
            app,
            database,
            #[cfg(feature = "auth-fortify")]
            user_id: credentials.user_id(),
        }))
    }
    async fn close(mut self) -> TestResult {
        self.app
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await?;
        self.database.close().await?;
        Ok(())
    }
    async fn post(
        &self,
        path: &str,
        value: &serde_json::Value,
        token: Option<&str>,
        cookie: Option<&str>,
        csrf: Option<&str>,
    ) -> Result<TestResponse> {
        let response = self
            .app
            .request(request("POST", path, value, token, cookie, csrf)?)
            .await?;
        trace_response("POST", path, &response);
        Ok(response)
    }
}
fn trace_response(method: &str, path: &str, response: &TestResponse) {
    let code = response
        .json::<serde_json::Value>()
        .ok()
        .and_then(|value| {
            value
                .get("code")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .filter(|code| {
            code.len() <= 64
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        });
    eprintln!(
        "account_security_http method={method} path={path} status={} code={code:?}",
        response.status().as_u16()
    );
}

fn credentials() -> serde_json::Value {
    serde_json::json!({"email":EMAIL,"password":PASSWORD})
}
fn request(
    method: &str,
    path: &str,
    value: &serde_json::Value,
    token: Option<&str>,
    cookie: Option<&str>,
    csrf: Option<&str>,
) -> Result<Request<Body>> {
    // 模拟不同可信 socket 客户端，保留框架每来源登录限流，挑战及完成使用同一来源。
    let peer = if path == "/session/login" { 10 } else { 24 };
    let mut builder = Request::builder()
        .method(method)
        .uri(format!("{BASE}{path}"))
        // TestApp 的相对 URI 不会自动补浏览器 Host；域路由须先验证 authority。
        .header("host", "example.test")
        .extension(ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, peer],
            45678,
        ))))
        .header("content-type", "application/json")
        .header("origin", ORIGIN)
        .header("x-example-request", "same-origin");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    if let Some(csrf) = csrf {
        builder = builder.header("x-csrf-token", csrf);
    }
    builder
        .body(Body::from(
            serde_json::to_vec(value).map_err(Error::internal)?,
        ))
        .map_err(Error::internal)
}
fn cookie(response: &TestResponse) -> TestResult<String> {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|v| {
            v.starts_with("example_account_session=")
                .then(|| v.split(';').next().map(str::to_owned))
                .flatten()
        })
        .ok_or_else(|| "missing session cookie".into())
}
async fn login_session(fixture: &Fixture) -> TestResult<(String, String)> {
    let response = fixture
        .post("/session/login", &credentials(), None, None, None)
        .await?;
    assert_eq!(response.status(), StatusCode::OK, "session login");
    response
        .assert_status(StatusCode::OK)?
        .assert_header("cache-control", "no-store")?;
    let body: serde_json::Value = response.json()?;
    Ok((
        cookie(&response)?,
        body["data"]["csrf_token"]
            .as_str()
            .ok_or("missing CSRF token")?
            .into(),
    ))
}

async fn assert_session_auth_attributes(
    fixture: &Fixture,
    session: Option<&str>,
    authenticated: bool,
) -> TestResult {
    let response = fixture
        .app
        .request(request(
            "GET",
            "/session/status",
            &serde_json::Value::Null,
            None,
            session,
            None,
        )?)
        .await?;
    response
        .assert_status(StatusCode::OK)?
        .assert_header("cache-control", "no-store")?;
    let body: serde_json::Value = response.json()?;
    assert_eq!(
        body["data"],
        serde_json::json!({
            "guard_authenticated": authenticated,
            "authenticated": authenticated,
            "current_user": authenticated,
            "default_user": authenticated,
        })
    );
    Ok(())
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "同一安全业务流程的断言、真实资源与回收保持在一个测试生命周期内"
)]
async fn session_login_csrf_preferences_logout_and_origin_are_enforced() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let mut bad = request("POST", "/session/login", &credentials(), None, None, None)?;
    bad.headers_mut()
        .insert("origin", "https://evil.test".parse()?);
    let response = fixture.app.request(bad).await?;
    assert_eq!(response.status(), StatusCode::FORBIDDEN, "origin rejection");
    response.assert_header("cache-control", "no-store")?;
    let mut bad = request("POST", "/session/login", &credentials(), None, None, None)?;
    bad.headers_mut().remove("x-example-request");
    let response = fixture.app.request(bad).await?;
    assert_eq!(response.status(), StatusCode::FORBIDDEN, "header rejection");
    response.assert_header("cache-control", "no-store")?;
    let mut form = request("POST", "/session/login", &credentials(), None, None, None)?;
    form.headers_mut()
        .insert("content-type", "application/x-www-form-urlencoded".parse()?);
    let response = fixture.app.request(form).await?;
    assert!(!response.status().is_success());
    response.assert_header("cache-control", "no-store")?;
    let response = fixture
        .post(
            "/session/login",
            &serde_json::json!({"email":"not-an-email","password":PASSWORD}),
            None,
            None,
            None,
        )
        .await?;
    assert!(!response.status().is_success());
    response.assert_header("cache-control", "no-store")?;
    let response = fixture
        .app
        .request(request(
            "GET",
            "/session/preference",
            &serde_json::Value::Null,
            None,
            None,
            None,
        )?)
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    response.assert_header("cache-control", "no-store")?;
    assert_session_auth_attributes(&fixture, None, false).await?;
    let (session, csrf) = login_session(&fixture).await?;
    assert_session_auth_attributes(&fixture, Some(&session), true).await?;
    let response = fixture
        .post(
            "/session/preference",
            &serde_json::json!({"density":"compact"}),
            None,
            Some(&session),
            None,
        )
        .await?;
    assert_eq!(
        response.status(),
        StatusCode::from_u16(419)?,
        "missing csrf"
    );
    response.assert_header("cache-control", "no-store")?;
    let response = fixture
        .post(
            "/session/preference",
            &serde_json::json!({"density":"compact"}),
            None,
            Some(&session),
            Some("wrong-csrf"),
        )
        .await?;
    assert_eq!(response.status(), StatusCode::from_u16(419)?, "wrong csrf");
    let response = fixture
        .post(
            "/session/preference",
            &serde_json::json!({"density":"compact"}),
            None,
            Some(&session),
            Some(&csrf),
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK, "valid csrf");
    let response = fixture
        .app
        .request(request(
            "GET",
            "/session/preference",
            &serde_json::Value::Null,
            None,
            Some(&session),
            None,
        )?)
        .await?;
    let body: serde_json::Value = response.json()?;
    assert_eq!(body["data"]["density"], "compact");
    let response = fixture
        .post(
            "/session/logout",
            &serde_json::json!({}),
            None,
            Some(&session),
            Some(&csrf),
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK, "session logout");
    let logged_out_session = cookie(&response)?;
    assert_session_auth_attributes(&fixture, Some(&logged_out_session), false).await?;
    let response = fixture
        .app
        .request(request(
            "GET",
            "/session/preference",
            &serde_json::Value::Null,
            None,
            Some(&session),
            None,
        )?)
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "after logout");
    let response = fixture
        .post(
            "/session/login",
            &credentials(),
            None,
            Some(&logged_out_session),
            None,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let renewed_session = cookie(&response)?;
    let body: serde_json::Value = response.json()?;
    let renewed_csrf = body["data"]["csrf_token"]
        .as_str()
        .ok_or("missing renewed CSRF token")?;
    assert_ne!(renewed_csrf, csrf, "注销必须轮换标准 Session token");
    let response = fixture
        .app
        .request(request(
            "GET",
            "/session/preference",
            &serde_json::Value::Null,
            None,
            Some(&renewed_session),
            None,
        )?)
        .await?;
    response.assert_status(StatusCode::OK)?;
    let body: serde_json::Value = response.json()?;
    assert_eq!(body["data"]["density"], "compact", "认证轮换保留应用偏好");
    fixture
        .post(
            "/session/preference",
            &serde_json::json!({"density":"compact"}),
            None,
            Some(&renewed_session),
            Some(&csrf),
        )
        .await?
        .assert_status(StatusCode::from_u16(419)?)?;
    fixture.close().await
}

#[tokio::test]
async fn disabled_account_rejects_existing_named_session_on_new_requests() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let (session, csrf) = login_session(&fixture).await?;
    let read = || {
        request(
            "GET",
            "/session/preference",
            &serde_json::Value::Null,
            None,
            Some(&session),
            None,
        )
    };
    fixture
        .app
        .request(read()?)
        .await?
        .assert_status(StatusCode::OK)?;
    // 修改真实隔离库；后续每次请求都必须经 app-users 重新加载账户状态。
    let db = fixture
        .app
        .app()?
        .state()
        .resolve::<openrust::database::Db>()
        .await?;
    let affected = db
        .table("users")?
        .where_eq("email", EMAIL)?
        .update(vec![("disabled_at", chrono::Utc::now().naive_utc().into())])
        .await?;
    assert_eq!(affected, 1, "必须实际禁用已登录账户");
    fixture
        .app
        .request(read()?)
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?
        .assert_header("cache-control", "no-store")?;
    // 提交正确 CSRF 也不得绕过账户禁用；此处应是认证拒绝而非 419。
    fixture
        .post(
            "/session/preference",
            &serde_json::json!({"density":"compact"}),
            None,
            Some(&session),
            Some(&csrf),
        )
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?
        .assert_header("cache-control", "no-store")?;
    drop(db);
    fixture.close().await
}

#[cfg(feature = "auth-fortify")]
fn authenticator_code(encoded: &str) -> TestResult<String> {
    use hmac::{Hmac, Mac, digest::KeyInit};
    let (mut bits, mut buffer, mut secret) = (0_u8, 0_u32, Vec::new());
    for byte in encoded.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'2'..=b'7' => byte - b'2' + 26,
            _ => return Err("invalid authenticator encoding".into()),
        };
        buffer = ((buffer << 5) | u32::from(value)) & 0xffff;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            secret.push(u8::try_from((buffer >> bits) & 0xff)?);
        }
    }
    let step = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        / 30;
    let mut mac = <Hmac<sha1::Sha1> as KeyInit>::new_from_slice(&secret)?;
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[19] & 0x0f);
    let binary = (u32::from(digest[offset] & 0x7f) << 24)
        | (u32::from(digest[offset + 1]) << 16)
        | (u32::from(digest[offset + 2]) << 8)
        | u32::from(digest[offset + 3]);
    Ok(format!("{:06}", binary % 1_000_000))
}
#[cfg(feature = "auth-fortify")]
pub(super) async fn setup_confirmed_factor(
    scope: &openrust::container::Scope,
    id: i64,
) -> TestResult<(
    std::sync::Arc<openrust::auth::fortify::TwoFactorActions>,
    openrust::auth::AuthUser,
)> {
    let actions = scope
        .resolve::<openrust::auth::fortify::TwoFactorActions>()
        .await?;
    let provider = scope
        .resolve::<openrust::auth::AuthManager>()
        .await?
        .create_user_provider(Some("users"))
        .await?
        .ok_or("missing account provider")?;
    let user = provider
        .retrieve_by_id(&serde_json::json!(id))
        .await?
        .ok_or("missing account")?;
    actions.enable(&user, false).await?;
    let secret = actions.secret_key(&user).await?;
    actions
        .confirm(&user, &authenticator_code(&secret)?)
        .await?;
    Ok((actions, user))
}
#[cfg(feature = "auth-fortify")]
async fn begin_challenge(fixture: &Fixture) -> TestResult<String> {
    let response = fixture
        .post("/login", &credentials(), None, None, None)
        .await?;
    response
        .assert_status(StatusCode::OK)?
        .assert_header("cache-control", "no-store")?;
    let body: serde_json::Value = response.json()?;
    assert_eq!(body["data"]["status"], "two_factor_required");
    assert!(body["data"].get("credentials").is_none());
    Ok(body["data"]["challenge"]
        .as_str()
        .ok_or("missing two-factor challenge")?
        .into())
}
#[cfg(feature = "auth-fortify")]
async fn management_requires_a_current_password_confirmation(
    fixture: &Fixture,
    cookie: &str,
    csrf: &str,
) -> TestResult {
    let read = |cookie: Option<&str>| {
        let mut request = Request::get("/user/two-factor-recovery-codes")
            .header("host", "example.test")
            .header("accept", "application/json");
        if let Some(cookie) = cookie {
            request = request.header("cookie", cookie);
        }
        request.body(Body::empty())
    };
    fixture
        .app
        .request(read(None)?)
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    fixture
        .app
        .request(read(Some(cookie))?)
        .await?
        .assert_status(StatusCode::LOCKED)?;
    let wrong = Request::post("/user/confirm-password")
        .header("host", "example.test")
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .header("cookie", cookie)
        .header("x-csrf-token", csrf)
        .body(Body::from(serde_json::to_vec(
            &serde_json::json!({"password":"wrong-password"}),
        )?))?;
    fixture
        .app
        .request(wrong)
        .await?
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY)?;
    fixture
        .app
        .request(read(Some(cookie))?)
        .await?
        .assert_status(StatusCode::LOCKED)?;
    Ok(())
}
#[cfg(feature = "auth-fortify")]
#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "同一安全业务流程的断言、真实资源与回收保持在一个测试生命周期内"
)]
async fn selected_factor_blocks_password_only_resources_and_consumes_recovery_once() -> TestResult {
    use openrust::events::{EventPropagation, EventsServiceProvider};
    use openrust::fortify::events::TwoFactorAuthenticationEnabled;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let enable_count = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&enable_count);
    let Some(fixture) = Fixture::create_with_providers(move |providers| {
        let mut listeners = EventsServiceProvider::new();
        let observed = Arc::clone(&observed);
        listeners.listen_named::<TwoFactorAuthenticationEnabled, _>(
            r"OpenRust\Fortify\Events\TwoFactorAuthenticationEnabled",
            move |_, context| {
                let observed = Arc::clone(&observed);
                Box::pin(async move {
                    // 普通启用在写入完成后派发；API 管理事务另有回滚回归。
                    assert!(context.transaction().is_none());
                    observed.fetch_add(1, Ordering::Relaxed);
                    Ok(EventPropagation::Continue)
                })
            },
        );
        // 示例模块先登记同类型 Provider。此 fixture 替换监听清单，继续使用应用事件总线。
        providers.force_provider(listeners);
    })
    .await?
    else {
        return Ok(());
    };
    let (old_session, csrf) = login_session(&fixture).await?;
    management_requires_a_current_password_confirmation(&fixture, &old_session, &csrf).await?;
    let scope = fixture.app.app()?.state().container().scope()?;
    let (actions, user) = setup_confirmed_factor(&scope, fixture.user_id).await?;
    assert_eq!(enable_count.load(Ordering::Relaxed), 1);
    let codes = actions.recovery_codes(&user).await?;
    assert_eq!(codes.len(), 8);
    let first = codes.first().ok_or("missing recovery code")?;
    // 已有 Session 和只有密码的 Session 登录必须全部失效，不能绕过第二因子。
    fixture
        .app
        .request(request(
            "GET",
            "/session/preference",
            &serde_json::Value::Null,
            None,
            Some(&old_session),
            None,
        )?)
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    let rejected_password_session = fixture
        .post("/session/login", &credentials(), None, None, None)
        .await?;
    rejected_password_session.assert_status(StatusCode::FORBIDDEN)?;
    assert_eq!(
        rejected_password_session.json::<serde_json::Value>()?["code"],
        "auth_two_factor_required"
    );
    // 原密码登录 API 也必须 fail-closed，不能从旧入口获得完整凭据。
    let password_only = Request::post("/api/example/auth/login")
        .header("host", "example.test")
        .extension(ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 50],
            45678,
        ))))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&credentials())?))?;
    let password_only = fixture.app.request(password_only).await?;
    trace_response("POST", "/api/example/auth/login", &password_only);
    assert!(!password_only.status().is_success());
    let password_only: serde_json::Value = password_only.json()?;
    assert_eq!(password_only["code"], "auth_two_factor_required");
    let challenge = begin_challenge(&fixture).await?;
    let mut wrong_source = request(
        "POST",
        "/login/two-factor",
        &serde_json::json!({"challenge":challenge,"recovery_code":first}),
        None,
        None,
        None,
    )?;
    wrong_source
        .extensions_mut()
        .insert(ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 99],
            45678,
        ))));
    let rejected = fixture.app.request(wrong_source).await?;
    trace_response("POST", "/login/two-factor (different source)", &rejected);
    rejected.assert_status(StatusCode::UNAUTHORIZED)?;
    let completed = fixture
        .post(
            "/login/two-factor",
            &serde_json::json!({"challenge":challenge,"recovery_code":first}),
            None,
            None,
            None,
        )
        .await?;
    completed
        .assert_status(StatusCode::OK)?
        .assert_header("cache-control", "no-store")?;
    fixture
        .post(
            "/login/two-factor",
            &serde_json::json!({"challenge":challenge,"recovery_code":first}),
            None,
            None,
            None,
        )
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    let next = begin_challenge(&fixture).await?;
    fixture
        .post(
            "/login/two-factor",
            &serde_json::json!({"challenge":next,"recovery_code":first}),
            None,
            None,
            None,
        )
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    let db = fixture.app.app()?.state().resolve::<openrust::Db>().await?;
    assert_eq!(
        db.table("audit_records")?
            .where_eq("action", "auth.two_factor.login")?
            .count()
            .await?,
        1
    );
    let replaced = actions.recovery_codes(&user).await?;
    assert_eq!(replaced.len(), 8);
    assert!(!replaced.contains(first));
    let issued: serde_json::Value = completed.json()?;
    let bearer = issued["data"]["access_token"]
        .as_str()
        .ok_or("missing access token")?;
    let refreshed = issued["data"]["refresh_token"]
        .as_str()
        .ok_or("missing refresh token")?;
    actions.generate_new_recovery_codes(&user).await?;
    let rotated = actions.recovery_codes(&user).await?;
    assert_eq!(rotated.len(), 8);
    assert!(replaced.iter().all(|old| !rotated.contains(old)));
    fixture
        .app
        .request(
            Request::get("/api/example/me")
                .header("host", "example.test")
                .header("authorization", format!("Bearer {bearer}"))
                .body(Body::empty())?,
        )
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    let flows = fixture.app.app()?.state().resolve::<AuthFlows>().await?;
    assert!(flows.refresh(refreshed, "rotated-refresh").await.is_err());
    let challenge = begin_challenge(&fixture).await?;
    fixture
        .post(
            "/login/two-factor",
            &serde_json::json!({"challenge":challenge,"recovery_code":codes[1]}),
            None,
            None,
            None,
        )
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    fixture
        .post(
            "/login/two-factor",
            &serde_json::json!({"challenge":challenge,"recovery_code":rotated[0]}),
            None,
            None,
            None,
        )
        .await?
        .assert_status(StatusCode::OK)?;
    assert_eq!(
        db.table("audit_records")?
            .where_eq("action", "auth.two_factor.login")?
            .count()
            .await?,
        2
    );
    actions.disable(&user).await?;
    drop(actions);
    drop(scope);
    let result = fixture
        .post("/login", &credentials(), None, None, None)
        .await?;
    result.assert_status(StatusCode::OK)?;
    assert_eq!(
        result.json::<serde_json::Value>()?["data"]["status"],
        "authenticated"
    );
    fixture.close().await
}

// 唯一模块正式登录入口：能力检查变更live默认时，本次仍使用已选最终叶。
mod hash_operation_http {
    use super::{EMAIL, Fixture, TestResult};
    use futures_util::FutureExt as _;
    use openrust::config::repository::ConfigRepository;
    use openrust::database::Db;
    use openrust::hashing::{ArgonConfig, ArgonHasher, HashInfo, HashManager, HashOptions, Hasher};
    use openrust::http::axum::http::StatusCode;
    use openrust::support::BoxFuture;
    use openrust::{Error, Result};
    use serde_json::json;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Default)]
    struct Counts {
        capabilities: AtomicUsize,
        makes: AtomicUsize,
        checks: AtomicUsize,
    }
    impl Counts {
        fn kdf(&self) -> (usize, usize) {
            (
                self.makes.load(Ordering::SeqCst),
                self.checks.load(Ordering::SeqCst),
            )
        }
    }
    struct Driver {
        argon: Arc<ArgonHasher>,
        prefix_72: bool,
        counts: Arc<Counts>,
        switch: Mutex<Option<ConfigRepository>>,
    }
    impl Driver {
        fn input<'a>(&self, value: &'a [u8]) -> &'a [u8] {
            if self.prefix_72 {
                &value[..value.len().min(72)]
            } else {
                value
            }
        }
    }
    impl Hasher for Driver {
        fn accepts_exact_input(&self, value: &[u8]) -> Result<bool> {
            self.counts.capabilities.fetch_add(1, Ordering::SeqCst);
            let repository = self.switch.lock().map_err(|_| state_error())?.take();
            if let Some(repository) = repository {
                repository.set("hashing.driver", json!("b"))?;
            }
            Ok(!self.prefix_72 || value.len() <= 72)
        }
        fn info(&self, encoded: &str) -> Result<HashInfo> {
            self.argon.info(encoded)
        }
        fn make<'a>(
            &'a self,
            value: &'a [u8],
            options: &'a HashOptions,
        ) -> BoxFuture<'a, Result<String>> {
            Box::pin(async move {
                self.counts.makes.fetch_add(1, Ordering::SeqCst);
                self.argon.make(self.input(value), options).await
            })
        }
        fn check<'a>(
            &'a self,
            value: &'a [u8],
            encoded: Option<&'a str>,
            options: &'a HashOptions,
        ) -> BoxFuture<'a, Result<bool>> {
            Box::pin(async move {
                self.counts.checks.fetch_add(1, Ordering::SeqCst);
                self.argon.check(self.input(value), encoded, options).await
            })
        }
        fn needs_rehash(&self, encoded: &str, options: &HashOptions) -> Result<bool> {
            self.argon.needs_rehash(encoded, options)
        }
    }
    fn state_error() -> Error {
        Error::custom(
            "example_hash_operation_fixture",
            "模块哈希操作fixture状态无效",
        )
    }
    fn install(manager: &HashManager, name: &str, driver: Arc<dyn Hasher>) -> Result<()> {
        manager.extend(name, move || Ok(Arc::clone(&driver)))
    }
    struct Scenario {
        db: Arc<Db>,
        a: Arc<Driver>,
        ac: Arc<Counts>,
        bc: Arc<Counts>,
        inner_config: ConfigRepository,
        password: String,
        encoded: String,
    }
    async fn prepare(fixture: &Fixture) -> TestResult<Scenario> {
        let app = fixture.app.app()?;
        let state = app.state();
        let db = state.resolve::<Db>().await?;
        let outer = state.resolve::<HashManager>().await?;
        let config = app.config_repository();
        let inner_config = ConfigRepository::new(figment::Figment::new());
        inner_config.set("hashing.driver", json!("a"))?;
        let inner = Arc::new(HashManager::from_repository(inner_config.clone(), 2)?);
        let argon = Arc::new(ArgonHasher::new(ArgonConfig {
            memory: 1024,
            time: 1,
            threads: 1,
            verify: true,
        })?);
        let ac = Arc::new(Counts::default());
        let bc = Arc::new(Counts::default());
        let a = Arc::new(Driver {
            argon: Arc::clone(&argon),
            prefix_72: false,
            counts: Arc::clone(&ac),
            switch: Mutex::new(None),
        });
        let b = Arc::new(Driver {
            argon,
            prefix_72: true,
            counts: Arc::clone(&bc),
            switch: Mutex::new(None),
        });
        install(&inner, "a", a.clone())?;
        install(&inner, "b", b)?;
        install(&outer, "ha-module-nested", inner)?;
        config.set("hashing.driver", json!("ha-module-nested"))?;
        let password = "h".repeat(73);
        let encoded = a.make(password.as_bytes(), &HashOptions::new()).await?;
        assert!(
            a.check(password.as_bytes(), Some(&encoded), &HashOptions::new())
                .await?
        );
        assert!(
            !a.check(
                &password.as_bytes()[..72],
                Some(&encoded),
                &HashOptions::new()
            )
            .await?
        );
        assert_eq!(ac.capabilities.load(Ordering::SeqCst), 0);
        assert_eq!(bc.kdf(), (0, 0));
        assert_eq!(
            db.table("users")?
                .where_eq("email", EMAIL)?
                .update(vec![("password_hash", encoded.as_str().into())])
                .await?,
            1
        );
        assert_eq!(stored_hash(&db).await?, encoded);
        // seed及真实73/72比较完成后才武装切换；第一次执行必须由HTTP增强登录触发。
        *a.switch.lock().map_err(|_| state_error())? = Some(inner_config.clone());
        Ok(Scenario {
            db,
            a,
            ac,
            bc,
            inner_config,
            password,
            encoded,
        })
    }
    async fn stored_hash(db: &Db) -> Result<String> {
        db.table("users")?
            .where_eq("email", EMAIL)?
            .value::<String>("password_hash")
            .await?
            .ok_or_else(state_error)
    }
    async fn exercise(fixture: &Fixture) -> TestResult {
        let scenario = prepare(fixture).await?;
        let baseline = scenario.ac.kdf();
        let refresh_before = scenario.db.table("auth_refresh_tokens")?.count().await?;
        let response = fixture
            .post(
                "/login",
                &json!({"email":EMAIL,"password":scenario.password}),
                None,
                None,
                None,
            )
            .await?;
        response
            .assert_status(StatusCode::OK)?
            .assert_header("cache-control", "no-store")?;
        let body: serde_json::Value = response.json()?;
        assert_eq!(body["data"]["status"], "authenticated");
        assert!(
            body["data"]["credentials"]["access_token"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
        assert!(
            body["data"]["credentials"]["refresh_token"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
        assert_eq!(scenario.ac.kdf(), (baseline.0, baseline.1 + 1));
        assert_eq!(scenario.bc.kdf(), (0, 0));
        assert_eq!(scenario.ac.capabilities.load(Ordering::SeqCst), 2);
        assert_eq!(
            scenario.inner_config.section::<String>("hashing.driver")?,
            "b"
        );
        let refresh_after_success = scenario.db.table("auth_refresh_tokens")?.count().await?;
        assert_eq!(refresh_after_success, refresh_before + 1);
        let a_after_success = scenario.ac.kdf();
        let rejected = fixture
            .post(
                "/login",
                &json!({"email":EMAIL,"password":scenario.password}),
                None,
                None,
                None,
            )
            .await?;
        rejected.assert_status(StatusCode::BAD_REQUEST)?;
        let body: serde_json::Value = rejected.json()?;
        assert_eq!(body["code"], "password_bcrypt_input_ambiguous");
        assert!(body["data"].is_null());
        assert!(!String::from_utf8_lossy(rejected.body()).contains(&scenario.password));
        assert_eq!(scenario.ac.kdf(), a_after_success);
        assert_eq!(scenario.bc.kdf(), (0, 0));
        assert_eq!(scenario.bc.capabilities.load(Ordering::SeqCst), 1);
        assert_eq!(
            scenario.db.table("auth_refresh_tokens")?.count().await?,
            refresh_after_success
        );
        assert_eq!(stored_hash(&scenario.db).await?, scenario.encoded);
        assert!(
            scenario
                .a
                .switch
                .lock()
                .map_err(|_| state_error())?
                .is_none()
        );
        Ok(())
    }
    async fn finish(mut fixture: Fixture, outcome: std::thread::Result<TestResult>) -> TestResult {
        let shutdown = fixture
            .app
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await;
        let close = fixture.database.close().await;
        if let Err(error) = &shutdown {
            eprintln!("HA模块HTTP shutdown secondary code={}", error.code());
        }
        if let Err(error) = &close {
            eprintln!("HA模块HTTP database.close secondary code={}", error.code());
        }
        match outcome {
            Ok(primary) => {
                primary?;
                shutdown?;
                close?;
                Ok(())
            }
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
    #[tokio::test]
    #[ignore = "真实MySQL8.4模块HTTP验收：须显式配置OPENRUST_TEST_DATABASE_URL并专跑"]
    async fn account_login_keeps_nested_selected_leaf_then_rejects_next_without_credentials()
    -> TestResult {
        let admin = std::env::var("OPENRUST_TEST_DATABASE_URL")?;
        if !admin.starts_with("mysql://") {
            return Err("HA模块HTTP需要真实MySQL URL".into());
        }
        let fixture = Fixture::create().await?.ok_or("真实MySQL fixture未创建")?;
        let outcome = std::panic::AssertUnwindSafe(exercise(&fixture))
            .catch_unwind()
            .await;
        finish(fixture, outcome).await
    }
}
