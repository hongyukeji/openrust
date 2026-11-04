//! 根标准Provider、唯一模块真实HTTP与公开AuthFlows的SQLite本地认证；不声称完整R7/OIDC。

use std::{future::Future, time::Duration};

#[cfg(feature = "auth-mfa")]
use openrust::database::sea_orm::ConnectionTrait;
use openrust::{
    Error, Result,
    auth::{AuthFlows, JwtTokens, PersonalAccessTokens, Policy},
    http::axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
    },
    testing::{TestApp, TestDatabase, TestResponse},
};
use serde_json::{Value, json};

const BUDGET: Duration = Duration::from_secs(20);
const ACCOUNT: &str = "/api/example/account";
const EMAIL: &str = "sqlite-root-auth@example.test";
const PASSWORD: &str = "correct horse battery staple";
const RESET_PASSWORD: &str = "new correct horse battery staple";
const COOKIE: &str = "example_sqlite_auth_session";

fn mismatch() -> Error {
    Error::custom("sqlite_root_auth_contract", "SQLite本地认证业务契约不匹配")
}
#[track_caller]
fn ensure(value: bool) -> Result<()> {
    if value {
        return Ok(());
    }
    let location = std::panic::Location::caller();
    eprintln!(
        "sqlite_native_contract assertion={}:{}",
        location.file(),
        location.line()
    );
    Err(mismatch())
}

async fn bounded<T>(future: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(BUDGET, Box::pin(future))
        .await
        .map_err(|_| {
            Error::custom(
                "sqlite_auth_timeout",
                "SQLite认证操作超时，可能未释放写所有权",
            )
        })?
}
fn string(value: &Value, key: &str) -> Result<String> {
    value[key].as_str().map(str::to_owned).ok_or_else(mismatch)
}
fn response_data(response: &TestResponse) -> Result<Value> {
    Ok(response.json::<Value>()?["data"].clone())
}

struct Fixture {
    app: TestApp<crate::bootstrap::Server>,
    database: TestDatabase,
}
impl Fixture {
    async fn create(memory: bool) -> Result<Self> {
        let database = if memory {
            TestDatabase::sqlite_memory::<super::EmptyMigrator>().await?
        } else {
            TestDatabase::sqlite_file::<super::EmptyMigrator>().await?
        };
        let source = super::sqlite_default_host_source(&database).merge(
            figment::providers::Serialized::defaults(json!({
                "app":{"url":"http://localhost"},
                "session":{"driver":"database","cookie":COOKIE,"secure":false,"lottery":[0,100]}
            })),
        );
        let sources = match super::sqlite_test_sources(&source) {
            Ok(sources) => sources,
            Err(error) => {
                let _ = database.close().await;
                return Err(error);
            }
        };
        // 同根bootstrap/Module Catalog的正式命令先迁移；不手工拼认证测试schema。
        if let Err(error) = bounded(
            TestApp::<crate::bootstrap::Server>::builder(sources.clone())
                .run_command("migrate", Vec::new()),
        )
        .await
        {
            let _ = database.close().await;
            return Err(error);
        }
        match bounded(TestApp::<crate::bootstrap::Server>::builder(sources).build()).await {
            Ok(app) => Ok(Self { app, database }),
            Err(error) => {
                let _ = database.close().await;
                Err(error)
            }
        }
    }
    async fn close(mut self) -> Result<()> {
        let shutdown = self
            .app
            .shutdown(tokio::time::Instant::now() + BUDGET)
            .await;
        let cleanup = self.database.close().await;
        shutdown?;
        cleanup
    }
    async fn flows(&self) -> Result<std::sync::Arc<AuthFlows>> {
        self.app.app()?.state().resolve::<AuthFlows>().await
    }
    async fn request(
        &self,
        method: &str,
        path: &str,
        input: &Value,
        token: Option<&str>,
        cookie: Option<&str>,
        csrf: Option<&str>,
    ) -> Result<TestResponse> {
        // 每个敏感端点独立受控来源，保留默认限流；login与complete仍用同源88。
        let peer = if path.ends_with("/mfa/enroll") {
            20
        } else if path.ends_with("/mfa/confirm") {
            21
        } else if path.ends_with("/session/login") {
            89
        } else {
            88
        };
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("origin", "http://localhost")
            .header("x-example-request", "same-origin")
            .extension(ConnectInfo(std::net::SocketAddr::from((
                [127, 0, 0, peer],
                45678,
            ))));
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        if let Some(cookie) = cookie {
            request = request.header("cookie", cookie);
        }
        if let Some(csrf) = csrf {
            request = request.header("x-csrf-token", csrf);
        }
        let request = request
            .body(Body::from(
                serde_json::to_vec(input).map_err(Error::internal)?,
            ))
            .map_err(Error::internal)?;
        self.app
            .request_with_limits(request, 128 * 1024, BUDGET)
            .await
    }
    async fn scalar(&self, sql: String) -> Result<i64> {
        super::scalar(&self.database.connection()?, &sql).await
    }
    async fn provision(&self) -> Result<i64> {
        let flows = self.flows().await?;
        let code = bounded(flows.register(EMAIL, PASSWORD, "sqlite-fixture"))
            .await?
            .ok_or_else(mismatch)?;
        ensure(
            bounded(flows.register(EMAIL, PASSWORD, "sqlite-fixture-duplicate"))
                .await?
                .is_none(),
        )?;
        ensure(bounded(flows.verify_email(EMAIL, code.code(), "sqlite-fixture")).await?)?;
        ensure(!bounded(flows.verify_email(EMAIL, code.code(), "sqlite-fixture-replay")).await?)?;
        let login = bounded(flows.login(EMAIL, PASSWORD, "sqlite-fixture-login")).await?;
        ensure(login.user_id() > 0)?;
        Ok(login.user_id())
    }
}
fn credentials(password: &str) -> Value {
    json!({"email":EMAIL,"password":password})
}
fn cookie(response: &TestResponse) -> Result<String> {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find_map(|value| {
            value
                .strip_prefix(&format!("{COOKIE}="))
                .and_then(|_| value.split(';').next())
                .map(str::to_owned)
        })
        .ok_or_else(mismatch)
}
async fn native_session_contract(f: &Fixture) -> Result<()> {
    f.request(
        "POST",
        &format!("{ACCOUNT}/session/login"),
        &credentials("incorrect-password"),
        None,
        None,
        None,
    )
    .await?
    .assert_status(StatusCode::UNAUTHORIZED)?;
    let login = f
        .request(
            "POST",
            &format!("{ACCOUNT}/session/login"),
            &credentials(PASSWORD),
            None,
            None,
            None,
        )
        .await?;
    login
        .assert_status(StatusCode::OK)?
        .assert_header("cache-control", "no-store")?;
    let session = cookie(&login).inspect_err(|_| {
        let names = login
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| value.split('=').next())
            .collect::<Vec<_>>();
        eprintln!("sqlite_native_contract phase=cookie names={names:?}");
    })?;
    let csrf = string(&response_data(&login)?, "csrf_token")?;
    f.request(
        "GET",
        &format!("{ACCOUNT}/session/preference"),
        &Value::Null,
        None,
        Some(&session),
        None,
    )
    .await?
    .assert_status(StatusCode::OK)?;
    f.request(
        "POST",
        &format!("{ACCOUNT}/session/preference"),
        &json!({"density":"compact"}),
        None,
        Some(&session),
        None,
    )
    .await?
    .assert_status(StatusCode::FORBIDDEN)?;
    f.request(
        "POST",
        &format!("{ACCOUNT}/session/preference"),
        &json!({"density":"compact"}),
        None,
        Some(&session),
        Some("wrong-csrf"),
    )
    .await?
    .assert_status(StatusCode::FORBIDDEN)?;
    f.request(
        "POST",
        &format!("{ACCOUNT}/session/preference"),
        &json!({"density":"compact"}),
        None,
        Some(&session),
        Some(&csrf),
    )
    .await?
    .assert_status(StatusCode::OK)?;
    let response = f
        .request(
            "GET",
            &format!("{ACCOUNT}/session/preference"),
            &Value::Null,
            None,
            Some(&session),
            None,
        )
        .await?;
    ensure(response_data(&response)?["density"] == "compact")?;
    let db = f.app.app()?.state().resolve::<openrust::Db>().await?;
    let table = db.quoted_table_name("sessions")?;
    ensure(
        f.scalar(format!(
            "SELECT count(*) FROM {table} WHERE length(id)=40 AND last_activity>0"
        ))
        .await?
            > 0,
    )?;
    native_logout_contract(f, &session, &csrf).await
}

async fn native_logout_contract(f: &Fixture, session: &str, csrf: &str) -> Result<()> {
    f.request(
        "POST",
        &format!("{ACCOUNT}/session/logout"),
        &json!({}),
        None,
        Some(session),
        Some(csrf),
    )
    .await?
    .assert_status(StatusCode::OK)?;
    f.request(
        "GET",
        &format!("{ACCOUNT}/session/preference"),
        &Value::Null,
        None,
        Some(session),
        None,
    )
    .await?
    .assert_status(StatusCode::UNAUTHORIZED)?;
    Ok(())
}
async fn bearer_pat_rbac_and_refresh(f: &Fixture, user_id: i64) -> Result<()> {
    let flows = f.flows().await?;
    let credentials = bounded(flows.login(EMAIL, PASSWORD, "sqlite-bearer")).await?;
    f.request(
        "GET",
        "/api/example/me",
        &Value::Null,
        Some(credentials.access_token()),
        None,
        None,
    )
    .await?
    .assert_status(StatusCode::OK)?;
    f.request(
        "GET",
        "/api/example/me",
        &Value::Null,
        Some("invalid"),
        None,
        None,
    )
    .await?
    .assert_status(StatusCode::UNAUTHORIZED)?;
    let tokens = f
        .app
        .app()?
        .state()
        .resolve::<PersonalAccessTokens>()
        .await?;
    let token =
        bounded(tokens.issue(user_id, "SQLite module", &["articles.read".into()], None)).await?;
    let meta = bounded(tokens.list_for_user(user_id)).await?;
    let token_id = meta.first().ok_or_else(mismatch)?.id;
    f.request(
        "GET",
        "/api/example/me/pat",
        &Value::Null,
        Some(token.expose()),
        None,
        None,
    )
    .await?
    .assert_status(StatusCode::OK)?;
    ensure(bounded(tokens.revoke(user_id, token_id)).await?)?;
    f.request(
        "GET",
        "/api/example/me/pat",
        &Value::Null,
        Some(token.expose()),
        None,
        None,
    )
    .await?
    .assert_status(StatusCode::UNAUTHORIZED)?;
    rbac_contract(f, user_id).await?;
    refresh_and_reset_contract(&flows, credentials.refresh_token().expose(), user_id).await
}

async fn rbac_contract(f: &Fixture, user_id: i64) -> Result<()> {
    let policy = f.app.app()?.state().resolve::<Policy>().await?;
    let role = bounded(policy.ensure_role("sqlite-editor")).await?;
    ensure(role == bounded(policy.ensure_role("sqlite-editor")).await?)?;
    bounded(policy.assign_role(user_id, role)).await?;
    bounded(policy.grant_permission(role, "articles.read")).await?;
    let db = f.app.app()?.state().resolve::<openrust::Db>().await?;
    let links = db.quoted_table_name("role_permissions")?;
    ensure(f.scalar(format!("SELECT count(*) FROM {links}")).await? == 1)?;
    bounded(policy.revoke_permission(role, "articles.read")).await?;
    ensure(f.scalar(format!("SELECT count(*) FROM {links}")).await? == 0)?;
    bounded(policy.revoke_role(user_id, role)).await?;
    Ok(())
}

async fn refresh_and_reset_contract(flows: &AuthFlows, refresh: &str, user_id: i64) -> Result<()> {
    let refresh = refresh.to_owned();
    let rotated = bounded(flows.refresh(&refresh, "sqlite-refresh")).await?;
    ensure(
        bounded(flows.refresh(&refresh, "sqlite-refresh-replay"))
            .await
            .is_err_and(|error| matches!(error, Error::Unauthorized)),
    )?;
    ensure(
        bounded(flows.refresh(rotated.refresh_token().expose(), "sqlite-refresh-family"))
            .await
            .is_err_and(|error| matches!(error, Error::Unauthorized)),
    )?;
    let before_reset = bounded(flows.login(EMAIL, PASSWORD, "sqlite-reset-before")).await?;
    let code = bounded(flows.request_password_reset(EMAIL, "sqlite-reset-request"))
        .await?
        .ok_or_else(mismatch)?;
    ensure(
        bounded(flows.reset_password(EMAIL, code.code(), RESET_PASSWORD, "sqlite-reset-confirm"))
            .await?,
    )?;
    ensure(
        bounded(flows.login(EMAIL, PASSWORD, "sqlite-old-password"))
            .await
            .is_err_and(|error| matches!(error, Error::Unauthorized)),
    )?;
    ensure(
        bounded(flows.login(EMAIL, RESET_PASSWORD, "sqlite-new-password"))
            .await?
            .user_id()
            == user_id,
    )?;
    ensure(
        bounded(flows.refresh(
            before_reset.refresh_token().expose(),
            "sqlite-reset-revoked",
        ))
        .await
        .is_err_and(|error| matches!(error, Error::Unauthorized)),
    )?;
    Ok(())
}

pub(super) async fn root_file_local_auth_contract() -> Result<()> {
    let f = Fixture::create(false).await?;
    let outcome: Result<()> = async {
        let user = f.provision().await?;
        native_session_contract(&f)
            .await
            .inspect_err(|_| eprintln!("sqlite_native_contract phase=session"))?;
        bearer_pat_rbac_and_refresh(&f, user)
            .await
            .inspect_err(|_| eprintln!("sqlite_native_contract phase=bearer-rbac-refresh"))
    }
    .await;
    let cleanup = f.close().await;
    outcome?;
    cleanup
}

#[tokio::test]
async fn root_named_shared_memory_auth_uses_same_catalog_and_public_flows() -> Result<()> {
    let f = Fixture::create(true).await?;
    let outcome: Result<()> = async {
        let user = f.provision().await?;
        let tokens = f.app.app()?.state().resolve::<JwtTokens>().await?;
        let token = tokens.issue_access_token(user)?;
        f.request(
            "GET",
            "/api/example/me",
            &Value::Null,
            Some(&token),
            None,
            None,
        )
        .await?
        .assert_status(StatusCode::OK)?;
        Ok(())
    }
    .await;
    let cleanup = f.close().await;
    outcome?;
    cleanup
}

#[cfg(feature = "auth-mfa")]
fn authenticator_code(uri: &str) -> Result<String> {
    use hmac::{Hmac, Mac, digest::KeyInit};
    let uri = url::Url::parse(uri).map_err(Error::internal)?;
    let encoded = uri
        .query_pairs()
        .find_map(|(key, value)| (key == "secret").then(|| value.into_owned()))
        .ok_or_else(mismatch)?;
    let (mut bits, mut buffer, mut secret) = (0_u8, 0_u32, Vec::new());
    for byte in encoded.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'2'..=b'7' => byte - b'2' + 26,
            _ => return Err(mismatch()),
        };
        buffer = ((buffer << 5) | u32::from(value)) & 0xffff;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            secret.push(u8::try_from((buffer >> bits) & 0xff).map_err(Error::internal)?);
        }
    }
    let step = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(Error::internal)?
        .as_secs()
        / 30;
    let mut mac =
        <Hmac<sha1::Sha1> as KeyInit>::new_from_slice(&secret).map_err(Error::internal)?;
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[19] & 0x0f);
    let value = (u32::from(digest[offset] & 0x7f) << 24)
        | (u32::from(digest[offset + 1]) << 16)
        | (u32::from(digest[offset + 2]) << 8)
        | u32::from(digest[offset + 3]);
    Ok(format!("{:06}", value % 1_000_000))
}

#[cfg(feature = "auth-mfa")]
async fn challenge(f: &Fixture) -> Result<String> {
    let response = f
        .request(
            "POST",
            &format!("{ACCOUNT}/login"),
            &credentials(PASSWORD),
            None,
            None,
            None,
        )
        .await?;
    response
        .assert_status(StatusCode::OK)?
        .assert_header("cache-control", "no-store")?;
    let data = response_data(&response)?;
    ensure(data["status"] == "mfa_required")?;
    string(&data, "challenge")
}

#[cfg(feature = "auth-mfa")]
async fn confirm_and_rollback_audit(f: &Fixture, user_id: i64, token: &str) -> Result<Vec<String>> {
    let response = f
        .request(
            "POST",
            &format!("{ACCOUNT}/mfa/enroll"),
            &credentials(PASSWORD),
            Some(token),
            None,
            None,
        )
        .await?;
    response
        .assert_status(StatusCode::OK)?
        .assert_header("cache-control", "no-store")?;
    let uri = string(&response_data(&response)?, "provisioning_uri")?;
    let db = f.app.app()?.state().resolve::<openrust::Db>().await?;
    let audit = db.quoted_table_name("audit_records")?;
    let factors = db.quoted_table_name("auth_mfa_factors")?;
    let pending = db.quoted_table_name("auth_mfa_enrollments")?;
    let recovery = db.quoted_table_name("auth_mfa_recovery_codes")?;
    let connection = f.database.connection()?;
    // 让真实事务内审计失败；factor/recovery/enrollment变更必须整体回滚，不能静默放行。
    connection.execute_unprepared(&format!("CREATE TRIGGER openrust_mfa_audit_failure BEFORE INSERT ON {audit} WHEN NEW.action='auth.mfa.enabled' BEGIN SELECT RAISE(ABORT,'controlled audit failure'); END")).await.map_err(Error::internal)?;
    let code = authenticator_code(&uri)?;
    f.request(
        "POST",
        &format!("{ACCOUNT}/mfa/confirm"),
        &json!({"email":EMAIL,"password":PASSWORD,"code":code}),
        Some(token),
        None,
        None,
    )
    .await?
    .assert_status(StatusCode::INTERNAL_SERVER_ERROR)?;
    ensure(
        f.scalar(format!(
            "SELECT count(*) FROM {factors} WHERE user_id={user_id}"
        ))
        .await?
            == 0,
    )?;
    ensure(
        f.scalar(format!(
            "SELECT count(*) FROM {pending} WHERE user_id={user_id}"
        ))
        .await?
            == 1,
    )?;
    ensure(
        f.scalar(format!(
            "SELECT count(*) FROM {recovery} WHERE user_id={user_id}"
        ))
        .await?
            == 0,
    )?;
    connection
        .execute_unprepared("DROP TRIGGER openrust_mfa_audit_failure")
        .await
        .map_err(Error::internal)?;
    let code = authenticator_code(&uri)?;
    let response = f
        .request(
            "POST",
            &format!("{ACCOUNT}/mfa/confirm"),
            &json!({"email":EMAIL,"password":PASSWORD,"code":code}),
            Some(token),
            None,
            None,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    ensure(
        f.scalar(format!(
            "SELECT count(*) FROM {factors} WHERE user_id={user_id}"
        ))
        .await?
            == 1,
    )?;
    ensure(
        f.scalar(format!(
            "SELECT count(*) FROM {pending} WHERE user_id={user_id}"
        ))
        .await?
            == 0,
    )?;
    ensure(
        f.scalar(format!(
            "SELECT count(*) FROM {audit} WHERE action='auth.mfa.enabled'"
        ))
        .await?
            == 1,
    )?;
    response_data(&response)?["recovery_codes"]
        .as_array()
        .ok_or_else(mismatch)?
        .iter()
        .map(|value| value.as_str().map(str::to_owned).ok_or_else(mismatch))
        .collect()
}

#[cfg(feature = "auth-mfa")]
async fn mfa_contract(f: &Fixture) -> Result<()> {
    let user = f.provision().await?;
    let tokens = f.app.app()?.state().resolve::<JwtTokens>().await?;
    let token = tokens.issue_access_token(user)?;
    let codes = confirm_and_rollback_audit(f, user, &token).await?;
    ensure(codes.len() == 10)?;
    let first = codes.first().ok_or_else(mismatch)?;
    let challenge_a = challenge(f).await?;
    let challenge_b = challenge(f).await?;
    // 两个不同challenge并发竞争同一recovery，只能一个成功；同一Root池/真实事务与审计。
    let a = json!({"challenge":challenge_a,"code":first});
    let b = json!({"challenge":challenge_b,"code":first});
    let endpoint = format!("{ACCOUNT}/login/mfa");
    let (left, right) = tokio::join!(
        f.request("POST", &endpoint, &a, None, None, None),
        f.request("POST", &endpoint, &b, None, None, None)
    );
    let left = left?;
    let right = right?;
    ensure(
        (left.status() == StatusCode::OK && right.status() == StatusCode::UNAUTHORIZED)
            || (right.status() == StatusCode::OK && left.status() == StatusCode::UNAUTHORIZED),
    )?;
    let (winner, consumed_input) = if left.status() == StatusCode::OK {
        (&left, &a)
    } else {
        (&right, &b)
    };
    let access = string(&response_data(winner)?, "access_token")?;
    f.request(
        "GET",
        "/api/example/me",
        &Value::Null,
        Some(&access),
        None,
        None,
    )
    .await?
    .assert_status(StatusCode::OK)?;
    f.request("POST", &endpoint, consumed_input, None, None, None)
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    let db = f.app.app()?.state().resolve::<openrust::Db>().await?;
    let recovery = db.quoted_table_name("auth_mfa_recovery_codes")?;
    let audit = db.quoted_table_name("audit_records")?;
    ensure(
        f.scalar(format!(
            "SELECT count(*) FROM {recovery} WHERE user_id={user}"
        ))
        .await?
            == 9,
    )?;
    ensure(
        f.scalar(format!(
            "SELECT count(*) FROM {audit} WHERE action='auth.mfa.login'"
        ))
        .await?
            == 1,
    )?;
    #[cfg(feature = "auth-oidc")]
    external_association_rejected(f, user, &codes).await?;

    Ok(())
}

#[cfg(all(feature = "auth-mfa", feature = "auth-oidc"))]
async fn external_association_rejected(f: &Fixture, user: i64, codes: &[String]) -> Result<()> {
    use sha2::{Digest, Sha256};
    let db = f.app.app()?.state().resolve::<openrust::Db>().await?;
    let recovery = db.quoted_table_name("auth_mfa_recovery_codes")?;
    let audit = db.quoted_table_name("audit_records")?;
    let external_challenge = challenge(f).await?;
    let hash = format!("{:x}", Sha256::digest(external_challenge.as_bytes()));
    let external = db.quoted_table_name("auth_external_sessions")?;
    let now = chrono::Utc::now();
    let expires = (now + chrono::Duration::minutes(2))
        .format("%Y-%m-%d %H:%M:%S%.6f")
        .to_string();
    db.connection()?.execute_raw(openrust::database::sea_orm::Statement::from_sql_and_values(openrust::database::sea_orm::DbBackend::Sqlite,
            format!("INSERT INTO {external}(id_hash,user_id,provider,issuer,subject_hash,challenge_hash,expires_at,created_at) VALUES(?,?,?,?,?,?,?,?)"),
            [hash.clone().into(),user.into(),"disabled-sqlite-oidc".into(),"https://idp.example.test".into(),"f".repeat(64).into(),hash.into(),expires.into(),now.format("%Y-%m-%d %H:%M:%S%.6f").to_string().into()]))
            .await.map_err(Error::internal)?;
    let flows = f.app.app()?.state().resolve::<AuthFlows>().await?;
    let result = bounded(flows.complete_mfa_login(
        &external_challenge,
        codes.get(1).ok_or_else(mismatch)?,
        "127.0.0.1",
    ))
    .await;
    let error = result.err().ok_or_else(mismatch)?;
    ensure(
        error.code() == "unsupported_oidc_database"
            && error.status() == StatusCode::INTERNAL_SERVER_ERROR,
    )?;
    ensure(
        f.scalar(format!(
            "SELECT count(*) FROM {recovery} WHERE user_id={user}"
        ))
        .await?
            == 9,
    )?;
    ensure(
        f.scalar(format!(
            "SELECT count(*) FROM {audit} WHERE action='auth.mfa.login'"
        ))
        .await?
            == 1,
    )?;
    Ok(())
}

#[cfg(feature = "auth-mfa")]
#[tokio::test]
async fn sqlite_root_file_mfa_real_totp_recovery_once_and_atomic_audit() -> Result<()> {
    let f = Fixture::create(false).await?;
    let outcome = mfa_contract(&f).await;
    let cleanup = f.close().await;
    outcome?;
    cleanup
}
