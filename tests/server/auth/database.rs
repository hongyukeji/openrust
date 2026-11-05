//! 验证 PostgreSQL 和 MySQL 上共享账户与凭据状态的集成行为。
#![cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]

use std::{error::Error as StdError, sync::Arc};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use figment::{Figment, providers::Format};
use openrust::audit::{
    Audit, AuditEvent, AuditOutcome, AuditPolicy, AuditSubject, DatabaseAuditMigration,
    DatabaseAuditSink, DatabaseAuditTenantMigration,
};
use openrust::auth::{
    AuthFlows, AuthLayer, AuthMigration, AuthMode, AuthPolicyMigration, AuthRefreshTokenMigration,
    AuthServiceProvider, AuthSource, AuthVerificationCodeMigration, CurrentUser, JwtConfig,
    JwtServiceProvider, JwtTokens, PersonalAccessTokens, Policy, RefreshTokens, Session,
    SessionServiceProvider, VerificationCodePolicy, VerificationCodes,
};
use openrust::core::RateLimiter;
use openrust::database::sea_orm::{ConnectOptions, ConnectionTrait, Database, Statement};
use openrust::database::sea_orm_migration::{MigratorTrait, prelude::*};
use openrust::database::{
    DatabaseBackend, DatabaseRateLimitMigration, DatabaseRateLimitStore, DatabaseServiceProvider,
    DatabaseTableNames, Db,
};
use openrust::events::EventsServiceProvider;
use openrust::http::axum::body::{Body, to_bytes};
use openrust::http::axum::http::{Request, StatusCode};
use openrust::http::{HttpServiceProvider, Inject, Json, Routes, build_router};
use openrust::session::{GuardedDatabaseSessionStore, SessionStore};
use openrust::session::{SessionId as Id, SessionRecord as Record};
use openrust::signing::{
    DatabaseSignedLinkRevocationStore, SignedLinkRevocationMigration, SignedLinkRevocationStore,
    SignedLinks, SignedLinksConfig, SignedLinksServiceProvider,
};
use openrust::testing::TestDatabase;
use openrust::{App, Result, config::FrameworkConfig};
use sha2::{Digest, Sha256};
use time::Duration;
use tower::ServiceExt;

type TestResult = Result<(), Box<dyn StdError>>;

fn require_auth_contract(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

struct AuthMigrator;

impl MigratorTrait for AuthMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(AuthMigration),
            Box::new(openrust::session::GuardedSessionMigration),
            Box::new(openrust::session::GuardedSessionGcMigration),
            Box::new(AuthRefreshTokenMigration),
            Box::new(AuthVerificationCodeMigration),
            Box::new(AuthPolicyMigration),
            Box::new(DatabaseRateLimitMigration),
            Box::new(DatabaseAuditMigration),
            Box::new(DatabaseAuditTenantMigration),
            Box::new(SignedLinkRevocationMigration),
        ]
    }
}

struct Marker;

async fn current_user(user: CurrentUser, Inject(_marker): Inject<Marker>) -> Result<Json<String>> {
    Ok(Json(format!(
        "{}:{}:{}",
        user.id(),
        user.source() == AuthSource::PersonalAccessToken,
        user.has_ability("posts:read")
    )))
}

async fn create_session(session: Session) -> Result<Json<&'static str>> {
    session
        .put("openrust.auth.user_id", 1_i64.into())
        .map_err(openrust::Error::internal)?;
    session
        .put("openrust.auth.csrf_hash", hex_sha256("csrf-secret").into())
        .map_err(openrust::Error::internal)?;
    Ok(Json("created"))
}

async fn logout(session: Session) -> Result<Json<&'static str>> {
    session.flush().map_err(openrust::Error::internal)?;
    Ok(Json("logged out"))
}

async fn check_policy(user: CurrentUser, Inject(policy): Inject<Policy>) -> Result<Json<bool>> {
    Ok(Json(policy.allows(&user, "posts:read").await?))
}

async fn require_policy(
    user: CurrentUser,
    Inject(policy): Inject<Policy>,
) -> Result<Json<&'static str>> {
    policy.authorize(&user, "posts:read").await?;
    Ok(Json("allowed"))
}

async fn app_router(url: &str, table_prefix: &str) -> Result<(App, openrust::http::axum::Router)> {
    let source = format!(
        "[session]\ndriver = 'database-guarded'\n[database]\nurl = {url:?}\ntable_prefix = {table_prefix:?}\n\n[database.table_names]\nusers = \"mapped_accounts\"\n"
    );
    let figment = Figment::new().merge(figment::providers::Toml::string(&source));
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment);
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(DatabaseServiceProvider)
        .provider(openrust::audit::AuditServiceProvider::database())
        .provider(SessionServiceProvider::new())
        .provider(JwtSettings::new(
            "testing-issuer",
            "testing-audience",
            "auth-test-key",
            &std::collections::BTreeMap::from([(
                "auth-test-key".to_owned(),
                STANDARD.encode([0x53_u8; 32]),
            )]),
        ))
        .provider(JwtServiceProvider)
        .provider(SignedLinksServiceProvider::with_config(
            SignedLinksConfig::new(
                "testing-issuer",
                "link-key",
                std::collections::BTreeMap::from([(
                    "link-key".to_owned(),
                    STANDARD.encode([0x71_u8; 32]),
                )]),
            ),
        ))
        .provider(AuthServiceProvider::new())
        .provider(HttpServiceProvider)
        .provider(TestInstance(std::sync::Arc::new(Marker)))
        .provider(TestRoutes::new(|routes: Routes| {
            routes
                .merge(
                    Routes::new()
                        .get("/auth/current", current_user)
                        .post("/auth/current", current_user)
                        .layer(AuthLayer::required(AuthMode::SessionOrToken)),
                )
                .merge(Routes::new().get("/auth/session", create_session))
                .merge(Routes::new().post("/auth/logout", logout))
                .merge(
                    Routes::new()
                        .get("/auth/policy", check_policy)
                        .get("/auth/policy-authorize", require_policy)
                        .layer(AuthLayer::required(AuthMode::Jwt)),
                )
                .merge(
                    Routes::new()
                        .get("/auth/policy-pat", check_policy)
                        .layer(AuthLayer::required(AuthMode::PersonalAccessToken)),
                )
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    assert!(app.state().resolve::<Audit>().await.is_ok());
    assert!(app.state().resolve::<SignedLinks>().await.is_ok());
    assert_eq!(
        app.state().resolve::<Db>().await?.table_name("users")?,
        "mapped_accounts",
        "the configured full-name mapping must override the table prefix"
    );
    let router = build_router(&app)?;
    Ok((app, router))
}

fn hex_sha256(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(value.as_bytes()) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

async fn read_response(response: openrust::http::axum::response::Response) -> TestResult {
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let json: serde_json::Value = serde_json::from_slice(&body)?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"], "1:false:false");
    Ok(())
}

#[tokio::test]
async fn signed_link_revocation_is_shared_by_database_instances() -> TestResult {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping signed-link database integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let backend = DatabaseBackend::from_url(&url)?;
    if !backend.is_enabled() {
        return Ok(());
    }
    let table_names = openrust::database::DatabaseTableNames::new(
        "link_",
        std::collections::BTreeMap::from([(
            "signed_link_revocations".to_owned(),
            "mapped_link_revocations".to_owned(),
        )]),
    )?;
    let database =
        TestDatabase::create_with_table_names::<AuthMigrator>(&url, table_names.clone()).await?;
    let connection = database.connection()?;
    let db = Db::with_table_names(table_names.clone());
    db.initialize(connection.clone())?;
    let config = SignedLinksConfig::new(
        "testing-issuer",
        "link-key",
        std::collections::BTreeMap::from([("link-key".to_owned(), STANDARD.encode([0x71_u8; 32]))]),
    );
    let issuer = SignedLinks::new(config.clone())?.with_database_revocations(db.clone());
    let verifier = SignedLinks::new(config)?.with_database_revocations(db.clone());
    let link = issuer.issue("email.verify", "user:71", 60)?;
    verifier.verify(link.expose(), "email.verify").await?;
    issuer.revoke(link.expose()).await?;
    assert!(
        verifier
            .verify(link.expose(), "email.verify")
            .await
            .is_err()
    );
    assert_eq!(
        table_names.resolve("signed_link_revocations")?,
        "mapped_link_revocations"
    );
    let revocations = DatabaseSignedLinkRevocationStore::new(db);
    revocations.revoke(&"a".repeat(64), 0).await?;
    assert_eq!(revocations.purge_expired(1).await?, 1);
    drop(connection);
    database.close().await?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn auth_migration_and_shared_revocation_work_on_the_configured_database() -> TestResult {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!(
            "skipping authentication database integration: OPENRUST_TEST_DATABASE_URL is unset"
        );
        return Ok(());
    };
    let feature_backend = DatabaseBackend::from_url(&url)?;
    if !feature_backend.is_enabled() {
        eprintln!(
            "skipping authentication database integration: configured backend feature is disabled"
        );
        return Ok(());
    }

    let table_names = openrust::database::DatabaseTableNames::new(
        "it_",
        std::collections::BTreeMap::from([("users".to_owned(), "mapped_accounts".to_owned())]),
    )?;
    let database =
        TestDatabase::create_with_table_names::<AuthMigrator>(&url, table_names.clone()).await?;
    let connection = database.connection()?;
    let backend = connection.get_database_backend();
    let session_db = Db::with_table_names(table_names);
    session_db.initialize(connection.clone())?;
    let session_store = GuardedDatabaseSessionStore::new(session_db.clone());
    let collision_record = Record {
        id: Id::generate()?,
        data: serde_json::json!({}),
        expiry_date: time::OffsetDateTime::now_utc() + Duration::minutes(5),
    };
    assert_eq!(
        session_store
            .write(
                &collision_record,
                openrust::session::SessionWriteMode::Insert,
                openrust::session::DEFAULT_MAX_BYTES
            )
            .await?,
        openrust::session::SessionWriteOutcome::Written
    );
    assert_eq!(
        session_store
            .write(
                &collision_record,
                openrust::session::SessionWriteMode::Insert,
                openrust::session::DEFAULT_MAX_BYTES
            )
            .await?,
        openrust::session::SessionWriteOutcome::Conflict
    );
    let loaded = session_store
        .load(
            &collision_record.id,
            openrust::session::DEFAULT_MAX_BYTES,
            collision_record.expiry_date,
        )
        .await?
        .ok_or("Inserted Session missing")?;
    assert_eq!(loaded.id, collision_record.id);
    assert_eq!(loaded.data, collision_record.data);
    assert_eq!(
        loaded.expiry_date.unix_timestamp_nanos(),
        collision_record.expiry_date.unix_timestamp_nanos() / 1000 * 1000
    );
    session_store.delete(&collision_record.id).await?;
    assert_eq!(
        session_store
            .write(
                &collision_record,
                openrust::session::SessionWriteMode::Update,
                openrust::session::DEFAULT_MAX_BYTES
            )
            .await?,
        openrust::session::SessionWriteOutcome::Missing
    );
    assert!(
        session_store
            .load(
                &collision_record.id,
                openrust::session::DEFAULT_MAX_BYTES,
                time::OffsetDateTime::now_utc() + time::Duration::minutes(120)
            )
            .await?
            .is_none()
    );
    let expired_record = Record {
        id: Id::generate()?,
        data: serde_json::json!({}),
        expiry_date: time::OffsetDateTime::now_utc() - Duration::minutes(1),
    };
    assert_eq!(
        session_store
            .write(
                &expired_record,
                openrust::session::SessionWriteMode::Insert,
                openrust::session::DEFAULT_MAX_BYTES
            )
            .await?,
        openrust::session::SessionWriteOutcome::Written
    );
    assert!(
        session_store
            .load(
                &expired_record.id,
                openrust::session::DEFAULT_MAX_BYTES,
                time::OffsetDateTime::now_utc() + time::Duration::minutes(120)
            )
            .await?
            .is_none()
    );
    let user_sql = match backend {
        openrust::database::DbBackend::Postgres | openrust::database::DbBackend::MySql => {
            "INSERT INTO mapped_accounts (email, password_hash, created_at, updated_at) VALUES ('user@example.test', 'unused', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)"
        }
        _ => return Err("auth migration selected an unsupported backend".into()),
    };
    connection.execute_unprepared(user_sql).await?;
    let id_row = connection
        .query_one_raw(Statement::from_string(
            backend,
            "SELECT id FROM mapped_accounts WHERE email = 'user@example.test'".to_owned(),
        ))
        .await?
        .ok_or("auth account insert returned no row")?;
    let user_id = id_row.try_get::<i64>("", "id")?;

    let refresh_tokens = RefreshTokens::new(session_db.clone());
    let expires_at = chrono::Utc::now() + chrono::Duration::hours(1);
    let issued = refresh_tokens.issue(user_id, expires_at).await?;
    let initial_token = issued.expose().to_owned();
    let initial_hash = hex_sha256(&initial_token);
    let stored_refresh = connection
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            match backend {
                openrust::database::DbBackend::Postgres => {
                    "SELECT token_hash FROM it_auth_refresh_tokens WHERE token_hash = $1"
                }
                openrust::database::DbBackend::MySql => {
                    "SELECT token_hash FROM it_auth_refresh_tokens WHERE token_hash = ?"
                }
                _ => return Err("refresh migration selected an unsupported backend".into()),
            },
            [initial_hash.clone().into()],
        ))
        .await?
        .ok_or("refresh token was not persisted")?;
    assert_eq!(
        stored_refresh.try_get::<String>("", "token_hash")?,
        initial_hash
    );
    assert!(!initial_hash.contains(&initial_token));
    let rotated = refresh_tokens.rotate(&initial_token).await?;
    assert_eq!(rotated.user_id(), user_id);
    assert_eq!(rotated.expires_at().timestamp(), expires_at.timestamp());
    let successor = rotated.token().expose().to_owned();
    assert!(refresh_tokens.rotate(&initial_token).await.is_err());
    let family_row = connection
        .query_one_raw(Statement::from_string(
            backend,
            format!(
                "SELECT COUNT(*) AS revoked_count FROM it_auth_refresh_tokens WHERE token_hash IN ('{}', '{}') AND revoked_at IS NOT NULL",
                initial_hash,
                hex_sha256(&successor)
            ),
        ))
        .await?
        .ok_or("refresh family revocation query returned no row")?;
    assert_eq!(family_row.try_get::<i64>("", "revoked_count")?, 2);
    let single_revoked = refresh_tokens
        .issue(user_id, chrono::Utc::now() + chrono::Duration::hours(1))
        .await?;
    refresh_tokens.revoke(single_revoked.expose()).await?;
    assert!(
        refresh_tokens
            .rotate(single_revoked.expose())
            .await
            .is_err()
    );
    let account_token = refresh_tokens
        .issue(user_id, chrono::Utc::now() + chrono::Duration::hours(1))
        .await?;
    assert_eq!(refresh_tokens.revoke_all_for_user(user_id).await?, 1);
    assert!(refresh_tokens.rotate(account_token.expose()).await.is_err());

    let pat_sql = match backend {
        openrust::database::DbBackend::Postgres => {
            "INSERT INTO it_personal_access_tokens (user_id, name, token_prefix, token_hash, abilities, expires_at, created_at) VALUES ($1, 'integration', 'test', $2, '[\"posts:read\"]'::jsonb, NULL, CURRENT_TIMESTAMP)"
        }
        openrust::database::DbBackend::MySql => {
            "INSERT INTO it_personal_access_tokens (user_id, name, token_prefix, token_hash, abilities, expires_at, created_at) VALUES (?, 'integration', 'test', ?, '[\"posts:read\"]', NULL, CURRENT_TIMESTAMP)"
        }
        _ => return Err("auth migration selected an unsupported backend".into()),
    };
    let pat_hash = hex_sha256("pat-secret");
    connection
        .execute_raw(Statement::from_sql_and_values(
            backend,
            pat_sql,
            [user_id.into(), pat_hash.clone().into()],
        ))
        .await?;

    let (app_a, router_a) = app_router(database.url(), "it_").await?;
    let (app_b, router_b) = app_router(database.url(), "it_").await?;
    let verification_codes = app_a.state().resolve::<VerificationCodes>().await?;
    let verification_codes_b = app_b.state().resolve::<VerificationCodes>().await?;
    let code_policy = VerificationCodePolicy::new(
        6,
        std::time::Duration::from_secs(3600),
        std::time::Duration::from_secs(60),
        2,
    )?;
    let issued_code = verification_codes
        .issue("email.verify", "code-user@example.test", code_policy)
        .await?;
    let binding_hash = hex_sha256("email.verify\0code-user@example.test");
    let (verification_query, verification_value) = match backend {
        openrust::database::DbBackend::Postgres => (
            "SELECT key_hash, code_hash FROM it_verification_codes WHERE key_hash = $1",
            binding_hash.into(),
        ),
        openrust::database::DbBackend::MySql => (
            "SELECT key_hash, code_hash FROM it_verification_codes WHERE key_hash = ?",
            binding_hash.into(),
        ),
        _ => return Err("auth migration selected an unsupported backend".into()),
    };
    let verification_row = connection
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            verification_query,
            [verification_value],
        ))
        .await?
        .ok_or("verification challenge was not persisted")?;
    let persisted_key = verification_row.try_get::<String>("", "key_hash")?;
    let persisted_hash = verification_row.try_get::<String>("", "code_hash")?;
    assert_eq!(
        persisted_key,
        hex_sha256("email.verify\0code-user@example.test")
    );
    let hashing = app_a
        .state()
        .resolve::<openrust::hashing::HashManager>()
        .await?;
    let info = hashing.info(&persisted_hash)?;
    assert_eq!(info.algo_name, "bcrypt");
    assert_eq!(info.options.get("cost"), Some(&serde_json::json!(12)));
    assert!(
        hashing
            .check(
                issued_code.code(),
                Some(&persisted_hash),
                &openrust::hashing::HashOptions::new()
            )
            .await?
    );
    assert!(!persisted_hash.contains(issued_code.code()));
    match verification_codes
        .issue("email.verify", "code-user@example.test", code_policy)
        .await
    {
        Err(openrust::Error::RateLimitedWithRetryAfter(seconds)) => assert!(seconds > 0),
        Err(error) => return Err(format!("unexpected resend error: {error:?}").into()),
        Ok(_) => return Err("resend cooldown allowed a duplicate code".into()),
    }
    assert!(
        !verification_codes
            .verify(
                "password.reset",
                "code-user@example.test",
                issued_code.code()
            )
            .await?
    );
    assert!(
        !verification_codes
            .verify("email.verify", "other@example.test", issued_code.code())
            .await?
    );
    assert!(
        !verification_codes
            .verify("email.verify", "code-user@example.test", "not-a-code")
            .await?
    );
    let wrong_code = if issued_code.code() == "000000" {
        "000001"
    } else {
        "000000"
    };
    assert!(
        !verification_codes
            .verify("email.verify", "code-user@example.test", wrong_code)
            .await?
    );
    assert!(
        !verification_codes
            .verify("email.verify", "code-user@example.test", wrong_code)
            .await?
    );
    assert!(
        !verification_codes
            .verify("email.verify", "code-user@example.test", issued_code.code())
            .await?
    );

    let one_time_code = verification_codes
        .issue("email.verify", "race-user@example.test", code_policy)
        .await?;
    let first_codes = verification_codes.clone();
    let first_code = one_time_code.code().to_owned();
    let second_code = first_code.clone();
    let (first_result, second_result) = tokio::join!(
        first_codes.verify("email.verify", "race-user@example.test", &first_code),
        verification_codes_b.verify("email.verify", "race-user@example.test", &second_code),
    );
    assert_eq!(u8::from(first_result?) + u8::from(second_result?), 1);
    drop(first_codes);
    let expiry_policy = VerificationCodePolicy::new(
        6,
        std::time::Duration::from_secs(1),
        std::time::Duration::from_secs(1),
        2,
    )?;
    let expired_code = verification_codes
        .issue("email.verify", "expired-user@example.test", expiry_policy)
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
    assert!(
        !verification_codes
            .verify(
                "email.verify",
                "expired-user@example.test",
                expired_code.code()
            )
            .await?
    );
    let personal_access_tokens = app_a.state().resolve::<PersonalAccessTokens>().await?;
    let issued_pat = personal_access_tokens
        .issue(
            user_id,
            "integration lifecycle",
            &["posts:read".to_owned()],
            Some(chrono::Utc::now() + chrono::Duration::hours(1)),
        )
        .await?;
    let issued_pat_raw = issued_pat.expose().to_owned();
    assert!(!format!("{issued_pat:?}").contains(&issued_pat_raw));
    let pat_hash = hex_sha256(&issued_pat_raw);
    let stored_pat = connection
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            match backend {
                openrust::database::DbBackend::Postgres => {
                    "SELECT token_hash FROM it_personal_access_tokens WHERE token_hash = $1"
                }
                openrust::database::DbBackend::MySql => {
                    "SELECT token_hash FROM it_personal_access_tokens WHERE token_hash = ?"
                }
                _ => return Err("PAT migration selected an unsupported backend".into()),
            },
            [pat_hash.clone().into()],
        ))
        .await?
        .ok_or("issued PAT was not persisted")?;
    assert_eq!(stored_pat.try_get::<String>("", "token_hash")?, pat_hash);
    assert!(!pat_hash.contains(&issued_pat_raw));
    let listed_pats = personal_access_tokens.list_for_user(user_id).await?;
    let issued_pat_info = listed_pats
        .iter()
        .find(|pat| pat.token_prefix == issued_pat_raw.chars().take(24).collect::<String>())
        .ok_or("issued PAT metadata was not listed")?;
    assert_eq!(issued_pat_info.abilities, ["posts:read"]);
    assert!(
        personal_access_tokens
            .issue(user_id + 10_000, "unknown account", &[], None)
            .await
            .is_err()
    );
    assert_eq!(
        personal_access_tokens.list_for_user(user_id + 1).await?,
        Vec::new()
    );
    assert!(
        !personal_access_tokens
            .revoke(user_id + 1, issued_pat_info.id)
            .await?
    );
    assert!(
        personal_access_tokens
            .list_for_user(user_id)
            .await?
            .iter()
            .find(|pat| pat.id == issued_pat_info.id)
            .is_some_and(|pat| pat.revoked_at.is_none())
    );
    let expired_pat = personal_access_tokens
        .issue(
            user_id,
            "expired",
            &[],
            Some(chrono::Utc::now() - chrono::Duration::seconds(1)),
        )
        .await;
    assert!(expired_pat.is_err());
    let app_refresh_tokens = app_a.state().resolve::<RefreshTokens>().await?;
    let concurrent = app_refresh_tokens
        .issue(user_id, chrono::Utc::now() + chrono::Duration::hours(1))
        .await?;
    let concurrent_raw = concurrent.expose().to_owned();
    let (rotation_a, rotation_b) = tokio::join!(
        app_refresh_tokens.rotate(&concurrent_raw),
        app_refresh_tokens.rotate(&concurrent_raw)
    );
    assert_ne!(rotation_a.is_ok(), rotation_b.is_ok());
    let successful_rotation = rotation_a
        .ok()
        .or_else(|| rotation_b.ok())
        .ok_or("one concurrent refresh rotation must win")?;
    assert!(
        app_refresh_tokens
            .rotate(successful_rotation.token().expose())
            .await
            .is_err(),
        "the concurrent loser must revoke the family, including the winner's successor"
    );
    let revoke_race = app_refresh_tokens
        .issue(user_id, chrono::Utc::now() + chrono::Duration::hours(1))
        .await?;
    let revoke_race_raw = revoke_race.expose().to_owned();
    let (racing_rotation, _) = tokio::join!(
        app_refresh_tokens.rotate(&revoke_race_raw),
        app_refresh_tokens.revoke_all_for_user(user_id)
    );
    if let Ok(racing_rotation) = racing_rotation {
        assert!(
            app_refresh_tokens
                .rotate(racing_rotation.token().expose())
                .await
                .is_err(),
            "account-wide revocation must include a concurrent successor"
        );
    }
    let pat_request = Request::get("/auth/current")
        .header("host", "localhost")
        .header("authorization", format!("Bearer {issued_pat_raw}"))
        .body(Body::empty())?;
    let pat_response = router_a.clone().oneshot(pat_request).await?;
    let pat_body = to_bytes(pat_response.into_body(), usize::MAX).await?;
    let pat_json: serde_json::Value = serde_json::from_slice(&pat_body)?;
    assert_eq!(pat_json["data"], "1:true:true");
    let used_row = connection
        .query_one_raw(Statement::from_string(
            backend,
            format!(
                "SELECT COUNT(*) AS was_used FROM it_personal_access_tokens WHERE token_hash = '{pat_hash}' AND last_used_at IS NOT NULL"
            ),
        ))
        .await?
        .ok_or("personal token usage query returned no row")?;
    assert_eq!(used_row.try_get::<i64>("", "was_used")?, 1);

    let session_response = router_a
        .clone()
        .oneshot(
            Request::get("/auth/session")
                .header("host", "localhost")
                .body(Body::empty())?,
        )
        .await?;
    let set_cookie = session_response
        .headers()
        .get("set-cookie")
        .ok_or("Session middleware did not set a cookie")?
        .to_str()?;
    let model = openrust::http::Cookie::from_header(set_cookie)?;
    assert_eq!(model.name(), "openrust_session");
    assert!(model.http_only());
    assert_eq!(model.secure(), Some(false));
    assert_eq!(model.same_site(), Some(openrust::http::SameSite::Lax));
    let session_cookie = set_cookie
        .split(';')
        .next()
        .ok_or("Session cookie was empty")?
        .to_owned();
    read_response(
        router_a
            .clone()
            .oneshot(
                Request::get("/auth/current")
                    .header("host", "localhost")
                    .header("cookie", &session_cookie)
                    .body(Body::empty())?,
            )
            .await?,
    )
    .await?;
    let post_missing_csrf = router_a
        .clone()
        .oneshot(
            Request::post("/auth/current")
                .header("host", "localhost")
                .header("cookie", &session_cookie)
                .body(Body::empty())?,
        )
        .await?;
    let body = to_bytes(post_missing_csrf.into_body(), usize::MAX).await?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body)?["code"],
        "forbidden"
    );
    read_response(
        router_a
            .clone()
            .oneshot(
                Request::post("/auth/current")
                    .header("host", "localhost")
                    .header("cookie", &session_cookie)
                    .header("x-csrf-token", "csrf-secret")
                    .body(Body::empty())?,
            )
            .await?,
    )
    .await?;
    router_a
        .clone()
        .oneshot(
            Request::post("/auth/logout")
                .header("host", "localhost")
                .header("cookie", &session_cookie)
                .body(Body::empty())?,
        )
        .await?;
    let revoked_session = router_b
        .clone()
        .oneshot(
            Request::get("/auth/current")
                .header("host", "localhost")
                .header("cookie", &session_cookie)
                .body(Body::empty())?,
        )
        .await?;
    let revoked_body = to_bytes(revoked_session.into_body(), usize::MAX).await?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&revoked_body)?["code"],
        "unauthorized"
    );

    let pat_request = Request::get("/auth/current")
        .header("host", "localhost")
        .header("authorization", "Bearer pat-secret")
        .body(Body::empty())?;
    let pat_still_valid = router_b.clone().oneshot(pat_request).await?;
    let pat_body = to_bytes(pat_still_valid.into_body(), usize::MAX).await?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&pat_body)?["data"],
        "1:true:true"
    );

    assert!(
        personal_access_tokens
            .revoke(user_id, issued_pat_info.id)
            .await?
    );
    let revoked_pat = router_b
        .clone()
        .oneshot(
            Request::get("/auth/current")
                .header("host", "localhost")
                .header("authorization", format!("Bearer {issued_pat_raw}"))
                .body(Body::empty())?,
        )
        .await?;
    let revoked_pat_body = to_bytes(revoked_pat.into_body(), usize::MAX).await?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&revoked_pat_body)?["code"],
        "unauthorized"
    );
    let account_wide_pat = personal_access_tokens
        .issue(user_id, "account-wide revoke", &[], None)
        .await?;
    let account_wide_prefix = account_wide_pat
        .expose()
        .chars()
        .take(24)
        .collect::<String>();
    let account_wide_id = personal_access_tokens
        .list_for_user(user_id)
        .await?
        .into_iter()
        .find(|pat| pat.token_prefix == account_wide_prefix)
        .ok_or("account-wide PAT metadata was not listed")?
        .id;
    assert_eq!(
        personal_access_tokens.revoke_all_for_user(user_id).await?,
        2
    );
    assert!(
        personal_access_tokens
            .list_for_user(user_id)
            .await?
            .iter()
            .filter(|pat| pat.id == issued_pat_info.id || pat.id == account_wide_id)
            .all(|pat| pat.revoked_at.is_some())
    );
    drop(router_a);
    drop(router_b);
    drop(app_refresh_tokens);
    drop(personal_access_tokens);
    drop(refresh_tokens);
    drop(session_store);
    drop(session_db);
    drop(verification_codes);
    drop(verification_codes_b);
    let shutdown_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    app_a.shutdown(shutdown_deadline).await?;
    app_b.shutdown(shutdown_deadline).await?;
    drop(app_a);
    drop(app_b);
    drop(connection);
    database.close().await?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One fixture keeps the complete credential lifecycle on one isolated database.
async fn auth_flows_run_registration_login_refresh_reset_and_logout_on_the_configured_database()
-> TestResult {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping authentication flow integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let backend = DatabaseBackend::from_url(&url)?;
    if !backend.is_enabled() {
        eprintln!(
            "skipping authentication flow integration: configured backend feature is disabled"
        );
        return Ok(());
    }
    let table_names = openrust::database::DatabaseTableNames::new(
        "flow_",
        std::collections::BTreeMap::from([("users".to_owned(), "mapped_accounts".to_owned())]),
    )?;
    let database =
        TestDatabase::create_with_table_names::<AuthMigrator>(&url, table_names.clone()).await?;
    let db = Db::with_table_names(table_names);
    let connection = database.connection()?;
    connection
        .query_one_raw(Statement::from_string(
            connection.get_database_backend(),
            "SELECT id FROM mapped_accounts LIMIT 0".to_owned(),
        ))
        .await?;
    db.initialize(connection.clone())?;
    let passwords = std::sync::Arc::new(openrust::hashing::HashManager::new(
        openrust::hashing::HashingConfig {
            driver: "argon2id".into(),
            argon: openrust::hashing::ArgonConfig {
                memory: 19_456,
                time: 1,
                threads: 1,
                verify: true,
            },
            ..openrust::hashing::HashingConfig::default()
        },
    )?);
    let codes = VerificationCodes::new(
        openrust::auth::DatabaseVerificationCodeStore::new(db.clone()),
        passwords.clone(),
    );
    let jwt = JwtTokens::new(JwtConfig::new(
        "testings",
        "testing-client",
        "test-key",
        std::collections::BTreeMap::from([("test-key".to_owned(), STANDARD.encode([7_u8; 32]))]),
    ))?;
    let flows = AuthFlows::new(
        db.clone(),
        passwords,
        jwt.clone(),
        RefreshTokens::new(db.clone()),
        codes,
        RateLimiter::new(DatabaseRateLimitStore::new(db.clone())),
    )
    .with_code_policy(VerificationCodePolicy::new(
        6,
        std::time::Duration::from_secs(600),
        std::time::Duration::from_secs(1),
        5,
    )?);
    let (session_app, session_router) = app_router(database.url(), "flow_").await?;

    let code = flows
        .register("Flow.User@example.test", "initial-password", "127.0.0.1")
        .await?
        .ok_or("new account was not registered")?;
    assert!(
        flows
            .request_email_verification("flow.user@example.test", "127.0.0.1")
            .await?
            .is_none()
    );
    assert!(
        flows
            .register("flow.user@example.test", "ignored-password", "127.0.0.1")
            .await?
            .is_none()
    );
    connection
        .execute_unprepared(
            "ALTER TABLE mapped_accounts ADD CONSTRAINT reject_test_email CHECK (email <> 'reject@example.test')",
        )
        .await?;
    assert!(
        flows
            .register("reject@example.test", "initial-password", "127.0.0.1")
            .await
            .is_err(),
        "non-unique constraint failures must propagate instead of looking like duplicate emails"
    );
    assert!(
        flows
            .login("flow.user@example.test", "initial-password", "127.0.0.1")
            .await
            .is_err()
    );
    let wrong_code = if code.code() == "000000" {
        "000001"
    } else {
        "000000"
    };
    assert!(
        !flows
            .verify_email("flow.user@example.test", wrong_code, "127.0.0.1")
            .await?
    );
    assert!(
        flows
            .verify_email("flow.user@example.test", code.code(), "127.0.0.1")
            .await?
    );

    let session_store = Arc::new(GuardedDatabaseSessionStore::new(db.clone()));
    let session = Session::new(None, session_store.clone(), None)?;
    session.start().await?;
    session.put_typed("app.preference", "compact")?;
    session.save().await?;
    let old_session_id = session.id()?;
    let stale_request = Session::new(Some(old_session_id.clone()), session_store.clone(), None)?;
    stale_request.start().await?;
    assert_eq!(
        stale_request.get_typed::<String>("app.preference")?,
        Some("compact".to_owned())
    );
    assert!(
        flows
            .login_session(
                "flow.user@example.test",
                "wrong-password",
                "session-login-source",
                &session,
            )
            .await
            .is_err()
    );
    assert_eq!(session.id()?, old_session_id);
    assert!(session.get_typed::<i64>("openrust.auth.user_id")?.is_none());

    let session_credentials = flows
        .login_session(
            "flow.user@example.test",
            "initial-password",
            "session-login-source",
            &session,
        )
        .await?;
    assert_eq!(session_credentials.user_id(), 1);
    assert!(!session_credentials.csrf_token().contains('='));
    assert!(!format!("{session_credentials:?}").contains(session_credentials.csrf_token()));
    assert_ne!(session.id()?, old_session_id);
    session.save().await?;
    let logged_in_session_id = session.id()?;
    assert_ne!(logged_in_session_id, old_session_id);
    let csrf_hash = session
        .get_typed::<String>("openrust.auth.csrf_hash")?
        .ok_or("CSRF digest was not stored in the session")?;
    assert_eq!(csrf_hash, hex_sha256(session_credentials.csrf_token()));
    assert_ne!(csrf_hash, session_credentials.csrf_token());
    assert_eq!(
        session.get_typed::<String>("app.preference")?,
        Some("compact".to_owned())
    );
    assert!(
        session_store
            .load(
                &old_session_id,
                openrust::session::DEFAULT_MAX_BYTES,
                time::OffsetDateTime::now_utc() + time::Duration::minutes(120)
            )
            .await?
            .is_none()
    );

    let session_cookie = format!("openrust_session={logged_in_session_id}");
    read_response(
        session_router
            .clone()
            .oneshot(
                Request::get("/auth/current")
                    .header("host", "localhost")
                    .header("cookie", &session_cookie)
                    .body(Body::empty())?,
            )
            .await?,
    )
    .await?;
    let denied_post = session_router
        .clone()
        .oneshot(
            Request::post("/auth/current")
                .header("host", "localhost")
                .header("cookie", &session_cookie)
                .body(Body::empty())?,
        )
        .await?;
    let denied_body = to_bytes(denied_post.into_body(), usize::MAX).await?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&denied_body)?["code"],
        "forbidden"
    );
    read_response(
        session_router
            .clone()
            .oneshot(
                Request::post("/auth/current")
                    .header("host", "localhost")
                    .header("cookie", &session_cookie)
                    .header("x-csrf-token", session_credentials.csrf_token())
                    .body(Body::empty())?,
            )
            .await?,
    )
    .await?;

    stale_request.put("app.stale", true.into())?;
    assert!(stale_request.save().await.is_err());
    assert!(
        session_store
            .load(
                &old_session_id,
                openrust::session::DEFAULT_MAX_BYTES,
                time::OffsetDateTime::now_utc() + time::Duration::minutes(120)
            )
            .await?
            .is_none()
    );
    assert_eq!(session.get_typed::<i64>("openrust.auth.user_id")?, Some(1));

    flows
        .logout_session("session-logout-source", &session)
        .await?;
    assert_ne!(session.id()?, old_session_id);
    session.save().await?;
    let anonymous_session_id = session.id()?;
    assert_ne!(anonymous_session_id, logged_in_session_id);
    assert!(session.get_typed::<i64>("openrust.auth.user_id")?.is_none());
    assert!(
        session
            .get_typed::<String>("openrust.auth.csrf_hash")?
            .is_none()
    );
    assert_eq!(
        session.get_typed::<String>("app.preference")?,
        Some("compact".to_owned())
    );
    assert!(
        session_store
            .load(
                &logged_in_session_id,
                openrust::session::DEFAULT_MAX_BYTES,
                time::OffsetDateTime::now_utc() + time::Duration::minutes(120)
            )
            .await?
            .is_none()
    );
    let revoked_session = session_router
        .clone()
        .oneshot(
            Request::get("/auth/current")
                .header("host", "localhost")
                .header("cookie", &session_cookie)
                .body(Body::empty())?,
        )
        .await?;
    let revoked_body = to_bytes(revoked_session.into_body(), usize::MAX).await?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&revoked_body)?["code"],
        "unauthorized"
    );

    let credentials = flows
        .login("flow.user@example.test", "initial-password", "127.0.0.1")
        .await?;
    assert!(jwt.verify_access_token(credentials.access_token()).is_ok());
    let rotated = flows
        .refresh(credentials.refresh_token().expose(), "127.0.0.1")
        .await?;
    assert!(jwt.verify_access_token(rotated.access_token()).is_ok());
    assert!(
        flows
            .refresh(credentials.refresh_token().expose(), "127.0.0.1")
            .await
            .is_err()
    );

    let reset_code = flows
        .request_password_reset("flow.user@example.test", "127.0.0.1")
        .await?
        .ok_or("eligible account did not receive reset code")?;
    assert!(
        flows
            .request_password_reset("flow.user@example.test", "127.0.0.1")
            .await?
            .is_none()
    );
    assert!(
        flows
            .request_password_reset("unknown@example.test", "127.0.0.1")
            .await?
            .is_none()
    );
    assert!(
        flows
            .reset_password(
                "flow.user@example.test",
                reset_code.code(),
                "replacement-password",
                "127.0.0.1",
            )
            .await?
    );
    assert!(
        flows
            .login("flow.user@example.test", "initial-password", "127.0.0.1")
            .await
            .is_err()
    );
    let replacement = flows
        .login(
            "flow.user@example.test",
            "replacement-password",
            "127.0.0.1",
        )
        .await?;
    flows
        .logout(replacement.refresh_token().expose(), "127.0.0.1")
        .await?;
    assert!(
        flows
            .refresh(replacement.refresh_token().expose(), "127.0.0.1")
            .await
            .is_err()
    );

    drop(stale_request);
    drop(session);
    drop(session_store);
    drop(session_router);
    drop(session_app);
    drop(flows);
    drop(db);
    drop(connection);
    database.close().await?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // 将真实数据库、并发突发和资源恢复保留在同一验收场景中。
#[allow(clippy::print_stdout)] // 性能样本由 --nocapture 输出并写入验收记录。
async fn auth_login_burst_recovers_under_a_two_connection_database_pool() -> TestResult {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping login burst integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let backend = DatabaseBackend::from_url(&url)?;
    if !backend.is_enabled() {
        eprintln!("skipping login burst integration: configured backend feature is disabled");
        return Ok(());
    }

    let table_names = DatabaseTableNames::new("load_", std::collections::BTreeMap::new())?;
    let database =
        TestDatabase::create_with_table_names::<AuthMigrator>(&url, table_names.clone()).await?;
    let mut options = ConnectOptions::new(database.url().to_owned());
    options
        .max_connections(2)
        .min_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(5));
    let connection = Database::connect(options).await?;
    let db = Db::with_table_names(table_names);
    db.initialize(connection.clone())?;
    let passwords = std::sync::Arc::new(
        openrust::hashing::HashManager::with_max_concurrent_operations(
            openrust::hashing::HashingConfig {
                driver: "argon2id".into(),
                argon: openrust::hashing::ArgonConfig {
                    memory: 19_456,
                    time: 1,
                    threads: 1,
                    verify: true,
                },
                ..openrust::hashing::HashingConfig::default()
            },
            4,
        )?,
    );
    let codes = VerificationCodes::new(
        openrust::auth::DatabaseVerificationCodeStore::new(db.clone()),
        passwords.clone(),
    );
    let flows = AuthFlows::new(
        db.clone(),
        passwords,
        JwtTokens::new(JwtConfig::new(
            "load-test-issuer",
            "load-test-audience",
            "load-test-key",
            std::collections::BTreeMap::from([(
                "load-test-key".to_owned(),
                STANDARD.encode([0x39_u8; 32]),
            )]),
        ))?,
        RefreshTokens::new(db.clone()),
        codes,
        RateLimiter::new(DatabaseRateLimitStore::new(db)),
    )
    .with_code_policy(VerificationCodePolicy::new(
        6,
        std::time::Duration::from_secs(600),
        std::time::Duration::from_secs(1),
        5,
    )?);
    let code = flows
        .register("load.user@example.test", "load-test-password", "127.0.0.1")
        .await?
        .ok_or("load test account was not registered")?;
    assert!(
        flows
            .verify_email("load.user@example.test", code.code(), "127.0.0.1")
            .await?
    );

    let flows = Arc::new(flows);
    let burst_started = std::time::Instant::now();
    let mut requests = tokio::task::JoinSet::new();
    for attempt in 1..=16 {
        let flows = Arc::clone(&flows);
        requests.spawn(async move {
            let started = std::time::Instant::now();
            let source = format!("127.0.0.{attempt}");
            let result = flows
                .login("load.user@example.test", "load-test-password", &source)
                .await;
            (started.elapsed(), result)
        });
    }
    let mut durations = Vec::with_capacity(16);
    let mut succeeded = 0;
    let mut capacity_rejected = 0;
    while let Some(result) = requests.join_next().await {
        let (duration, result) = result?;
        durations.push(duration);
        match result {
            Ok(_) => succeeded += 1,
            Err(error) if error.code() == "password_capacity_exhausted" => {
                capacity_rejected += 1;
            }
            Err(error) => return Err(error.into()),
        }
    }
    durations.sort_unstable();
    let percentile_ms = |percentile: usize| {
        let index = (durations.len() * percentile)
            .div_ceil(100)
            .saturating_sub(1);
        durations[index.min(durations.len() - 1)].as_secs_f64() * 1_000.0
    };
    assert!(succeeded > 0, "at least one login must complete");
    assert!(
        capacity_rejected > 0,
        "the bounded hash capacity must reject excess work"
    );
    let recovery_started = std::time::Instant::now();
    flows
        .login("load.user@example.test", "load-test-password", "127.0.1.1")
        .await?;
    let recovery_ms = recovery_started.elapsed().as_secs_f64() * 1_000.0;
    let elapsed = burst_started.elapsed();
    println!(
        "login burst backend={backend:?} pool_connections=2 requests=16 succeeded={succeeded} capacity_rejected={capacity_rejected} throughput_rps={:.1} p50_ms={:.1} p95_ms={:.1} p99_ms={:.1} recovery_ms={recovery_ms:.1}",
        16.0 / elapsed.as_secs_f64(),
        percentile_ms(50),
        percentile_ms(95),
        percentile_ms(99),
    );

    connection.close().await?;
    database.close().await?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // Keep the transactional listener and rollback assertions beside their real DB fixture.
async fn auth_flow_events_share_account_and_refresh_transactions() -> TestResult {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping auth flow event integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let backend = DatabaseBackend::from_url(&admin_url)?;
    if !backend.is_enabled() {
        eprintln!("skipping auth flow event integration: configured backend feature is disabled");
        return Ok(());
    }
    let names =
        openrust::database::DatabaseTableNames::new("flowev_", std::collections::BTreeMap::new())?;
    let database =
        TestDatabase::create_with_table_names::<AuthMigrator>(&admin_url, names.clone()).await?;
    let reject_registration = Arc::new(AtomicBool::new(false));
    let reject_login_success = Arc::new(AtomicBool::new(false));
    let failed_login_events = Arc::new(AtomicUsize::new(0));
    let mut events = EventsServiceProvider::new();
    let reject_registration_listener = Arc::clone(&reject_registration);
    events.listen::<openrust::auth::RegistrationValidating, _>(move |event, _| {
        let reject = Arc::clone(&reject_registration_listener);
        let email = event.email().to_owned();
        Box::pin(async move {
            if reject.load(Ordering::SeqCst) && email == "denied@example.test" {
                Err(openrust::Error::Forbidden(
                    "registration policy denied".into(),
                ))
            } else {
                Ok(())
            }
        })
    });
    let reject_success_listener = Arc::clone(&reject_login_success);
    events.listen::<openrust::auth::LoginSucceeded, _>(move |_, _| {
        let reject = Arc::clone(&reject_success_listener);
        Box::pin(async move {
            if reject.load(Ordering::SeqCst) {
                Err(openrust::Error::custom(
                    "login_result_rejected",
                    "test listener rejected login result",
                ))
            } else {
                Ok(())
            }
        })
    });
    let failed_listener = Arc::clone(&failed_login_events);
    events.listen::<openrust::auth::LoginFailed, _>(move |_, _| {
        let failed = Arc::clone(&failed_listener);
        Box::pin(async move {
            failed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    });

    let source = format!(
        "[database]\nurl = {:?}\ntable_prefix = \"flowev_\"\n",
        database.url()
    );
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(Figment::new().merge(figment::providers::Toml::string(&source)));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(DatabaseServiceProvider)
        .provider(events)
        .provider(AuthServiceProvider::new())
        .provider(JwtSettings::new(
            "event-test-issuer",
            "event-test-audience",
            "event-test-key",
            &std::collections::BTreeMap::from([(
                "event-test-key".to_owned(),
                STANDARD.encode([0x45_u8; 32]),
            )]),
        ))
        .provider(JwtServiceProvider)
        .provider(openrust::auth::AuthFlowsServiceProvider::new());
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let flows = app.state().resolve::<AuthFlows>().await?;
    let db = app.state().resolve::<Db>().await?;
    let connection = db.connection()?;

    reject_registration.store(true, Ordering::SeqCst);
    assert!(
        flows
            .register("denied@example.test", "password", "127.0.0.1")
            .await
            .is_err()
    );
    reject_registration.store(false, Ordering::SeqCst);
    let denied_count = connection
        .query_one_raw(Statement::from_string(
            connection.get_database_backend(),
            "SELECT COUNT(*) AS row_count FROM flowev_users WHERE email = 'denied@example.test'"
                .to_owned(),
        ))
        .await?
        .ok_or("registration event count query returned no row")?
        .try_get::<i64>("", "row_count")?;
    assert_eq!(
        denied_count, 0,
        "a rejected registration event must leave no account"
    );

    let code = flows
        .register("events@example.test", "event-password", "127.0.0.1")
        .await?
        .ok_or("account was not registered")?;
    assert!(
        flows
            .verify_email("events@example.test", code.code(), "127.0.0.1")
            .await?
    );
    assert!(
        flows
            .login("events@example.test", "wrong-password", "127.0.0.1")
            .await
            .is_err()
    );
    assert_eq!(
        failed_login_events.load(Ordering::SeqCst),
        1,
        "login failure events must be generic and observable"
    );

    reject_login_success.store(true, Ordering::SeqCst);
    assert!(
        flows
            .login("events@example.test", "event-password", "127.0.0.1")
            .await
            .is_err()
    );
    let token_count = connection
        .query_one_raw(Statement::from_string(
            connection.get_database_backend(),
            "SELECT COUNT(*) AS row_count FROM flowev_auth_refresh_tokens".to_owned(),
        ))
        .await?
        .ok_or("refresh token count query returned no row")?
        .try_get::<i64>("", "row_count")?;
    assert_eq!(
        token_count, 0,
        "a failed login result event must roll back refresh token storage"
    );

    drop(flows);
    drop(db);
    drop(connection);
    drop(app);
    database.close().await?;
    Ok(())
}

#[tokio::test]
// 此 fixture 同时验证路由级角色检查与 PAT scope 交集。
#[allow(clippy::too_many_lines)]
async fn policy_grants_and_revocations_are_visible_to_authenticated_routes() -> TestResult {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping Policy integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let backend = DatabaseBackend::from_url(&url)?;
    if !backend.is_enabled() {
        return Ok(());
    }
    let names = openrust::database::DatabaseTableNames::new(
        "policy_",
        std::collections::BTreeMap::from([("users".to_owned(), "mapped_accounts".to_owned())]),
    )?;
    let database = TestDatabase::create_with_table_names::<AuthMigrator>(&url, names).await?;
    let (app, router) = app_router(database.url(), "policy_").await?;
    let connection = app.state().resolve::<Db>().await?.connection()?;
    connection.execute_unprepared(
        "INSERT INTO mapped_accounts (email, password_hash, created_at, updated_at) VALUES ('policy@example.test', 'unused', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    ).await?;
    let user_id = connection
        .query_one_raw(Statement::from_string(
            connection.get_database_backend(),
            "SELECT id FROM mapped_accounts WHERE email = 'policy@example.test'".to_owned(),
        ))
        .await?
        .ok_or("policy account insert returned no row")?
        .try_get::<i64>("", "id")?;
    let policy = app.state().resolve::<Policy>().await?;
    let role = policy.ensure_role("content-editor").await?;
    policy.assign_role(user_id, role).await?;
    let tokens = app.state().resolve::<PersonalAccessTokens>().await?;
    let scoped_token = tokens
        .issue(user_id, "scoped", &["posts:read".to_owned()], None)
        .await?;
    let unscoped_token = tokens
        .issue(user_id, "unscoped", &["posts:write".to_owned()], None)
        .await?;
    let jwt = app.state().resolve::<JwtTokens>().await?;
    let token = jwt.issue_access_token(user_id)?;
    let request = || {
        Request::get("/auth/policy")
            .header("host", "localhost")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
    };
    let before = router.clone().oneshot(request()?).await?;
    assert_eq!(before.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(before.into_body(), usize::MAX).await?)?;
    assert_eq!(body["data"], false, "{body}");
    let denied = router
        .clone()
        .oneshot(
            Request::get("/auth/policy-authorize")
                .header("host", "localhost")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())?,
        )
        .await?;
    let denied_body: serde_json::Value =
        serde_json::from_slice(&to_bytes(denied.into_body(), usize::MAX).await?)?;
    assert_eq!(denied_body["success"], false);
    assert_eq!(denied_body["code"], "forbidden");

    policy.grant_permission(role, "posts:read").await?;
    let granted = router.clone().oneshot(request()?).await?;
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(granted.into_body(), usize::MAX).await?)?;
    assert_eq!(body["data"], true);

    let scoped = router
        .clone()
        .oneshot(
            Request::get("/auth/policy-pat")
                .header("host", "localhost")
                .header("authorization", format!("Bearer {}", scoped_token.expose()))
                .body(Body::empty())?,
        )
        .await?;
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(scoped.into_body(), usize::MAX).await?)?;
    assert_eq!(body["data"], true);
    let unscoped = router
        .clone()
        .oneshot(
            Request::get("/auth/policy-pat")
                .header("host", "localhost")
                .header(
                    "authorization",
                    format!("Bearer {}", unscoped_token.expose()),
                )
                .body(Body::empty())?,
        )
        .await?;
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(unscoped.into_body(), usize::MAX).await?)?;
    assert_eq!(body["data"], false);

    let second_role = policy.ensure_role("content-auditor").await?;
    policy.grant_permission(second_role, "posts:read").await?;
    policy.assign_role(user_id, second_role).await?;
    policy.revoke_role(user_id, role).await?;
    let inherited = router.clone().oneshot(request()?).await?;
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(inherited.into_body(), usize::MAX).await?)?;
    assert_eq!(body["data"], true);

    policy.revoke_permission(second_role, "posts:read").await?;
    let revoked = router.clone().oneshot(request()?).await?;
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(revoked.into_body(), usize::MAX).await?)?;
    assert_eq!(body["data"], false);
    drop(connection);
    drop(tokens);
    drop(policy);
    drop(router);
    drop(app);
    database.close().await?;
    Ok(())
}

type AuthMigrationNameCase = (DatabaseTableNames, [&'static str; 9]);

fn auth_migration_name_cases() -> Result<[AuthMigrationNameCase; 4], Box<dyn StdError>> {
    Ok([
        (
            openrust::database::DatabaseTableNames::default(),
            [
                "users",
                "sessions",
                "personal_access_tokens",
                "auth_refresh_tokens",
                "verification_codes",
                "roles",
                "permissions",
                "user_roles",
                "role_permissions",
            ],
        ),
        (
            openrust::database::DatabaseTableNames::new(
                "tb_",
                std::collections::BTreeMap::default(),
            )?,
            [
                "tb_users",
                "tb_sessions",
                "tb_personal_access_tokens",
                "tb_auth_refresh_tokens",
                "tb_verification_codes",
                "tb_roles",
                "tb_permissions",
                "tb_user_roles",
                "tb_role_permissions",
            ],
        ),
        (
            openrust::database::DatabaseTableNames::new("", std::collections::BTreeMap::default())?,
            [
                "users",
                "sessions",
                "personal_access_tokens",
                "auth_refresh_tokens",
                "verification_codes",
                "roles",
                "permissions",
                "user_roles",
                "role_permissions",
            ],
        ),
        (
            openrust::database::DatabaseTableNames::new(
                "custom_",
                std::collections::BTreeMap::from([("users".to_owned(), "accounts".to_owned())]),
            )?,
            [
                "accounts",
                "custom_sessions",
                "custom_personal_access_tokens",
                "custom_auth_refresh_tokens",
                "custom_verification_codes",
                "custom_roles",
                "custom_permissions",
                "custom_user_roles",
                "custom_role_permissions",
            ],
        ),
    ])
}

#[tokio::test]
async fn auth_migrations_use_default_empty_prefix_and_full_name_mapping() -> TestResult {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping table-name migration matrix: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let backend = DatabaseBackend::from_url(&url)?;
    if !backend.is_enabled() {
        return Ok(());
    }
    let cases = auth_migration_name_cases()?;
    for (table_names, expected_names) in cases {
        let database =
            TestDatabase::create_with_table_names::<AuthMigrator>(&url, table_names).await?;
        let outcome:TestResult=async {
        let connection = database.connection()?;
        for table_name in expected_names {
            let (sql, value) = match connection.get_database_backend() {
                openrust::database::DbBackend::Postgres => (
                    "SELECT COUNT(*) AS row_count FROM information_schema.tables WHERE table_schema = current_schema() AND table_name = $1",
                    table_name.into(),
                ),
                openrust::database::DbBackend::MySql => (
                    "SELECT COUNT(*) AS row_count FROM information_schema.tables WHERE table_schema = DATABASE() AND table_name = ?",
                    table_name.into(),
                ),
                _ => return Err("auth migration selected an unsupported backend".into()),
            };
            let row = connection
                .query_one_raw(Statement::from_sql_and_values(
                    connection.get_database_backend(),
                    sql,
                    [value],
                ))
                .await?
                .ok_or("table-name verification returned no row")?;
            if row.try_get::<i64>("", "row_count")? != 1 {
                return Err(format!("missing table {table_name}").into());
            }
        }
        drop(connection);
        Ok(())
        }.await;
        let cleanup = database.close().await;
        outcome?;
        cleanup?;
    }
    Ok(())
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn audit_and_permission_change_share_a_transaction_on_the_configured_database() -> TestResult
{
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping audit database integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let backend = DatabaseBackend::from_url(&url)?;
    if !backend.is_enabled() {
        return Ok(());
    }
    let names = openrust::database::DatabaseTableNames::new(
        "audit_it_",
        std::collections::BTreeMap::from([(
            "audit_records".to_owned(),
            "mapped_audit_log".to_owned(),
        )]),
    )?;
    let database =
        TestDatabase::create_with_table_names::<AuthMigrator>(&url, names.clone()).await?;
    let outcome:TestResult=async {
    let connection = database.connection()?;
    let backend = connection.get_database_backend();
    let db = Db::with_table_names(names);
    db.initialize(connection.clone())?;
    let policy = Policy::new(db.clone());
    let role_id = policy.ensure_role("audit-reviewer").await?;
    let sink = DatabaseAuditSink::new(db.clone());
    let audit = Audit::new(sink.clone(), AuditPolicy::default());
    let record = openrust::tenant::TenantContext::scope(
        openrust::tenant::TenantId::new("audit-tenant-a")?,
        async {
            audit.prepare(
                AuditEvent::new(
                    "permission.grant",
                    AuditSubject::new("role", role_id.to_string())?,
                    AuditOutcome::Succeeded,
                )?
                .actor(AuditSubject::new("user", "9001")?),
            )
        },
    )
    .await??;
    db.transaction(|transaction| {
        let policy = policy.clone();
        let sink = sink.clone();
        let record = record;
        Box::pin(async move {
            policy
                .grant_permission_in_transaction(transaction, role_id, "reports:read")
                .await?;
            sink.record_in_transaction(transaction, &record).await?;
            Ok(())
        })
    })
    .await?;

    let table = db.quoted_table_name("audit_records")?;
    let count_sql = format!("SELECT COUNT(*) AS count FROM {table}");
    let count = connection
        .query_one_raw(Statement::from_string(backend, count_sql.clone()))
        .await?
        .ok_or("audit count query returned no row")?
        .try_get::<i64>("", "count")?;
    require_auth_contract(count == 1,"audit row count differs")?;
    let stored = connection
        .query_one_raw(Statement::from_string(
            backend,
            format!(
                "SELECT action, actor_id, outcome, tenant_id FROM {table} WHERE action = 'permission.grant'"
            ),
        ))
        .await?
        .ok_or("audit record was not persisted")?;
    require_auth_contract(stored.try_get::<String>("", "action")? == "permission.grant","audit action differs")?;
    require_auth_contract(stored.try_get::<String>("", "actor_id")? == "9001","audit actor_id differs")?;
    require_auth_contract(stored.try_get::<String>("", "outcome")? == "succeeded","audit outcome differs")?;
    require_auth_contract(stored.try_get::<String>("", "tenant_id")? == "audit-tenant-a","audit tenant_id differs")?;

    let failed_record = audit.prepare(AuditEvent::new(
        "permission.grant",
        AuditSubject::new("role", role_id.to_string())?,
        AuditOutcome::Succeeded,
    )?)?;
    let rollback = db
        .transaction(|transaction| {
            let policy = policy.clone();
            let sink = sink.clone();
            Box::pin(async move {
                policy
                    .grant_permission_in_transaction(transaction, role_id, "reports:write")
                    .await?;
                sink.record_in_transaction(transaction, &failed_record)
                    .await?;
                Err::<(), _>(openrust::Error::BadRequest("exercise rollback".into()))
            })
        })
        .await;
    require_auth_contract(rollback.is_err(),"expected rollback failure")?;
    let count = connection
        .query_one_raw(Statement::from_string(backend, count_sql.clone()))
        .await?
        .ok_or("audit count query returned no row")?
        .try_get::<i64>("", "count")?;
    require_auth_contract(count == 1,"rolled back audit record must not remain")?;
    let permission =
        "SELECT COUNT(*) AS count FROM audit_it_permissions WHERE name = 'reports:write'";
    let count = connection
        .query_one_raw(Statement::from_string(backend, permission.to_owned()))
        .await?
        .ok_or("permission count query returned no row")?
        .try_get::<i64>("", "count")?;
    require_auth_contract(count == 0,"permission mutation must roll back with audit insert")?;

    let purge_event = AuditEvent::new(
        "account.updated",
        AuditSubject::new("account", "77")?,
        AuditOutcome::Succeeded,
    )?;
    audit.record(purge_event).await?;
    let purge_event = AuditEvent::new(
        "account.updated",
        AuditSubject::new("account", "78")?,
        AuditOutcome::Succeeded,
    )?;
    audit.record(purge_event).await?;
    let cutoff = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
    require_auth_contract(sink.purge_before(cutoff, 1).await? == 1,"audit purge batch differs")?;
    let count = connection
        .query_one_raw(Statement::from_string(backend, count_sql.clone()))
        .await?
        .ok_or("audit count query returned no row")?
        .try_get::<i64>("", "count")?;
    require_auth_contract(count == 2,"one purge call must respect its batch bound")?;
    require_auth_contract(sink.purge_before(cutoff, 2).await? == 2,"audit purge batch differs")?;
    require_auth_contract(sink.purge_before(cutoff, 2).await? == 0,"audit purge batch differs")?;

    connection
        .execute_unprepared(&format!("DROP TABLE {table}"))
        .await?;
    let audit_failure_record = audit.prepare(AuditEvent::new(
        "permission.grant",
        AuditSubject::new("role", role_id.to_string())?,
        AuditOutcome::Succeeded,
    )?)?;
    let audit_write_failure = db
        .transaction(|transaction| {
            let policy = policy.clone();
            let sink = sink.clone();
            Box::pin(async move {
                policy
                    .grant_permission_in_transaction(transaction, role_id, "reports:audit-failure")
                    .await?;
                sink.record_in_transaction(transaction, &audit_failure_record)
                    .await
            })
        })
        .await;
    require_auth_contract(audit_write_failure.is_err(),"expected audit write failure")?;
    let count = connection
        .query_one_raw(Statement::from_string(
            backend,
            "SELECT COUNT(*) AS count FROM audit_it_permissions WHERE name = 'reports:audit-failure'".to_owned(),
        ))
        .await?
        .ok_or("permission count query returned no row")?
        .try_get::<i64>("", "count")?;
    require_auth_contract(count == 0,"audit insert failure must roll back permission changes")?;
    Ok(())
    }.await;
    let cleanup = database.close().await;
    outcome?;
    cleanup?;
    Ok(())
}

struct TestInstance<T: Send + Sync + 'static>(std::sync::Arc<T>);
impl<T: Send + Sync + 'static> openrust::core::ServiceProvider for TestInstance<T> {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::core::RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.instance(std::sync::Arc::clone(&self.0)).await?;
            Ok(())
        })
    }
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
            app.exceptions(|handler| {
                handler.api_envelope_when(|_, _| true);
            })?;
            app.routes(move |value| {
                value.group(
                    openrust::http::RouteGroup::new().middleware_alias("session"),
                    |value| routes(value),
                )
            })?;
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
