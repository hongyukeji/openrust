//! 根标准Provider、唯一模块真实HTTP与公开AuthFlows的SQLite本地认证；不声称完整R7/OIDC。

use std::{future::Future, time::Duration};

#[cfg(feature = "auth-fortify")]
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
        Self::create_with_overlay(memory, json!({})).await
    }
    async fn create_with_overlay(memory: bool, overlay: Value) -> Result<Self> {
        let database = if memory {
            TestDatabase::sqlite_memory::<super::EmptyMigrator>().await?
        } else {
            TestDatabase::sqlite_file::<super::EmptyMigrator>().await?
        };
        let source = super::sqlite_default_host_source(&database).merge(
            figment::providers::Serialized::defaults(json!({
                // 精确 testing 引导按Laravel跳过CSRF；本契约显式启用真实Web防护。
                "app":{"env":"test","url":"http://localhost"},
                "session":{"driver":"database","cookie":COOKIE,"secure":false,"lottery":[0,100]}
            })),
        );
        let source = source.merge(figment::providers::Serialized::defaults(overlay));
        let sources = match super::sqlite_test_sources(&source) {
            Ok(sources) => sources,
            Err(error) => {
                eprintln!("SQLite 根宿主配置/启动诊断：{error}");
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
            eprintln!("SQLite 根宿主迁移命令诊断：{error}");
            let _ = database.close().await;
            return Err(error);
        }
        match bounded(TestApp::<crate::bootstrap::Server>::builder(sources).build()).await {
            Ok(app) => Ok(Self { app, database }),
            Err(error) => {
                eprintln!("SQLite 根宿主配置/启动诊断：{error}");
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
        let peer = if path.ends_with("/session/login") {
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
        let response = self
            .app
            .request_with_limits(request, 128 * 1024, BUDGET)
            .await?;
        if response.status() != StatusCode::OK {
            let code = response
                .json::<Value>()
                .ok()
                .and_then(|body| body.get("code").cloned());
            eprintln!(
                "sqlite_contract response method={method} path={path} status={} code={code:?}",
                response.status()
            );
        }
        Ok(response)
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
    .assert_status(StatusCode::from_u16(419).map_err(Error::internal)?)?;
    f.request(
        "POST",
        &format!("{ACCOUNT}/session/preference"),
        &json!({"density":"compact"}),
        None,
        Some(&session),
        Some("wrong-csrf"),
    )
    .await?
    .assert_status(StatusCode::from_u16(419).map_err(Error::internal)?)?;
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
// 所选authority保留纯密码学裸签的负例；正式入口必须产生可重核的凭据家族。
async fn current_password_access(f: &Fixture, user_id: i64, source: &str) -> Result<String> {
    let tokens = f.app.app()?.state().resolve::<JwtTokens>().await?;
    let raw = tokens.issue_access_token(user_id)?;
    #[cfg(feature = "auth-fortify")]
    {
        f.request(
            "GET",
            "/api/example/me",
            &Value::Null,
            Some(&raw),
            None,
            None,
        )
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
        let authenticated = bounded(f.flows().await?.login(EMAIL, PASSWORD, source)).await?;
        ensure(authenticated.user_id() == user_id)?;
        Ok(authenticated.access_token().to_owned())
    }
    #[cfg(not(feature = "auth-fortify"))]
    {
        let _source = source;
        Ok(raw)
    }
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
    #[cfg(feature = "auth-fortify")]
    let token = {
        let refused = bounded(tokens.issue(user_id, "unverified", &["articles.read".into()], None))
            .await
            .err()
            .ok_or_else(mismatch)?;
        ensure(refused.code() == "auth_credential_proof_required")?;
        let authority = f
            .app
            .app()?
            .state()
            .resolve::<openrust::auth::fortify::CredentialAuthority>()
            .await?;
        let jwt = f.app.app()?.state().resolve::<JwtTokens>().await?;
        let parent =
            bounded(authority.authenticate_access_token(jwt.as_ref(), credentials.access_token()))
                .await?;
        ensure(parent.user_id() == user_id)?;
        let expires_at = parent.expires_at().ok_or_else(mismatch)?;
        bounded(tokens.issue_from_verified(
            parent,
            "SQLite module",
            &["articles.read".into()],
            Some(expires_at),
        ))
        .await?
    };
    #[cfg(not(feature = "auth-fortify"))]
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
        let token = current_password_access(&f, user, "sqlite-memory-positive").await?;
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

#[cfg(feature = "auth-fortify")]
fn authenticator_code(encoded: &str, offset: u64) -> Result<String> {
    use hmac::{Hmac, Mac, digest::KeyInit};
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
    mac.update(&(step + offset).to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[19] & 0x0f);
    let value = (u32::from(digest[offset] & 0x7f) << 24)
        | (u32::from(digest[offset + 1]) << 16)
        | (u32::from(digest[offset + 2]) << 8)
        | u32::from(digest[offset + 3]);
    Ok(format!("{:06}", value % 1_000_000))
}

#[cfg(feature = "auth-fortify")]
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
    ensure(data["status"] == "two_factor_required" && data.get("credentials").is_none())?;
    string(&data, "challenge")
}

#[cfg(feature = "auth-fortify")]
async fn confirm_with_storage_rollback(
    f: &Fixture,
    actions: &openrust::auth::fortify::TwoFactorActions,
    user: &openrust::auth::AuthUser,
) -> Result<()> {
    actions.enable(user, false).await?;
    let secret = actions.secret_key(user).await?;
    let codes = actions.recovery_codes(user).await?;
    ensure(codes.len() == 8)?;
    let db = f.app.app()?.state().resolve::<openrust::Db>().await?;
    let users = db.quoted_table_name("users")?;
    let account = user
        .as_two_factor()
        .ok_or_else(mismatch)?
        .two_factor_authority()
        .ok_or_else(mismatch)?;
    let pending = account.load().await?;
    let id = user.auth_identifier()?.as_i64().ok_or_else(mismatch)?;
    let connection = f.database.connection()?;
    connection.execute_unprepared(&format!("CREATE TRIGGER reject_confirmation BEFORE UPDATE OF two_factor_confirmed_at ON {users} WHEN NEW.two_factor_confirmed_at IS NOT NULL BEGIN SELECT RAISE(ABORT,'controlled confirmation write failure'); END")).await.map_err(Error::internal)?;
    let outcome: Result<()> = async {
        let failed = actions.confirm(user, &authenticator_code(&secret, 0)?)
            .await.err().ok_or_else(mismatch)?;
        ensure(failed.status() == StatusCode::INTERNAL_SERVER_ERROR)?;
        ensure(account.load().await? == pending)?;
        ensure(f.scalar(format!("SELECT count(*) FROM {users} WHERE id={id} AND two_factor_confirmed_at IS NULL AND two_factor_consumed_codes IS NULL")).await? == 1)?;
        ensure(*actions.recovery_codes(user).await? == *codes)?;
        Ok(())
    }.await;
    let cleanup = connection
        .execute_unprepared("DROP TRIGGER reject_confirmation")
        .await
        .map_err(Error::internal);
    outcome?;
    cleanup?;
    // 独立时间步产生另一真实验证码，缓存中的前次校验不假称可随数据库回滚。
    actions
        .confirm(user, &authenticator_code(&secret, 1)?)
        .await?;
    ensure(account.load().await?.state.confirmed_at.is_some())?;
    Ok(())
}

#[cfg(feature = "auth-fortify")]
async fn selected_login_audit_rollback(
    f: &Fixture,
    user_id: i64,
    user: &openrust::auth::AuthUser,
    challenge: &str,
    recovery: &str,
) -> Result<()> {
    use sha2::{Digest as _, Sha256};
    let db = f.app.app()?.state().resolve::<openrust::Db>().await?;
    let audit = db.quoted_table_name("audit_records")?;
    let challenges = db.quoted_table_name("auth_factor_challenges")?;
    let refresh = db.quoted_table_name("auth_refresh_tokens")?;
    let account = user
        .as_two_factor()
        .ok_or_else(mismatch)?
        .two_factor_authority()
        .ok_or_else(mismatch)?;
    let before = account.load().await?;
    let issued_before = f
        .scalar(format!(
            "SELECT count(*) FROM {refresh} WHERE user_id={user_id}"
        ))
        .await?;
    let hash = format!("{:x}", Sha256::digest(challenge.as_bytes()));
    let connection = f.database.connection()?;
    connection.execute_unprepared(&format!("CREATE TRIGGER reject_factor_audit BEFORE INSERT ON {audit} WHEN NEW.action='auth.two_factor.login' BEGIN SELECT RAISE(ABORT,'controlled login audit failure'); END")).await.map_err(Error::internal)?;
    let outcome: Result<()> = async {
        f.request("POST", &format!("{ACCOUNT}/login/two-factor"), &json!({"challenge":challenge,"recovery_code":recovery}), None,None,None)
            .await?.assert_status(StatusCode::INTERNAL_SERVER_ERROR)?;
        ensure(f.scalar(format!("SELECT count(*) FROM {challenges} WHERE token_hash='{hash}' AND consumed_at IS NULL")).await? == 1)?;
        ensure(f.scalar(format!("SELECT count(*) FROM {refresh} WHERE user_id={user_id}")).await? == issued_before)?;
        ensure(account.load().await? == before)?;
        ensure(f.scalar(format!("SELECT count(*) FROM {audit} WHERE action='auth.two_factor.login'")).await? == 0)?;
        Ok(())
    }.await;
    let cleanup = connection
        .execute_unprepared("DROP TRIGGER reject_factor_audit")
        .await
        .map_err(Error::internal);
    outcome?;
    cleanup?;
    Ok(())
}

#[cfg(feature = "auth-fortify")]
async fn two_factor_contract(f: &Fixture) -> Result<()> {
    let user_id = f.provision().await?;
    let scope = f.app.app()?.state().container().scope()?;
    let actions = scope
        .resolve::<openrust::auth::fortify::TwoFactorActions>()
        .await?;
    let provider = scope
        .resolve::<openrust::auth::AuthManager>()
        .await?
        .create_user_provider(Some("users"))
        .await?
        .ok_or_else(mismatch)?;
    let user = provider
        .retrieve_by_id(&json!(user_id))
        .await?
        .ok_or_else(mismatch)?;
    confirm_with_storage_rollback(f, &actions, &user).await?;
    let codes = actions.recovery_codes(&user).await?;
    let first = codes.first().ok_or_else(mismatch)?;
    let challenge_a = challenge(f).await?;
    selected_login_audit_rollback(f, user_id, &user, &challenge_a, first).await?;
    let challenge_b = challenge(f).await?;
    let a = json!({"challenge":challenge_a,"recovery_code":first});
    let b = json!({"challenge":challenge_b,"recovery_code":first});
    let endpoint = format!("{ACCOUNT}/login/two-factor");
    let db = f.app.app()?.state().resolve::<openrust::Db>().await?;
    let refresh = db.quoted_table_name("auth_refresh_tokens")?;
    let issued_before = f
        .scalar(format!(
            "SELECT count(*) FROM {refresh} WHERE user_id={user_id}"
        ))
        .await?;
    let (left, right) = tokio::join!(
        f.request("POST", &endpoint, &a, None, None, None),
        f.request("POST", &endpoint, &b, None, None, None)
    );
    let left = left?;
    let right = right?;
    let (winner, replay, loser) = if left.status() == StatusCode::OK {
        (&left, &a, &right)
    } else {
        (&right, &b, &left)
    };
    winner.assert_status(StatusCode::OK)?;
    loser.assert_status(StatusCode::UNAUTHORIZED)?;
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
    f.request("POST", &endpoint, replay, None, None, None)
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    let remaining = actions.recovery_codes(&user).await?;
    ensure(
        remaining.len() == 8
            && !remaining.contains(first)
            && codes[1..].iter().all(|code| remaining.contains(code)),
    )?;
    ensure(
        f.scalar(format!(
            "SELECT count(*) FROM {refresh} WHERE user_id={user_id}"
        ))
        .await?
            == issued_before + 1,
    )?;
    let challenges = db.quoted_table_name("auth_factor_challenges")?;
    ensure(
        f.scalar(format!(
            "SELECT count(*) FROM {challenges} WHERE user_id={user_id} AND consumed_at IS NOT NULL"
        ))
        .await?
            == 1,
    )?;
    let audit = db.quoted_table_name("audit_records")?;
    ensure(
        f.scalar(format!(
            "SELECT count(*) FROM {audit} WHERE action='auth.two_factor.login'"
        ))
        .await?
            == 1,
    )?;
    #[cfg(feature = "auth-oidc")]
    external_association_rejected(f, user_id, &codes).await?;
    Ok(())
}

#[cfg(all(feature = "auth-fortify", feature = "auth-oidc"))]
async fn external_association_rejected(f: &Fixture, user: i64, codes: &[String]) -> Result<()> {
    use sha2::{Digest, Sha256};
    let db = f.app.app()?.state().resolve::<openrust::Db>().await?;
    let before: String = db
        .table("users")?
        .where_eq("id", user)?
        .first()
        .await?
        .ok_or_else(mismatch)?
        .try_get("", "two_factor_recovery_codes")
        .map_err(Error::internal)?;
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
    let input =
        json!({"challenge":external_challenge,"recovery_code":codes.get(1).ok_or_else(mismatch)?});
    let result = f
        .request(
            "POST",
            &format!("{ACCOUNT}/login/two-factor"),
            &input,
            None,
            None,
            None,
        )
        .await?;
    result.assert_status(StatusCode::INTERNAL_SERVER_ERROR)?;
    ensure(result.json::<Value>()?["code"] == "unsupported_oidc_database")?;
    let after: String = db
        .table("users")?
        .where_eq("id", user)?
        .first()
        .await?
        .ok_or_else(mismatch)?
        .try_get("", "two_factor_recovery_codes")
        .map_err(Error::internal)?;
    ensure(after == before)?;
    ensure(
        f.scalar(format!(
            "SELECT count(*) FROM {audit} WHERE action='auth.two_factor.login'"
        ))
        .await?
            == 1,
    )?;
    Ok(())
}

#[cfg(feature = "auth-fortify")]
#[tokio::test]
async fn sqlite_root_file_two_factor_recovery_once_and_atomic_audit() -> Result<()> {
    let f = Fixture::create(false).await?;
    let outcome = two_factor_contract(&f).await;
    let cleanup = f.close().await;
    outcome?;
    cleanup
}

// 此模块已由父模块限定auth+db-sqlite；明确启用encryption以运行真实Cookie/CSRF链。
#[cfg(all(feature = "realtime", feature = "encryption"))]
struct ProviderScopeServer(tokio::task::JoinHandle<std::io::Result<()>>);
#[cfg(all(feature = "realtime", feature = "encryption"))]
impl Drop for ProviderScopeServer {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(all(feature = "realtime", feature = "encryption"))]
async fn provider_scope_socket_status(
    address: std::net::SocketAddr,
    session: &str,
) -> Result<StatusCode> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    bounded(async {
        let mut stream = tokio::net::TcpStream::connect(address).await.map_err(Error::internal)?;
        // 真实Hyper请求产生OnUpgrade；不以oneshot的缺失upgrade错误冒充所有者查询未发生。
        let request = format!("GET /api/example/articles/991/browser-socket HTTP/1.1\r\nHost: localhost\r\nOrigin: http://localhost\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nCookie: {session}\r\n\r\n");
        stream.write_all(request.as_bytes()).await.map_err(Error::internal)?;
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            ensure(headers.len() < 32 * 1024)?;
            let byte = stream.read_u8().await.map_err(Error::internal)?;
            headers.push(byte);
        }
        let headers = std::str::from_utf8(&headers).map_err(Error::internal)?;
        let status = headers.lines().next().and_then(|line| line.split_ascii_whitespace().nth(1))
            .ok_or_else(mismatch)?.parse::<u16>().map_err(Error::internal)?;
        // 释放TCP连接，服务器端WebSocket任务随EOF回收；不保留额外认证连接。
        drop(stream);
        eprintln!("provider_scope_socket_status={status}");
        StatusCode::from_u16(status).map_err(Error::internal)
    }).await
}

#[cfg(all(feature = "realtime", feature = "encryption"))]
async fn provider_scope_wrong_sources(
    fixture: &Fixture,
    address: std::net::SocketAddr,
    session: &str,
    identifier: i64,
    other: &openrust::Db,
) -> Result<()> {
    use openrust::auth::{AuthDrivers, DatabaseUserProvider, UserProvider};
    let state = fixture.app.app()?.state();
    let db = state.resolve::<openrust::Db>().await?;
    let drivers = state.resolve::<AuthDrivers>().await?;
    let passwords = state.resolve::<openrust::hashing::HashManager>().await?;
    for (source, table) in [
        (db.clone(), "provider_scope_users"),
        (std::sync::Arc::new(other.clone()), "users"),
    ] {
        let provider = std::sync::Arc::new(DatabaseUserProvider::new(
            (*source).clone(),
            passwords.clone(),
            table,
        )?);
        let account = provider
            .retrieve_by_id(&json!(identifier))
            .await?
            .ok_or_else(mismatch)?;
        ensure(account.auth_identifier()? == json!(identifier))?;
        ensure(!provider.uses_database_accounts(&db, "users")?)?;
        drivers.provider("app-users", move |_, _| {
            let provider = provider.clone();
            Box::pin(async move { Ok(provider as std::sync::Arc<dyn UserProvider>) })
        })?;
        fixture
            .request(
                "GET",
                &format!("{ACCOUNT}/session/preference"),
                &Value::Null,
                None,
                Some(session),
                None,
            )
            .await?
            .assert_status(StatusCode::UNAUTHORIZED)?;
        // 根文章表已改名；若越过门禁进入require_owned，真实SQL必定500，不能恰好返回404掩盖查询。
        ensure(provider_scope_socket_status(address, session).await? == StatusCode::UNAUTHORIZED)?;
    }
    Ok(())
}

#[cfg(all(feature = "realtime", feature = "encryption"))]
async fn provider_scope_prepare(
    fixture: &Fixture,
    other: &openrust::Db,
) -> Result<(
    i64,
    String,
    std::sync::Arc<dyn openrust::auth::UserProvider>,
)> {
    use openrust::auth::AuthManager;
    use openrust::database::sea_orm::ConnectionTrait;
    let identifier = fixture.provision().await?;
    let login = fixture
        .request(
            "POST",
            &format!("{ACCOUNT}/session/login"),
            &credentials(PASSWORD),
            None,
            None,
            None,
        )
        .await?;
    login.assert_status(StatusCode::OK)?;
    let session = cookie(&login)?;
    let state = fixture.app.app()?.state();
    let db = state.resolve::<openrust::Db>().await?;
    let scope = state.container().scope()?;
    let manager = scope.resolve::<AuthManager>().await?;
    let original = manager
        .create_user_provider(Some("users"))
        .await?
        .ok_or_else(mismatch)?;
    ensure(original.uses_database_accounts(&db, "users")?)?;
    db.table("example_articles")?
        .insert(vec![
            ("id", 991_i64.into()),
            ("owner_id", identifier.into()),
            ("title", "provider source probe".into()),
            ("body", "owned data".into()),
        ])
        .await?;
    for (source, table) in [
        ((*db).clone(), "provider_scope_users"),
        (other.clone(), "users"),
    ] {
        let quoted = source.quoted_table_name(table)?;
        source
            .connection()?
            .execute_unprepared(&format!(
                "CREATE TABLE {quoted} (id INTEGER PRIMARY KEY,password TEXT,remember_token TEXT)"
            ))
            .await
            .map_err(Error::internal)?;
        source
            .table(table)?
            .insert(vec![
                ("id", identifier.into()),
                ("password", "not-used-for-session-restore".into()),
            ])
            .await?;
    }
    Ok((identifier, session, original))
}

#[cfg(all(feature = "realtime", feature = "encryption"))]
async fn provider_scope_business(fixture: &Fixture, other: &openrust::Db) -> Result<()> {
    use openrust::auth::AuthDrivers;
    use openrust::database::sea_orm::ConnectionTrait;
    let (identifier, session, original) = provider_scope_prepare(fixture, other).await?;
    let state = fixture.app.app()?.state();
    let db = state.resolve::<openrust::Db>().await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(Error::internal)?;
    let address = listener.local_addr().map_err(Error::internal)?;
    let router = openrust::http::build_router(fixture.app.app()?)?;
    let mut server = ProviderScopeServer(tokio::spawn(async move {
        openrust::http::axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
    }));
    let outcome: Result<()> = async {
        fixture
            .request(
                "GET",
                &format!("{ACCOUNT}/session/preference"),
                &Value::Null,
                None,
                Some(&session),
                None,
            )
            .await?
            .assert_status(StatusCode::OK)?;
        ensure(
            provider_scope_socket_status(address, &session).await?
                == StatusCode::SWITCHING_PROTOCOLS,
        )?;
        db.connection()?
            .execute_unprepared(
                "ALTER TABLE example_articles RENAME TO provider_scope_articles_hidden",
            )
            .await
            .map_err(Error::internal)?;
        // 正来源校准哨兵：相同真实握手确实进入所有者SQL，而不是在extractor或Origin前被拒。
        ensure(
            provider_scope_socket_status(address, &session).await?
                == StatusCode::INTERNAL_SERVER_ERROR,
        )?;
        provider_scope_wrong_sources(fixture, address, &session, identifier, other).await?;
        let drivers = state.resolve::<AuthDrivers>().await?;
        drivers.provider("app-users", move |_, _| {
            let original = original.clone();
            Box::pin(async move { Ok(original) })
        })?;
        db.connection()?
            .execute_unprepared(
                "ALTER TABLE provider_scope_articles_hidden RENAME TO example_articles",
            )
            .await
            .map_err(Error::internal)?;
        ensure(
            provider_scope_socket_status(address, &session).await?
                == StatusCode::SWITCHING_PROTOCOLS,
        )?;
        Ok(())
    }
    .await;
    server.0.abort();
    let stopped = tokio::time::timeout(BUDGET, &mut server.0)
        .await
        .map_err(Error::internal)?;
    match stopped {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return Err(Error::internal(error)),
        Err(error) => ensure(error.is_cancelled())?,
    }
    outcome
}

/// 必须另跑不启用auth-fortify的组合，防止条件双因素门禁恰好掩盖账户域缺口。
#[cfg(all(feature = "realtime", feature = "encryption"))]
#[tokio::test]
async fn sqlite_module_http_rejects_same_numeric_user_from_other_table_and_database_before_ownership_query()
-> Result<()> {
    let fixture = Fixture::create_with_overlay(
        false,
        json!({
            "api":{"http_status":"semantic"},
            "session":{"driver":"array"},
            "realtime":{"allowed_origins":["http://localhost"],"max_connections":16}
        }),
    )
    .await?;
    let secondary = match TestDatabase::sqlite_file::<super::EmptyMigrator>().await {
        Ok(database) => database,
        Err(error) => {
            let closed = fixture.close().await;
            closed?;
            return Err(error);
        }
    };
    let other = openrust::Db::default();
    let outcome: Result<()> = async {
        other.initialize(secondary.connection()?)?;
        provider_scope_business(&fixture, &other).await
    }
    .await;
    drop(other);
    let stopped = fixture.close().await;
    let closed = secondary.close().await;
    outcome?;
    stopped?;
    closed
}

type CapturedRootUsers =
    std::sync::Arc<std::sync::Mutex<Vec<std::sync::Arc<crate::app::models::user::User>>>>;

struct ModelPasswordGetter {
    encoded: String,
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
impl openrust::db::eloquent::AttributeGetter for ModelPasswordGetter {
    fn get(&self, _: Option<&Value>, _: &serde_json::Map<String, Value>) -> Result<Option<Value>> {
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(Some(json!(self.encoded)))
    }
}

// 沿实际根宿主的公开工厂扩展，只改变模型属性解释，不替换账户表或 Guard。
async fn install_root_model_factory(
    f: &Fixture,
    captured: CapturedRootUsers,
    encoded: String,
) -> Result<std::sync::Arc<std::sync::atomic::AtomicUsize>> {
    use openrust::auth::{AuthDrivers, DatabaseUserProvider, UserProvider};
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let getter_calls = calls.clone();
    let state = f.app.app()?.state();
    let db = state.resolve::<openrust::Db>().await?;
    let hashing = state.resolve::<openrust::hashing::HashManager>().await?;
    #[cfg(feature = "auth-fortify")]
    let encryption = state.resolve::<openrust::security::Encryptor>().await?;
    let mut metadata =
        openrust::db::eloquent::ModelMetadata::new("users", "app::models::user::User");
    // 本模块的因子authority要求数据库原始typed ID；成功HTTP夹具不改变整数账户域。
    metadata
        .casts
        .insert("id".into(), openrust::db::eloquent::AttributeCast::Integer);
    metadata.casts.insert(
        "email_verified_at".into(),
        openrust::db::eloquent::AttributeCast::Boolean,
    );
    let metadata = std::sync::Arc::new(metadata);
    let source = (*db).clone();
    let provider = std::sync::Arc::new(
        DatabaseUserProvider::new((*db).clone(), hashing, "users")?.with_user_factory(
            move |attributes| {
                let user = crate::app::models::user::User::new(attributes)
                    .with_model_metadata(metadata.clone());
                user.model_attributes().register_getter(
                    "password_hash",
                    std::sync::Arc::new(ModelPasswordGetter {
                        encoded: encoded.clone(),
                        calls: getter_calls.clone(),
                    }),
                )?;
                #[cfg(feature = "auth-fortify")]
                let user = {
                    use openrust::auth::Authenticatable;
                    let factor = openrust::auth::fortify::DatabaseTwoFactorState::new(
                        source.clone(),
                        "users",
                        "id",
                        user.auth_identifier()?,
                    )?;
                    user.with_two_factor_state(factor.clone())
                        .with_factor_authority(
                            openrust::auth::fortify::DatabaseFactorAuthority::new(
                                factor,
                                (*encryption).clone(),
                            ),
                        )
                };
                #[cfg(not(feature = "auth-fortify"))]
                let _ = &source;
                let user = std::sync::Arc::new(user);
                captured.lock().map_err(|_| mismatch())?.push(user.clone());
                Ok(user)
            },
        ),
    );
    state
        .resolve::<AuthDrivers>()
        .await?
        .provider("app-users", move |_, _| {
            let provider = provider.clone();
            Box::pin(async move { Ok(provider as std::sync::Arc<dyn UserProvider>) })
        })?;
    Ok(calls)
}

// 同一真实HTTP账户的raw快照仍允许独立的字符串主键模型解释；它不成为整数因子账户。
fn string_identity_model(
    user: &crate::app::models::user::User,
    identifier: i64,
) -> Result<std::sync::Arc<crate::app::models::user::User>> {
    use openrust::{auth::Authenticatable, db::eloquent::ModelIdentity};
    let mut metadata = user.model_metadata().clone();
    metadata
        .casts
        .insert("id".into(), openrust::db::eloquent::AttributeCast::String);
    let candidate = std::sync::Arc::new(
        crate::app::models::user::User::new(user.model_attributes().raw_attributes()?)
            .with_model_metadata(std::sync::Arc::new(metadata)),
    );
    ensure(candidate.auth_identifier()? == json!(identifier.to_string()))?;
    ensure(candidate.model_key()? == json!(identifier.to_string()))?;
    ensure(candidate.model_attributes().raw_attribute("id")? == Some(json!(identifier)))?;
    Ok(candidate)
}

async fn root_model_http_attributes(f: &Fixture) -> Result<()> {
    use openrust::auth::{AuthManager, Authenticatable, Credentials};
    let identifier = f.provision().await?;
    // 第一轮保留根 AppServiceProvider 的真实工厂及其默认整数主键。
    native_session_contract(f).await?;
    let state = f.app.app()?.state();
    let scope = state.container().scope()?;
    let manager = scope.resolve::<AuthManager>().await?;
    let original = manager
        .create_user_provider(Some("users"))
        .await?
        .ok_or_else(mismatch)?;
    let default = original
        .retrieve_by_id(&json!(identifier))
        .await?
        .ok_or_else(mismatch)?;
    ensure(default.as_model().is_some())?;
    ensure(default.auth_identifier()? == json!(identifier))?;
    let encoded = default.auth_password()?.ok_or_else(mismatch)?;
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let getter_calls = install_root_model_factory(f, captured.clone(), encoded.clone()).await?;
    let calls_before_login = getter_calls.load(std::sync::atomic::Ordering::Relaxed);
    // 第二轮仍经相同模块端点、Session Guard、账户来源和 Fortify 策略。
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
    login.assert_status(StatusCode::OK)?;
    // 在任何手工模型/Provider检查前，实证HTTP登录消费了所注册的密码getter。
    ensure(getter_calls.load(std::sync::atomic::Ordering::Relaxed) > calls_before_login)?;
    let session = cookie(&login)?;
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
    let users = captured.lock().map_err(|_| mismatch())?.clone();
    ensure(!users.is_empty())?;
    for user in &users {
        ensure(user.auth_identifier()? == json!(identifier))?;
        ensure(user.model_attributes().raw_attribute("id")? == Some(json!(identifier)))?;
        ensure(user.auth_attribute("email_verified_at")? == Some(json!(true)))?;
        ensure(
            user.model_attributes()
                .raw_attribute("email_verified_at")?
                .is_some_and(|value| !value.is_boolean()),
        )?;
    }
    // 使用 HTTP 实际物化的同一个 User，区分密码访问器与原始行；此项是 Provider 断言。
    let user = users.first().ok_or_else(mismatch)?;
    user.model_attributes()
        .set_raw_attribute("password_hash", json!("invalid-raw-hash"))?;
    ensure(user.auth_password()?.as_deref() == Some(encoded.as_str()))?;
    let scope = state.container().scope()?;
    let provider = scope
        .resolve::<AuthManager>()
        .await?
        .create_user_provider(Some("users"))
        .await?
        .ok_or_else(mismatch)?;
    let string_user = string_identity_model(user, identifier)?;
    #[cfg(feature = "auth-fortify")]
    {
        let authority = user
            .as_two_factor()
            .and_then(openrust::auth::fortify::TwoFactorAuthenticatable::two_factor_authority)
            .ok_or_else(mismatch)?;
        let candidate: openrust::auth::AuthUser = string_user;
        ensure(matches!(
            authority
                .validate_account(&candidate, provider.as_ref())
                .await,
            Err(Error::Unauthorized)
        ))?;
    }
    #[cfg(not(feature = "auth-fortify"))]
    let _ = string_user;
    let input = Credentials::new(serde_json::Map::from_iter([(
        "password".into(),
        json!(PASSWORD),
    )]));
    ensure(provider.validate_credentials(user.as_ref(), &input).await?)?;
    ensure(
        user.model_attributes().raw_attribute("password_hash")? == Some(json!("invalid-raw-hash")),
    )?;
    Ok(())
}

#[tokio::test]
async fn sqlite_module_http_uses_root_user_attribute_casts_and_registered_getter() -> Result<()> {
    let f = Fixture::create(false).await?;
    let outcome = root_model_http_attributes(&f).await;
    let cleanup = f.close().await;
    outcome?;
    cleanup
}
