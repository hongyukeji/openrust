//! 生产模块账户安全 HTTP 回归；进程内请求不冒充 TCP/Redis 验收。
#![cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]

use figment::{Figment, providers::Serialized};
use openrust::auth::AuthFlows;
#[cfg(feature = "auth-mfa")]
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
        let mut migrations = openrust::db::FrameworkMigrator::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}
struct Fixture {
    app: TestApp<crate::bootstrap::Server>,
    database: TestDatabase,
    #[cfg(feature = "auth-mfa")]
    token: String,
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
            "cache":{"default_store":"default","stores":{"default":{"driver":"memory"}}},
            "auth":{"jwt":{"issuer":ORIGIN,"audience":"account-tests","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":ORIGIN,"active_key_id":"test","keys":{"test":key}}},
            "session":{"secure":false,"cookie":"example_account_session","same_site":"strict"}
        })));
        let app = TestApp::<crate::bootstrap::Server>::builder(source)
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
        #[cfg(feature = "auth-mfa")]
        let credentials = flows.login(EMAIL, PASSWORD, "fixture-seed").await?;
        #[cfg(feature = "auth-mfa")]
        let token = app
            .app()?
            .state()
            .resolve::<JwtTokens>()
            .await?
            .issue_access_token(credentials.user_id())?;
        Ok(Some(Self {
            app,
            database,
            #[cfg(feature = "auth-mfa")]
            token,
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
        self.app
            .request(request("POST", path, value, token, cookie, csrf)?)
            .await
    }
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
    let peer = match path {
        "/session/login" => 10,
        "/mfa/enroll" => 20,
        "/mfa/confirm" => 21,
        "/mfa/recovery" => 22,
        "/mfa/disable" => 23,
        _ => 24,
    };
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
    assert_eq!(response.status(), StatusCode::FORBIDDEN, "missing csrf");
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
    assert_eq!(response.status(), StatusCode::FORBIDDEN, "wrong csrf");
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
    fixture.close().await
}

#[cfg(feature = "auth-mfa")]
pub(super) fn authenticator_code(uri: &str) -> TestResult<String> {
    // 仅测试客户端模拟外部 RFC 6238 验证器；生产业务不读取 seed 或实现算法。
    use hmac::{Hmac, Mac, digest::KeyInit};
    let uri = url::Url::parse(uri)?;
    let encoded = uri
        .query_pairs()
        .find_map(|(key, value)| (key == "secret").then(|| value.into_owned()))
        .ok_or("missing authenticator secret")?;
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
#[cfg(feature = "auth-mfa")]
async fn begin_challenge(fixture: &Fixture) -> TestResult<String> {
    let response = fixture
        .post("/login", &credentials(), None, None, None)
        .await?;
    response
        .assert_status(StatusCode::OK)?
        .assert_header("cache-control", "no-store")?;
    let body: serde_json::Value = response.json()?;
    assert_eq!(body["data"]["status"], "mfa_required");
    assert!(body["data"].get("credentials").is_none());
    Ok(body["data"]["challenge"]
        .as_str()
        .ok_or("missing MFA challenge")?
        .into())
}
#[cfg(feature = "auth-mfa")]
#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "同一安全业务流程的断言、真实资源与回收保持在一个测试生命周期内"
)]
async fn mfa_requires_reauthentication_challenge_and_once_only_recovery_codes() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let (old_session, _csrf) = login_session(&fixture).await?;
    fixture
        .post("/mfa/enroll", &credentials(), None, None, None)
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    fixture
        .post(
            "/mfa/enroll",
            &serde_json::json!({"email":EMAIL,"password":"wrong password"}),
            Some(&fixture.token),
            None,
            None,
        )
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    let enrollment = fixture
        .post(
            "/mfa/enroll",
            &credentials(),
            Some(&fixture.token),
            None,
            None,
        )
        .await?;
    enrollment
        .assert_status(StatusCode::OK)?
        .assert_header("cache-control", "no-store")?;
    let body: serde_json::Value = enrollment.json()?;
    let uri = body["data"]["provisioning_uri"]
        .as_str()
        .ok_or("missing provisioning URI")?;
    let code = authenticator_code(uri)?;
    fixture
        .post(
            "/mfa/confirm",
            &serde_json::json!({"email":EMAIL,"password":"wrong password","code":code}),
            Some(&fixture.token),
            None,
            None,
        )
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    let response = fixture
        .post(
            "/mfa/confirm",
            &serde_json::json!({"email":EMAIL,"password":PASSWORD,"code":code}),
            Some(&fixture.token),
            None,
            None,
        )
        .await?;
    response
        .assert_status(StatusCode::OK)?
        .assert_header("cache-control", "no-store")?;
    let body: serde_json::Value = response.json()?;
    let codes = body["data"]["recovery_codes"]
        .as_array()
        .ok_or("missing recovery codes")?;
    assert_eq!(codes.len(), 10);
    let first = codes[0].as_str().ok_or("missing recovery code")?;
    let second = codes[1].as_str().ok_or("missing recovery code")?;
    let unused_old = codes[2].as_str().ok_or("missing recovery code")?;
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
    fixture
        .post("/session/login", &credentials(), None, None, None)
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    // 原密码登录 API 也必须 fail-closed，不能从旧入口获得完整凭据。
    let legacy = Request::post("/api/example/auth/login")
        .header("host", "example.test")
        .extension(ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 50],
            45678,
        ))))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&credentials())?))?;
    let legacy = fixture.app.request(legacy).await?;
    assert!(!legacy.status().is_success());
    let legacy: serde_json::Value = legacy.json()?;
    assert_eq!(legacy["code"], "auth_mfa_required");
    let challenge = begin_challenge(&fixture).await?;
    let mut wrong_source = request(
        "POST",
        "/login/mfa",
        &serde_json::json!({"challenge":challenge,"code":first}),
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
    fixture
        .app
        .request(wrong_source)
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    let completed = fixture
        .post(
            "/login/mfa",
            &serde_json::json!({"challenge":challenge,"code":first}),
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
            "/login/mfa",
            &serde_json::json!({"challenge":challenge,"code":first}),
            None,
            None,
            None,
        )
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    let next = begin_challenge(&fixture).await?;
    fixture
        .post(
            "/login/mfa",
            &serde_json::json!({"challenge":next,"code":first}),
            None,
            None,
            None,
        )
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    let replacement = fixture
        .post(
            "/mfa/recovery",
            &serde_json::json!({"email":EMAIL,"password":PASSWORD,"code":second}),
            Some(&fixture.token),
            None,
            None,
        )
        .await?;
    replacement.assert_status(StatusCode::OK)?;
    let replacement: serde_json::Value = replacement.json()?;
    let old_codes_challenge = begin_challenge(&fixture).await?;
    fixture
        .post(
            "/login/mfa",
            &serde_json::json!({"challenge":old_codes_challenge,"code":unused_old}),
            None,
            None,
            None,
        )
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    let fresh = replacement["data"]["recovery_codes"][0]
        .as_str()
        .ok_or("missing rotated recovery code")?;
    fixture
        .post(
            "/mfa/disable",
            &serde_json::json!({"email":EMAIL,"password":"wrong password","code":fresh}),
            Some(&fixture.token),
            None,
            None,
        )
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    fixture
        .post(
            "/mfa/disable",
            &serde_json::json!({"email":EMAIL,"password":PASSWORD,"code":fresh}),
            Some(&fixture.token),
            None,
            None,
        )
        .await?
        .assert_status(StatusCode::OK)?;
    login_session(&fixture).await?;
    fixture.close().await
}
