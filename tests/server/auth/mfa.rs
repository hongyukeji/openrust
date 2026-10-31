//! 验证 MFA 登录与因子管理的真实数据库和 HTTP 边界。
#![cfg(all(
    feature = "auth-mfa",
    any(feature = "db-postgres", feature = "db-mysql")
))]

use std::{
    collections::BTreeMap,
    error::Error as StdError,
    net::SocketAddr,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::Utc;
use figment::{Figment, providers::Format};
use hmac::{Hmac, Mac, digest::KeyInit};
use openrust::http::{HttpServiceProvider, Inject, Json as FrameworkJson, Routes, build_router};
use openrust::testing::TestDatabase;
use openrust::{
    App, Result,
    audit::DatabaseAuditMigration,
    auth::{
        AuthFlows, AuthFlowsServiceProvider, AuthLayer, AuthMfaMigration, AuthMigration, AuthMode,
        AuthRefreshTokenMigration, AuthServiceProvider, AuthVerificationCodeMigration, CurrentUser,
        JwtServiceProvider, JwtTokens, LoginAttempt, MfaLoginChallenge, MfaServiceProvider,
        PasswordConfig, Session, SessionServiceProvider, TotpEnrollment,
    },
    config::FrameworkConfig,
    database::{
        DatabaseBackend, DatabaseRateLimitMigration, DatabaseServiceProvider, DatabaseTableNames,
        Db,
        sea_orm::{ConnectionTrait, DbBackend, Statement},
        sea_orm_migration::{MigratorTrait, prelude::*},
    },
    http::axum::{
        Json, Router,
        extract::State,
        http::{StatusCode, header::CONTENT_TYPE},
        response::{IntoResponse, Response},
        routing::post,
    },
    security::EncryptionServiceProvider,
};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

#[cfg(feature = "auth-oidc")]
mod oidc_mfa {
    use std::{
        collections::HashMap,
        sync::{
            RwLock,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use openrust::{
        Error, Result,
        auth::{ExternalIdentityProvider, OidcAuthorization, VerifiedExternalIdentity},
    };

    pub struct Provider {
        index: AtomicUsize,
        identities: RwLock<HashMap<String, VerifiedExternalIdentity>>,
    }

    impl Provider {
        pub fn new() -> Self {
            Self {
                index: AtomicUsize::new(0),
                identities: RwLock::new(HashMap::new()),
            }
        }
        pub fn set_identity(
            &self,
            code: &str,
            identity: VerifiedExternalIdentity,
        ) -> std::result::Result<(), Box<dyn std::error::Error>> {
            self.identities
                .write()
                .map_err(|error| std::io::Error::other(error.to_string()))?
                .insert(code.to_owned(), identity);
            Ok(())
        }
    }

    impl ExternalIdentityProvider for Provider {
        fn name(&self) -> &'static str {
            "mfa-test-idp"
        }
        fn issuer(&self) -> &'static str {
            "https://mfa-idp.example.test"
        }
        fn authorize(&self) -> Result<OidcAuthorization> {
            let index = self.index.fetch_add(1, Ordering::SeqCst);
            let state = format!("mfa-state-{index}");
            let mut url = url::Url::parse("https://mfa-idp.example.test/authorize")
                .map_err(Error::internal)?;
            url.query_pairs_mut().append_pair("state", &state);
            OidcAuthorization::new(
                url,
                state.clone(),
                format!("nonce-{index}"),
                format!("verifier-{index}"),
            )
        }
        fn authenticate_code<'a>(
            &'a self,
            callback_state: &'a str,
            expected_state: &'a str,
            code: &'a str,
            _nonce: &'a str,
            _pkce_verifier: &'a str,
        ) -> openrust::core::BoxFuture<'a, Result<VerifiedExternalIdentity>> {
            Box::pin(async move {
                if callback_state != expected_state {
                    return Err(Error::Unauthorized);
                }
                self.identities
                    .read()
                    .map_err(|_| {
                        Error::internal(std::io::Error::other("test provider lock poisoned"))
                    })?
                    .get(code)
                    .cloned()
                    .ok_or(Error::Unauthorized)
            })
        }
    }

    pub fn identity() -> Result<VerifiedExternalIdentity> {
        VerifiedExternalIdentity::new(
            "https://mfa-idp.example.test",
            "mfa-subject",
            Some("mfa.user@example.test".to_owned()),
            true,
        )
    }
}

type HmacSha1 = Hmac<Sha1>;
type TestResult<T> = std::result::Result<T, Box<dyn StdError>>;

struct MfaMigrator;

impl MigratorTrait for MfaMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(AuthMigration),
            Box::new(AuthRefreshTokenMigration),
            Box::new(AuthVerificationCodeMigration),
            Box::new(DatabaseRateLimitMigration),
            Box::new(DatabaseAuditMigration),
            Box::new(AuthMfaMigration),
            #[cfg(feature = "auth-oidc")]
            Box::new(openrust::auth::AuthExternalIdentityMigration),
            #[cfg(feature = "auth-oidc")]
            Box::new(openrust::auth::AuthExternalSessionMigration),
        ]
    }
}

#[derive(Deserialize)]
struct LoginInput {
    email: String,
    password: String,
}

#[derive(Deserialize)]
struct CompleteInput {
    challenge: String,
    code: String,
}

#[derive(Serialize, Deserialize)]
struct HttpChallenge {
    challenge: String,
    expires_at: String,
}

#[derive(Serialize, Deserialize)]
struct HttpCredentials {
    access_token: String,
    refresh_token: String,
}

async fn legacy_login(State(flows): State<AuthFlows>, Json(input): Json<LoginInput>) -> Response {
    match flows
        .login(&input.email, &input.password, "http-login")
        .await
    {
        Ok(credentials) => Json(HttpCredentials {
            access_token: credentials.access_token().to_owned(),
            refresh_token: credentials.refresh_token().expose().to_owned(),
        })
        .into_response(),
        Err(error) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"code": error.code()})),
        )
            .into_response(),
    }
}

async fn begin_http_login(
    State(flows): State<AuthFlows>,
    Json(input): Json<LoginInput>,
) -> Response {
    match flows
        .begin_login(&input.email, &input.password, "http-begin")
        .await
    {
        Ok(LoginAttempt::MfaRequired(challenge)) => Json(HttpChallenge {
            challenge: challenge.expose().to_owned(),
            expires_at: challenge.expires_at().to_rfc3339(),
        })
        .into_response(),
        Ok(LoginAttempt::Authenticated(credentials)) => Json(HttpCredentials {
            access_token: credentials.access_token().to_owned(),
            refresh_token: credentials.refresh_token().expose().to_owned(),
        })
        .into_response(),
        Err(error) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"code": error.code()})),
        )
            .into_response(),
    }
}

async fn complete_http_login(
    State(flows): State<AuthFlows>,
    Json(input): Json<CompleteInput>,
) -> Response {
    match flows
        .complete_mfa_login(&input.challenge, &input.code, "http-begin")
        .await
    {
        Ok(credentials) => Json(HttpCredentials {
            access_token: credentials.access_token().to_owned(),
            refresh_token: credentials.refresh_token().expose().to_owned(),
        })
        .into_response(),
        Err(error) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"code": error.code()})),
        )
            .into_response(),
    }
}

async fn seed_session(
    session: Session,
    FrameworkJson(user_id): FrameworkJson<i64>,
) -> Result<FrameworkJson<&'static str>> {
    session.put("openrust.auth.user_id", user_id)?;
    session.put("openrust.auth.csrf_hash", "test-csrf")?;
    Ok(FrameworkJson("seeded"))
}

async fn protected_session(user: CurrentUser) -> Result<FrameworkJson<i64>> {
    Ok(FrameworkJson(user.id()))
}

async fn session_login(
    Inject(flows): Inject<AuthFlows>,
    session: Session,
) -> Result<FrameworkJson<&'static str>> {
    flows
        .login_session(
            "mfa-session@example.test",
            "correct horse battery staple",
            "mfa-session-login",
            &session,
        )
        .await?;
    Ok(FrameworkJson("logged in"))
}

async fn start_http(flows: AuthFlows) -> TestResult<(String, oneshot::Sender<()>, JoinHandle<()>)> {
    let router = Router::new()
        .route("/auth/login", post(legacy_login))
        .route("/auth/login/begin", post(begin_http_login))
        .route("/auth/login/complete", post(complete_http_login))
        .with_state(flows);
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let address = listener.local_addr()?;
    let (shutdown, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let _ = openrust::http::axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = receiver.await;
            })
            .await;
    });
    Ok((format!("http://{address}"), shutdown, task))
}

async fn start_router(router: Router) -> TestResult<(String, oneshot::Sender<()>, JoinHandle<()>)> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let address = listener.local_addr()?;
    let (shutdown, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let _ = openrust::http::axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = receiver.await;
            })
            .await;
    });
    Ok((format!("http://{address}"), shutdown, task))
}

async fn build_app(
    database_url: &str,
    prefix: &str,
    active_key: &str,
    keys: BTreeMap<String, String>,
) -> Result<App> {
    let key_entries = keys
        .iter()
        .map(|(id, key)| format!("{id} = {key:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    let source = format!(
        "[database]\nurl = {database_url:?}\ntable_prefix = {prefix:?}\n\n[security.encryption]\nactive_key_id = {active_key:?}\n[security.encryption.keys]\n{key_entries}\n"
    );
    let figment = Figment::new().merge(figment::providers::Toml::string(&source));
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment);
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(DatabaseServiceProvider)
        .provider(PasswordSettings(PasswordConfig {
            memory_kib: 19_456,
            iterations: 1,
            parallelism: 1,
        }))
        .provider(AuthServiceProvider::new())
        .provider(JwtSettings::new(
            "mfa-test-issuer",
            "mfa-test-audience",
            "mfa-test-jwt-key",
            &BTreeMap::from([("mfa-test-jwt-key".to_owned(), STANDARD.encode([0x6d; 32]))]),
        ))
        .provider(JwtServiceProvider)
        .provider(EncryptionServiceProvider)
        .provider(SessionServiceProvider::new())
        .provider(MfaServiceProvider)
        .provider(AuthFlowsServiceProvider::new())
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes: Routes| {
            routes
                .post("/mfa-test/session/seed", seed_session)
                .merge(
                    Routes::new()
                        .get("/mfa-test/session/protected", protected_session)
                        .layer(AuthLayer::required(AuthMode::Session)),
                )
                .post("/mfa-test/session/login", session_login)
        }));
    builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)
}

fn decode_base32(encoded: &str) -> TestResult<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = 0_u32;
    let mut bits = 0_u8;
    for byte in encoded.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'2'..=b'7' => byte - b'2' + 26,
            _ => return Err("unexpected character in TOTP secret".into()),
        };
        buffer = ((buffer << 5) | u32::from(value)) & 0xffff;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            output.push(<u8 as std::convert::TryFrom<u32>>::try_from(
                (buffer >> bits) & 0xff,
            )?);
        }
    }
    Ok(output)
}

fn totp(secret: &[u8], step: u64) -> TestResult<String> {
    let mut mac = <HmacSha1 as KeyInit>::new_from_slice(secret)?;
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[19] & 0x0f);
    let binary = (u32::from(digest[offset] & 0x7f) << 24)
        | (u32::from(digest[offset + 1]) << 16)
        | (u32::from(digest[offset + 2]) << 8)
        | u32::from(digest[offset + 3]);
    Ok(format!("{:06}", binary % 1_000_000))
}

fn current_totp(secret: &[u8]) -> TestResult<String> {
    totp(secret, current_step()?)
}

fn current_step() -> TestResult<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() / 30)
}

fn code_hash(code: &str) -> String {
    format!("{:x}", Sha256::digest(code.as_bytes()))
}

#[cfg(feature = "auth-oidc")]
fn oidc_state(url: &url::Url) -> TestResult<String> {
    url.query_pairs()
        .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
        .ok_or_else(|| "OIDC authorization URL is missing state".into())
}

async fn create_verified_user(flows: &AuthFlows, db: &Db, email: &str) -> TestResult<i64> {
    let source = format!("register-{email}");
    let verification = flows
        .register(email, "correct horse battery staple", &source)
        .await?
        .ok_or("test account was not registered")?;
    if !flows
        .verify_email(email, verification.code(), &format!("verify-{email}"))
        .await?
    {
        return Err("test account email was not verified".into());
    }
    let users = db.quoted_table_name("users")?;
    let connection = db.connection()?;
    let backend = connection.get_database_backend();
    let sql = match backend {
        DbBackend::Postgres => format!("SELECT id FROM {users} WHERE email = $1"),
        DbBackend::MySql => format!("SELECT id FROM {users} WHERE email = ?"),
        _ => return Err("unexpected test backend".into()),
    };
    let row = connection
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            sql,
            [email.to_owned().into()],
        ))
        .await?
        .ok_or("verified account was not present")?;
    Ok(row.try_get("", "id")?)
}

fn seed_from_enrollment(enrollment: &TotpEnrollment) -> TestResult<Vec<u8>> {
    let uri = url::Url::parse(enrollment.provisioning_uri())?;
    let encoded = uri
        .query_pairs()
        .find_map(|(name, value)| (name == "secret").then(|| value.into_owned()))
        .ok_or("provisioning URI did not include a seed")?;
    decode_base32(&encoded)
}

async fn begin_challenge(
    flows: &AuthFlows,
    email: &str,
    source: &str,
) -> TestResult<MfaLoginChallenge> {
    match flows
        .begin_login(email, "correct horse battery staple", source)
        .await?
    {
        LoginAttempt::MfaRequired(challenge) => Ok(challenge),
        LoginAttempt::Authenticated(_) => Err("MFA account bypassed its challenge".into()),
    }
}

async fn database_scalar_i64(db: &Db, sql: &str) -> TestResult<i64> {
    let connection = db.connection()?;
    let row = connection
        .query_one_raw(Statement::from_string(
            connection.get_database_backend(),
            sql.to_owned(),
        ))
        .await?
        .ok_or("query did not return a row")?;
    Ok(row.try_get("", "value")?)
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "一个隔离数据库完整验证 MFA 的事务和 HTTP 生命周期"
)]
async fn mfa_lifecycle_is_shared_secure_and_reachable_over_real_tcp() -> TestResult<()> {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping MFA integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let backend = DatabaseBackend::from_url(&url)?;
    if !backend.is_enabled() {
        eprintln!("skipping MFA integration: configured backend feature is disabled");
        return Ok(());
    }

    let prefix = "mfat_";
    let names = DatabaseTableNames::new(prefix, BTreeMap::new())?;
    let database = TestDatabase::create_with_table_names::<MfaMigrator>(&url, names).await?;
    let key_old = STANDARD.encode([0x31_u8; 32]);
    let key_new = STANDARD.encode([0x32_u8; 32]);
    let app = build_app(
        database.url(),
        prefix,
        "mfa-old",
        BTreeMap::from([("mfa-old".to_owned(), key_old.clone())]),
    )
    .await?;
    let flows = (*app.state().resolve::<AuthFlows>().await?).clone();
    let db = (*app.state().resolve::<Db>().await?).clone();
    let jwt = (*app.state().resolve::<JwtTokens>().await?).clone();
    let sea_backend = db.backend()?;
    let email = "mfa.user@example.test";
    let user_id = create_verified_user(&flows, &db, email).await?;

    let pre_mfa = flows
        .login(email, "correct horse battery staple", "before-mfa")
        .await?;
    let enrollment = flows
        .begin_totp_enrollment(
            user_id,
            email,
            "correct horse battery staple",
            "OpenRust Test",
            "enroll-first",
        )
        .await?;
    let secret = seed_from_enrollment(&enrollment)?;
    let factors = db.quoted_table_name("auth_mfa_factors")?;
    assert_eq!(
        database_scalar_i64(&db, &format!("SELECT COUNT(*) AS value FROM {factors}")).await?,
        0,
        "an unconfirmed enrollment must not enable MFA"
    );
    let mut invalid = current_totp(&secret)?;
    invalid.replace_range(0..1, if invalid.starts_with('0') { "1" } else { "0" });
    assert!(
        flows
            .confirm_totp_enrollment(user_id, &invalid, "enroll-confirm-bad")
            .await
            .is_err()
    );
    assert_eq!(
        database_scalar_i64(&db, &format!("SELECT COUNT(*) AS value FROM {factors}")).await?,
        0,
        "an invalid enrollment code must not activate MFA"
    );
    let enrollment_step = current_step()?;
    let enrollment_code = totp(&secret, enrollment_step)?;
    let recovery = flows
        .confirm_totp_enrollment(user_id, &enrollment_code, "enroll-confirm-good")
        .await?;
    assert_eq!(recovery.expose().len(), 10);
    assert!(format!("{recovery:?}").contains("[REDACTED]"));
    assert!(
        flows
            .refresh(pre_mfa.refresh_token().expose(), "old-refresh")
            .await
            .is_err()
    );

    let recovery_table = db.quoted_table_name("auth_mfa_recovery_codes")?;
    let recovery_hashes = db
        .connection()?
        .query_all_raw(Statement::from_string(
            sea_backend,
            format!("SELECT code_hash FROM {recovery_table} WHERE user_id = {user_id}"),
        ))
        .await?;
    assert_eq!(recovery_hashes.len(), recovery.expose().len());
    let stored_hashes = recovery_hashes
        .iter()
        .map(|row| row.try_get::<String>("", "code_hash"))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert!(
        recovery
            .expose()
            .iter()
            .all(|code| !stored_hashes.contains(code) && stored_hashes.contains(&code_hash(code)))
    );

    #[cfg(feature = "auth-oidc")]
    {
        let provider = std::sync::Arc::new(oidc_mfa::Provider::new());
        let provider_trait: std::sync::Arc<dyn openrust::auth::ExternalIdentityProvider> =
            provider.clone();
        let encryptor = (*app
            .state()
            .resolve::<openrust::security::Encryptor>()
            .await?)
            .clone();
        let external = openrust::auth::ExternalIdentityFlows::new(
            db.clone(),
            flows.clone(),
            encryptor,
            [provider_trait],
        )?;
        provider.set_identity("bind-mfa", oidc_mfa::identity()?)?;
        let binding = external.authorize("mfa-test-idp", "oidc-mfa-bind").await?;
        let binding_state = oidc_state(binding.url())?;
        external
            .bind(
                user_id,
                email,
                "correct horse battery staple",
                "oidc-mfa-bind",
                "mfa-test-idp",
                &binding_state,
                "bind-mfa",
                binding.browser_binding_cookie(),
            )
            .await?;
        provider.set_identity("login-mfa", oidc_mfa::identity()?)?;
        let login = external.authorize("mfa-test-idp", "oidc-mfa-login").await?;
        let login_state = oidc_state(login.url())?;
        let result = external
            .complete(
                "mfa-test-idp",
                &login_state,
                "login-mfa",
                login.browser_binding_cookie(),
                "oidc-mfa-login",
            )
            .await?;
        assert!(matches!(result, LoginAttempt::MfaRequired(_)));
    }

    assert!(
        flows
            .login(email, "correct horse battery staple", "legacy-login")
            .await
            .is_err()
    );
    let (base, shutdown, server) = start_http(flows.clone()).await?;
    let (session_base, session_shutdown, session_server) =
        start_router(build_router(&app)?).await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let new_session_login = client
        .post(format!("{session_base}/mfa-test/session/login"))
        .send()
        .await?;
    let new_session_body: serde_json::Value = new_session_login.json().await?;
    assert_eq!(
        new_session_body
            .get("code")
            .and_then(serde_json::Value::as_str),
        Some("unauthorized")
    );
    let seeded = client
        .post(format!("{session_base}/mfa-test/session/seed"))
        .json(&user_id)
        .send()
        .await?;
    let cookie = seeded
        .headers()
        .get("set-cookie")
        .ok_or("session seed did not issue a cookie")?
        .to_str()?
        .split(';')
        .next()
        .ok_or("empty session cookie")?
        .to_owned();
    let existing_session = client
        .get(format!("{session_base}/mfa-test/session/protected"))
        .header("cookie", cookie)
        .send()
        .await?;
    let existing_session_body: serde_json::Value = existing_session.json().await?;
    assert_eq!(
        existing_session_body
            .get("code")
            .and_then(serde_json::Value::as_str),
        Some("unauthorized"),
        "MFA-enabled account bypassed through Session"
    );
    let _ = session_shutdown.send(());
    session_server.await?;
    let legacy = client
        .post(format!("{base}/auth/login"))
        .header(CONTENT_TYPE, "application/json")
        .json(&serde_json::json!({"email": email, "password": "correct horse battery staple"}))
        .send()
        .await?;
    assert_eq!(legacy.status(), StatusCode::UNAUTHORIZED);

    let started = client
        .post(format!("{base}/auth/login/begin"))
        .json(&serde_json::json!({"email": email, "password": "correct horse battery staple"}))
        .send()
        .await?;
    assert_eq!(started.status(), StatusCode::OK);
    let started_json: serde_json::Value = started.json().await?;
    assert!(started_json.get("access_token").is_none());
    assert!(started_json.get("refresh_token").is_none());
    let challenge: HttpChallenge = serde_json::from_value(started_json)?;
    let totp_code = totp(&secret, enrollment_step + 1)?;
    let completed = client
        .post(format!("{base}/auth/login/complete"))
        .json(&serde_json::json!({"challenge": challenge.challenge, "code": totp_code}))
        .send()
        .await?;
    let completed_status = completed.status();
    let completed_body: serde_json::Value = completed.json().await?;
    assert_eq!(completed_status, StatusCode::OK, "{completed_body}");
    let issued: HttpCredentials = serde_json::from_value(completed_body)?;
    assert_eq!(jwt.verify_access_token(&issued.access_token)?, user_id);
    drop(client);
    let _ = shutdown.send(());
    server.await?;

    let exhausted = begin_challenge(&flows, email, "attempt-limit").await?;
    for _ in 0..5 {
        assert!(
            flows
                .complete_mfa_login(exhausted.expose(), "000000", "attempt-limit")
                .await
                .is_err()
        );
    }
    assert!(
        flows
            .complete_mfa_login(exhausted.expose(), &recovery.expose()[9], "attempt-limit")
            .await
            .is_err()
    );

    let replay_challenge = begin_challenge(&flows, email, "totp-replay").await?;
    assert!(
        flows
            .complete_mfa_login(replay_challenge.expose(), &totp_code, "totp-replay",)
            .await
            .is_err()
    );

    let concurrent_a = begin_challenge(&flows, email, "recovery-race-a").await?;
    let concurrent_b = begin_challenge(&flows, email, "recovery-race-b").await?;
    let recovery_code = recovery.expose()[0].clone();
    let (first, second) = tokio::join!(
        flows.complete_mfa_login(concurrent_a.expose(), &recovery_code, "recovery-race-a"),
        flows.complete_mfa_login(concurrent_b.expose(), &recovery_code, "recovery-race-b"),
    );
    let consumed_count =
        usize::from(u8::from(first.is_ok())) + usize::from(u8::from(second.is_ok()));
    assert_eq!(consumed_count, 1);

    let expired = begin_challenge(&flows, email, "expired-challenge").await?;
    let challenges = db.quoted_table_name("auth_mfa_challenges")?;
    let now = Utc::now().naive_utc() - chrono::Duration::seconds(1);
    let expire_sql = match sea_backend {
        DbBackend::Postgres => {
            format!("UPDATE {challenges} SET expires_at = $1 WHERE token_hash = $2")
        }
        DbBackend::MySql => format!("UPDATE {challenges} SET expires_at = ? WHERE token_hash = ?"),
        _ => return Err("unexpected test backend".into()),
    };
    db.connection()?
        .execute_raw(Statement::from_sql_and_values(
            sea_backend,
            expire_sql,
            [now.into(), code_hash(expired.expose()).into()],
        ))
        .await?;
    assert!(
        flows
            .complete_mfa_login(expired.expose(), &recovery.expose()[1], "expired-challenge",)
            .await
            .is_err()
    );

    drop(app);
    let rotated = build_app(
        database.url(),
        prefix,
        "mfa-new",
        BTreeMap::from([
            ("mfa-old".to_owned(), key_old),
            ("mfa-new".to_owned(), key_new),
        ]),
    )
    .await?;
    let rotated_flows = (*rotated.state().resolve::<AuthFlows>().await?).clone();
    let rotated_db = (*rotated.state().resolve::<Db>().await?).clone();
    let rotated_jwt = (*rotated.state().resolve::<JwtTokens>().await?).clone();
    let rotated_challenge = begin_challenge(&rotated_flows, email, "rotated-key-login").await?;
    let rotated_credentials = rotated_flows
        .complete_mfa_login(
            rotated_challenge.expose(),
            &recovery.expose()[2],
            "rotated-key-login",
        )
        .await?;
    assert_eq!(
        rotated_jwt.verify_access_token(rotated_credentials.access_token())?,
        user_id
    );

    let source_bound = begin_challenge(&rotated_flows, email, "source-bound-login").await?;
    assert!(
        rotated_flows
            .complete_mfa_login(
                source_bound.expose(),
                &recovery.expose()[8],
                "different-request-source",
            )
            .await
            .is_err()
    );
    assert!(
        rotated_flows
            .complete_mfa_login(
                source_bound.expose(),
                &recovery.expose()[8],
                "source-bound-login",
            )
            .await
            .is_ok()
    );

    let second_email = "mfa.rotated@example.test";
    let second_user_id = create_verified_user(&rotated_flows, &rotated_db, second_email).await?;
    let second_enrollment = rotated_flows
        .begin_totp_enrollment(
            second_user_id,
            second_email,
            "correct horse battery staple",
            "OpenRust Test",
            "rotated-enroll",
        )
        .await?;
    let pending = rotated_db.quoted_table_name("auth_mfa_enrollments")?;
    let pending_ciphertext = rotated_db
        .connection()?
        .query_one_raw(Statement::from_string(
            sea_backend,
            format!("SELECT encrypted_secret FROM {pending} WHERE user_id = {second_user_id}"),
        ))
        .await?
        .ok_or("pending enrollment was not stored")?
        .try_get::<String>("", "encrypted_secret")?;
    assert!(pending_ciphertext.starts_with("or1.mfa-new."));
    let _ = second_enrollment;

    let regenerated = rotated_flows
        .regenerate_mfa_recovery_codes(
            user_id,
            email,
            "correct horse battery staple",
            &recovery.expose()[3],
            "recovery-regenerate",
        )
        .await?;
    assert!(
        rotated_flows
            .refresh(
                rotated_credentials.refresh_token().expose(),
                "revoked-by-regeneration"
            )
            .await
            .is_err()
    );

    let before_disable_challenge = begin_challenge(&rotated_flows, email, "before-disable").await?;
    let before_disable = rotated_flows
        .complete_mfa_login(
            before_disable_challenge.expose(),
            &regenerated.expose()[0],
            "before-disable",
        )
        .await?;
    rotated_flows
        .disable_mfa(
            user_id,
            email,
            "correct horse battery staple",
            &regenerated.expose()[1],
            "disable-mfa",
        )
        .await?;
    assert!(
        rotated_flows
            .refresh(
                before_disable.refresh_token().expose(),
                "revoked-by-disable"
            )
            .await
            .is_err()
    );

    let audit = rotated_db.quoted_table_name("audit_records")?;
    let actions = rotated_db
        .connection()?
        .query_all_raw(Statement::from_string(
            sea_backend,
            format!("SELECT action FROM {audit} WHERE object_id = '{user_id}'"),
        ))
        .await?;
    let actions = actions
        .iter()
        .map(|row| row.try_get::<String>("", "action"))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for expected in [
        "auth.mfa.enabled",
        "auth.mfa.login",
        "auth.mfa.recovery_codes_reset",
        "auth.mfa.disabled",
    ] {
        assert!(actions.iter().any(|action| action == expected));
    }

    drop(rotated_jwt);
    drop(rotated_db);
    drop(rotated_flows);
    drop(rotated);
    drop(jwt);
    drop(db);
    drop(flows);
    database.close().await?;
    Ok(())
}

struct TestRoutes<F>(std::sync::Arc<F>);
impl<F: Fn(openrust::http::Routes) -> openrust::http::Routes> TestRoutes<F> {
    fn new(routes: F) -> Self {
        Self(std::sync::Arc::new(routes))
    }
}
impl<F> openrust::core::ServiceProvider for TestRoutes<F>
where
    F: Fn(openrust::http::Routes) -> openrust::http::Routes + Send + Sync + 'static,
{
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        let routes = std::sync::Arc::clone(&self.0);
        Box::pin(async move {
            app.routes(move |value| routes(value))?;
            Ok(())
        })
    }
}

struct JwtSettings(serde_json::Value);
impl JwtSettings {
    fn new(
        issuer: &str,
        audience: &str,
        active_key_id: &str,
        keys: &std::collections::BTreeMap<String, String>,
    ) -> Self {
        Self(serde_json::json!({"issuer":issuer,"audience":audience,
            "active_key_id":active_key_id,"keys":keys}))
    }
}
impl openrust::core::ServiceProvider for JwtSettings {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::core::RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move { app.merge_defaults("auth.jwt", self.0.clone()) })
    }
}

struct PasswordSettings(openrust::auth::PasswordConfig);
impl openrust::core::ServiceProvider for PasswordSettings {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::core::RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.merge_defaults("auth.password", serde_json::json!({"memory_kib":self.0.memory_kib,"iterations":self.0.iterations,"parallelism":self.0.parallelism}))
        })
    }
}
