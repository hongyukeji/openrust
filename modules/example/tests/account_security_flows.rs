//! 生产模块账户安全 HTTP 回归；进程内请求不冒充 TCP/Redis 验收。
#![cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]

use figment::{Figment, providers::Serialized};
use openrust::auth::AuthFlows;
#[cfg(feature = "auth-fortify")]
use openrust::auth::JwtTokens;
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
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
#[openrust::db::sea_orm_migration::async_trait::async_trait]
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
        let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
            eprintln!("skipping account security DB test: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(None);
        };
        let database = TestDatabase::create::<Migrator>(&admin).await?;
        let mut framework = openrust::config::FrameworkConfig::default();
        framework.api.http_status = openrust::config::HttpStatusMode::Semantic;
        let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
        let source=Figment::from(Serialized::defaults(framework)).merge(Serialized::defaults(serde_json::json!({
            "app":{"env":"test","url":ORIGIN},"database":{"url":database.url()},
            "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
            "auth":{"defaults":{"guard":"web","passwords":"users","provider":null},"guards":{"web":{"driver":"session","provider":"users"}},"providers":{"users":{"driver":"app-users","table":"users"}},"jwt":{"issuer":ORIGIN,"audience":"account-tests","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":ORIGIN,"active_key_id":"test","keys":{"test":key}}},
            "session":{"secure":false,"cookie":"example_account_session","same_site":"strict"}
        })));
        let app = TestApp::<crate::bootstrap::Server>::builder(
            crate::modules::example::test_sources(&(source))?,
        )
        .build()
        .await?;
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
    let (session, csrf) = login_session(&fixture).await?;
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
        .resolve::<openrust::db::Db>()
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
    app: &openrust::App,
    id: i64,
) -> TestResult<(
    std::sync::Arc<openrust::auth::fortify::TwoFactorActions>,
    openrust::auth::AuthUser,
)> {
    let scope = app.state().container().scope()?;
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
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let (old_session, csrf) = login_session(&fixture).await?;
    management_requires_a_current_password_confirmation(&fixture, &old_session, &csrf).await?;
    let (actions, user) = setup_confirmed_factor(fixture.app.app()?, fixture.user_id).await?;
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
