//! 验证共享数据库上的外部身份流程、浏览器绑定与账户冲突边界。
#![cfg(all(
    feature = "auth-oidc",
    any(feature = "db-postgres", feature = "db-mysql")
))]

use std::{
    collections::{BTreeMap, HashMap},
    error::Error as StdError,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
#[cfg(feature = "auth-fortify")]
use hmac::{Hmac, Mac, digest::KeyInit};
use openrust::config::FrameworkConfig;
use openrust::session::SessionStore as _;
use openrust::testing::TestDatabase;
use openrust::{
    Error, Result,
    audit::{DatabaseAuditMigration, DatabaseAuditTenantMigration},
    auth::{
        AuthExternalIdentityMigration, AuthExternalSessionMigration, AuthFlows,
        AuthFlowsServiceProvider, AuthMigration, AuthRefreshTokenMigration,
        AuthVerificationCodeMigration, BackchannelLogoutOutcome, DatabaseVerificationCodeStore,
        ExternalIdentityFlows, ExternalIdentityProvider, JwtConfig, JwtTokens, OidcAuthorization,
        RefreshTokens, Session, SessionStoreHandle, VerifiedExternalIdentity, VerifiedLogoutToken,
    },
    config::EncryptionConfig,
    core::RateLimiter,
    database::{
        DatabaseBackend, DatabaseRateLimitMigration, DatabaseRateLimitStore, DatabaseTableNames,
        Db,
        sea_orm::{ConnectionTrait, Statement},
        sea_orm_migration::{MigratorTrait, prelude::*},
    },
    security::Encryptor,
};
#[cfg(feature = "auth-fortify")]
use sha1::Sha1;
use sha2::Digest as _;

type TestResult<T = ()> = std::result::Result<T, Box<dyn StdError>>;

#[derive(Clone)]
struct FailDeleteOnceStore {
    inner: Arc<dyn openrust::session::SessionStore>,
    fail_next_delete: Arc<AtomicBool>,
}

impl std::fmt::Debug for FailDeleteOnceStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("FailDeleteOnceStore")
    }
}

impl openrust::session::SessionStore for FailDeleteOnceStore {
    fn write<'a>(
        &'a self,
        record: &'a openrust::session::SessionRecord,
        mode: openrust::session::SessionWriteMode,
        max_bytes: usize,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<openrust::session::SessionWriteOutcome>>
    {
        self.inner.write(record, mode, max_bytes)
    }
    fn load<'a>(
        &'a self,
        id: &'a openrust::session::SessionId,
        max_bytes: usize,
        empty_expiry: time::OffsetDateTime,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<Option<openrust::session::SessionRecord>>>
    {
        self.inner.load(id, max_bytes, empty_expiry)
    }
    fn delete<'a>(
        &'a self,
        id: &'a openrust::session::SessionId,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            if self.fail_next_delete.swap(false, Ordering::SeqCst) {
                return Err(openrust::Error::custom_with_status(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    "session_store_unavailable",
                    "injected one-time Session delete failure",
                ));
            }
            self.inner.delete(id).await
        })
    }
}

struct OidcMigrator;

impl MigratorTrait for OidcMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(AuthMigration),
            Box::new(AuthRefreshTokenMigration),
            Box::new(AuthVerificationCodeMigration),
            Box::new(DatabaseRateLimitMigration),
            Box::new(DatabaseAuditMigration),
            Box::new(DatabaseAuditTenantMigration),
            Box::new(AuthExternalIdentityMigration),
            Box::new(AuthExternalSessionMigration),
            #[cfg(feature = "auth-fortify")]
            Box::new(openrust::auth::fortify::FortifyTwoFactorMigration::new()),
            #[cfg(feature = "auth-fortify")]
            Box::new(openrust::auth::fortify::FactorAuthorityMigration::new()),
            #[cfg(feature = "auth-fortify")]
            Box::new(openrust::auth::fortify::CredentialFactorMigration::new()),
            #[cfg(feature = "auth-fortify")]
            Box::new(openrust::auth::fortify::CredentialChallengeMigration::new()),
        ]
    }
}

struct TestProvider {
    next_authorization: AtomicUsize,
    identities: RwLock<HashMap<String, VerifiedExternalIdentity>>,
    authentication_calls: AtomicUsize,
    logout_available: AtomicBool,
}

impl TestProvider {
    fn new() -> Self {
        Self {
            next_authorization: AtomicUsize::new(0),
            identities: RwLock::new(HashMap::new()),
            authentication_calls: AtomicUsize::new(0),
            logout_available: AtomicBool::new(true),
        }
    }

    fn set_identity(&self, code: &str, identity: VerifiedExternalIdentity) -> TestResult {
        self.identities
            .write()
            .map_err(|_| "fake provider lock was poisoned")?
            .insert(code.to_owned(), identity);
        Ok(())
    }
}

impl ExternalIdentityProvider for TestProvider {
    fn name(&self) -> &'static str {
        "test-idp"
    }

    fn issuer(&self) -> &'static str {
        "https://idp.example.test"
    }

    fn end_session_endpoint(&self) -> Option<url::Url> {
        self.logout_available
            .load(Ordering::SeqCst)
            .then(|| url::Url::parse("https://idp.example.test/logout").ok())
            .flatten()
    }

    fn post_logout_redirect_uri(&self) -> Option<String> {
        Some("https://idp.example.test/callback".to_owned())
    }

    fn client_id(&self) -> Option<&str> {
        Some("openrust-test-client")
    }

    fn verify_logout_token<'a>(
        &'a self,
        token: &'a str,
    ) -> openrust::core::BoxFuture<'a, Result<VerifiedLogoutToken>> {
        Box::pin(async move {
            match token {
                "test-provider-verified-token" => VerifiedLogoutToken::after_provider_validation(
                    self.issuer(),
                    "openrust-test-client",
                    "test-jti-1",
                    1,
                    Some("idp-sid-2".to_owned()),
                    Some("sub-1".to_owned()),
                ),
                "test-provider-subject-token" => VerifiedLogoutToken::after_provider_validation(
                    self.issuer(),
                    "openrust-test-client",
                    "test-jti-subject-1",
                    1,
                    None,
                    Some("sub-1".to_owned()),
                ),
                _ => Err(Error::Unauthorized),
            }
        })
    }

    fn authorize(&self) -> Result<OidcAuthorization> {
        let index = self.next_authorization.fetch_add(1, Ordering::SeqCst);
        let state = format!("state-{index}");
        let mut url =
            url::Url::parse("https://idp.example.test/authorize").map_err(Error::internal)?;
        url.query_pairs_mut().append_pair("state", &state);
        OidcAuthorization::new(
            url,
            state,
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
            self.authentication_calls.fetch_add(1, Ordering::SeqCst);
            self.identities
                .read()
                .map_err(|_| {
                    Error::internal(std::io::Error::other("fake provider lock was poisoned"))
                })?
                .get(code)
                .cloned()
                .ok_or(Error::Unauthorized)
        })
    }
}

fn identity(subject: &str, email: &str, email_verified: bool) -> Result<VerifiedExternalIdentity> {
    VerifiedExternalIdentity::new(
        "https://idp.example.test",
        subject,
        Some(email.to_owned()),
        email_verified,
    )
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    clippy::infallible_destructuring_match,
    reason = "同一真实数据库用例覆盖外部身份、关联撤销与可选Fortify挑战"
)]
async fn external_identity_flows_enforce_browser_binding_single_use_and_explicit_linking()
-> TestResult {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping OIDC database integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let backend = DatabaseBackend::from_url(&url)?;
    if !backend.is_enabled() {
        eprintln!("skipping OIDC database integration: configured backend feature is disabled");
        return Ok(());
    }

    let names = DatabaseTableNames::new(
        "oidcit_",
        BTreeMap::from([("users".to_owned(), "oidc_accounts".to_owned())]),
    )?;
    let database =
        TestDatabase::create_with_table_names::<OidcMigrator>(&url, names.clone()).await?;
    let outcome:TestResult=Box::pin(async {
    let connection = database.connection()?;
    let db = Db::with_table_names(names);
    db.initialize(connection.clone())?;

    let passwords = std::sync::Arc::new(openrust::hashing::HashManager::new(openrust::hashing::HashingConfig { driver: "argon2id".into(), argon: openrust::hashing::ArgonConfig { memory: 19_456, time: 1, threads: 1, verify: true }, ..openrust::hashing::HashingConfig::default() })?);
    let jwt = JwtTokens::new(JwtConfig::new(
        "oidc-test-issuer",
        "oidc-test-audience",
        "oidc-test-key",
        BTreeMap::from([("oidc-test-key".to_owned(), STANDARD.encode([0x51_u8; 32]))]),
    ))?;
    let codes = openrust::auth::VerificationCodes::new(
        DatabaseVerificationCodeStore::new(db.clone()),
        passwords.clone(),
    );
    let refresh = RefreshTokens::new(db.clone());
    let encryptor = Encryptor::new(EncryptionConfig::new(
        "test-key",
        BTreeMap::from([("test-key".to_owned(), STANDARD.encode([0xa3_u8; 32]))]),
    ))?;
    let auth = {
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(
            FrameworkConfig::default(),
        ))
        .merge(figment::providers::Serialized::defaults(serde_json::json!({"hashing":{"driver":"argon2id","argon":{"memory":19456,"time":1,"threads":1,"verify":true}}})));
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(TestInstance(std::sync::Arc::new(db.clone())))
            .provider(TestInstance(passwords))
            .provider(TestInstance(std::sync::Arc::new(jwt.clone())))
            .provider(TestInstance(std::sync::Arc::new(refresh.clone())))
            .provider(TestInstance(std::sync::Arc::new(codes)))
            .provider(TestInstance(std::sync::Arc::new(RateLimiter::new(
                DatabaseRateLimitStore::new(db.clone()),
            ))))
            .provider(TestInstance(std::sync::Arc::new(encryptor.clone())));
        builder.provider(AuthFlowsServiceProvider::new());
        let app = builder
            .start(sources)
            .await
            .and_then(openrust::App::from_provider_runtime)?;
        (*app.state().resolve::<AuthFlows>().await?).clone()
    };
    let provider = Arc::new(TestProvider::new());
    let provider_trait: Arc<dyn ExternalIdentityProvider> = provider.clone();
    let external =
        ExternalIdentityFlows::new(db.clone(), auth.clone(), encryptor, [provider_trait])?;

    // 错误的浏览器绑定不得触发 provider token exchange。
    provider.set_identity(
        "verified-login",
        identity("sub-1", "first@example.test", true)?,
    )?;
    let login = external.authorize("test-idp", "127.0.0.1").await?;
    let state = state_from_url(login.url())?;
    assert_eq!(
        openrust::auth::ExternalIdentityAuthorization::cookie_name_for_callback(&state)?,
        login.browser_binding_cookie_name()
    );
    let set_cookie = login.set_cookie_header();
    assert!(set_cookie.contains("; HttpOnly; Secure; SameSite=Lax"));
    assert!(
        openrust::auth::ExternalIdentityAuthorization::clear_cookie_header(&state)?
            .contains("Max-Age=0")
    );
    assert!(
        external
            .complete(
                "test-idp",
                &state,
                "verified-login",
                "wrong-browser-binding",
                "127.0.0.2",
            )
            .await
            .is_err()
    );
    assert_eq!(provider.authentication_calls.load(Ordering::SeqCst), 0);

    let result = external
        .complete_with_handle(
            "test-idp",
            &state,
            "verified-login",
            login.browser_binding_cookie(),
            "127.0.0.3",
        )
        .await?;
    let (attempt, association_handle) = result.into_parts();
    let credentials = match attempt {
        openrust::auth::LoginAttempt::Authenticated(credentials) => credentials,
        #[cfg(feature = "auth-fortify")]
        openrust::auth::LoginAttempt::TwoFactorRequired(_) => {
            return Err("expected completed external login".into());
        }
    };
    let user_id = credentials.user_id();
    let refresh_token = credentials.refresh_token().expose().to_owned();
    assert!(user_id > 0);
    assert!(format!("{association_handle:?}").contains("[REDACTED]"));
    assert_eq!(
        count_rows(&connection, &db, "auth_external_sessions").await?,
        1
    );
    let association_table = db.quoted_table_name("auth_external_sessions")?;
    let association = connection
        .query_one_raw(Statement::from_string(
            connection.get_database_backend(),
            format!("SELECT refresh_family_id, subject_hash FROM {association_table}"),
        ))
        .await?
        .ok_or("external login association was not persisted")?;
    let family: Option<String> = association.try_get("", "refresh_family_id")?;
    assert!(family.is_some_and(|value| !value.is_empty()));
    assert_eq!(
        association.try_get::<String>("", "subject_hash")?,
        format!("{:x}", sha2::Sha256::digest("sub-1"))
    );
    let memory_store = openrust::session::MemorySessionStore::default();
    let session_store = SessionStoreHandle::new(memory_store.clone());
    let session = Session::new(None, Arc::new(memory_store.clone()), None)?;
    session.start().await?;
    session.put_typed("openrust.auth.user_id", user_id)?;
    session.save().await?;
    let first_session_id = session.id()?;
    external
        .attach_session(user_id, &association_handle, &session)
        .await?;
    assert!(
        memory_store
            .load(
                &first_session_id,
                openrust::session::DEFAULT_MAX_BYTES,
                time::OffsetDateTime::now_utc() + time::Duration::minutes(120)
            )
            .await?
            .is_some()
    );
    assert!(
        external
            .begin_logout(
                user_id,
                &association_handle,
                "https://attacker.example.test/after-logout",
                &session_store,
            )
            .await
            .is_err(),
        "an unregistered post-logout redirect must be rejected before local revocation"
    );
    assert!(
        refresh.rotate(&refresh_token).await.is_ok(),
        "a rejected redirect must not partially revoke local credentials"
    );
    let logout = external
        .begin_logout(
            user_id,
            &association_handle,
            "https://idp.example.test/callback",
            &session_store,
        )
        .await?
        .ok_or("expected configured IdP logout endpoint")?;
    assert_eq!(logout.url().path(), "/logout");
    assert!(
        logout
            .url()
            .query_pairs()
            .any(|(key, value)| key == "client_id" && value == "openrust-test-client")
    );
    assert!(refresh.rotate(&refresh_token).await.is_err());
    assert!(
        memory_store
            .load(
                &first_session_id,
                openrust::session::DEFAULT_MAX_BYTES,
                time::OffsetDateTime::now_utc() + time::Duration::minutes(120)
            )
            .await?
            .is_none()
    );
    assert_eq!(
        external
            .complete_logout("test-idp", logout.state())
            .await?
            .as_str(),
        "https://idp.example.test/callback"
    );
    assert!(
        external
            .complete_logout("test-idp", logout.state())
            .await
            .is_err()
    );
    assert!(
        memory_store
            .load(
                &first_session_id,
                openrust::session::DEFAULT_MAX_BYTES,
                time::OffsetDateTime::now_utc() + time::Duration::minutes(120)
            )
            .await?
            .is_none()
    );

    provider.set_identity(
        "second-login",
        identity("sub-1", "first@example.test", true)?.with_session_id("idp-sid-2")?,
    )?;
    let second_login = external.authorize("test-idp", "127.0.0.12").await?;
    let second_state = state_from_url(second_login.url())?;
    let second = external
        .complete_with_handle(
            "test-idp",
            &second_state,
            "second-login",
            second_login.browser_binding_cookie(),
            "127.0.0.13",
        )
        .await?;
    assert_eq!(
        count_rows(&connection, &db, "auth_external_logout_scopes").await?,
        0,
        "successful callbacks must release non-revocation scope locks"
    );
    let (second_attempt, second_handle) = second.into_parts();
    let second_credentials = match second_attempt {
        openrust::auth::LoginAttempt::Authenticated(credentials) => credentials,
        #[cfg(feature = "auth-fortify")]
        openrust::auth::LoginAttempt::TwoFactorRequired(_) => {
            return Err("expected completed second external login".into());
        }
    };
    let second_refresh = second_credentials.refresh_token().expose().to_owned();
    let second_session = Session::new(None, Arc::new(memory_store.clone()), None)?;
    second_session.start().await?;
    second_session.put_typed("openrust.auth.user_id", user_id)?;
    second_session.save().await?;
    let second_session_id = second_session.id()?;
    external
        .attach_session(user_id, &second_handle, &second_session)
        .await?;
    #[cfg(feature = "auth-fortify")]
    {
        // 这里只验证按owner撤销，即使context损坏也不得留下外部关联的待挑战行；不据此授权。
        let challenge = "pending-external-factor-challenge";
        let challenge_hash = format!("{:x}", sha2::Sha256::digest(challenge));
        let association_hash = format!("{:x}", sha2::Sha256::digest(second_handle.expose()));
        let backend = connection.get_database_backend();
        let challenge_table = db.quoted_table_name("auth_factor_challenges")?;
        let association_table = db.quoted_table_name("auth_external_sessions")?;
        let (insert, values, update, update_values) = match backend {
            openrust::database::sea_orm::DbBackend::Postgres => (
                format!(
                    "INSERT INTO {challenge_table} (token_hash, user_id, source_hash, context, purpose, attempts, expires_at, created_at) VALUES ($1, $2, $3, '{{}}', 'login', 0, CURRENT_TIMESTAMP + INTERVAL '5 minutes', CURRENT_TIMESTAMP)"
                ),
                vec![
                    challenge_hash.clone().into(),
                    user_id.into(),
                    format!("{:x}", sha2::Sha256::digest("127.0.0.20")).into(),
                ],
                format!("UPDATE {association_table} SET challenge_hash = $1 WHERE id_hash = $2"),
                vec![challenge_hash.clone().into(), association_hash.into()],
            ),
            openrust::database::sea_orm::DbBackend::MySql => (
                format!(
                    "INSERT INTO {challenge_table} (token_hash, user_id, source_hash, context, purpose, attempts, expires_at, created_at) VALUES (?, ?, ?, '{{}}', 'login', 0, CURRENT_TIMESTAMP(6) + INTERVAL 5 MINUTE, CURRENT_TIMESTAMP(6))"
                ),
                vec![
                    challenge_hash.clone().into(),
                    user_id.into(),
                    format!("{:x}", sha2::Sha256::digest("127.0.0.20")).into(),
                ],
                format!("UPDATE {association_table} SET challenge_hash = ? WHERE id_hash = ?"),
                vec![challenge_hash.clone().into(), association_hash.into()],
            ),
            _ => return Err("unsupported test database".into()),
        };
        connection
            .execute_raw(Statement::from_sql_and_values(backend, &insert, values))
            .await?;
        connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &update,
                update_values,
            ))
            .await?;
    }
    #[cfg(feature = "auth-fortify")]
    let cleanup_external = {
        let encryption = Encryptor::new(EncryptionConfig::new("test-key", BTreeMap::from([("test-key".into(),STANDARD.encode([0xa3_u8;32]))])))?;
        let parts = web::Parts::new(db.clone(),std::sync::Arc::new(openrust::hashing::HashManager::new(openrust::hashing::HashingConfig { driver: "argon2id".into(), argon: openrust::hashing::ArgonConfig { memory: 19_456, time: 1, threads: 1, verify: true }, ..openrust::hashing::HashingConfig::default() })?),encryption.clone(),auth.clone()).await?;
        ExternalIdentityFlows::new(db.clone(),parts.auth,encryption,[provider.clone() as Arc<dyn ExternalIdentityProvider>])?
    };
    #[cfg(not(feature = "auth-fortify"))]
    let cleanup_external = external.clone();
    assert_eq!(
        cleanup_external
            .process_backchannel_logout("test-idp", "test-provider-verified-token", &session_store,)
            .await?,
        BackchannelLogoutOutcome::Processed(1)
    );
    assert!(refresh.rotate(&second_refresh).await.is_err());
    assert!(
        memory_store
            .load(
                &second_session_id,
                openrust::session::DEFAULT_MAX_BYTES,
                time::OffsetDateTime::now_utc() + time::Duration::minutes(120)
            )
            .await?
            .is_none()
    );
    #[cfg(feature = "auth-fortify")]
    assert_eq!(
        count_rows(&connection, &db, "auth_factor_challenges").await?,
        0
    );
    assert_eq!(
        external
            .process_backchannel_logout("test-idp", "test-provider-verified-token", &session_store,)
            .await?,
        BackchannelLogoutOutcome::AlreadyProcessed
    );
    provider.set_identity(
        "callback-after-logout",
        identity("sub-1", "first@example.test", true)?.with_session_id("idp-sid-2")?,
    )?;
    let stale_callback = external.authorize("test-idp", "127.0.0.14").await?;
    let stale_state = state_from_url(stale_callback.url())?;
    assert!(
        external
            .complete_with_handle(
                "test-idp",
                &stale_state,
                "callback-after-logout",
                stale_callback.browser_binding_cookie(),
                "127.0.0.15",
            )
            .await
            .is_err()
    );
    assert!(
        external
            .complete(
                "test-idp",
                &state,
                "verified-login",
                login.browser_binding_cookie(),
                "127.0.0.4",
            )
            .await
            .is_err()
    );
    provider.set_identity(
        "subject-login",
        identity("sub-1", "first@example.test", true)?,
    )?;
    let subject_login = external.authorize("test-idp", "127.0.0.16").await?;
    let subject_state = state_from_url(subject_login.url())?;
    let subject_result = external
        .complete_with_handle(
            "test-idp",
            &subject_state,
            "subject-login",
            subject_login.browser_binding_cookie(),
            "127.0.0.17",
        )
        .await?;
    let (subject_attempt, _) = subject_result.into_parts();
    let subject_refresh = match subject_attempt {
        openrust::auth::LoginAttempt::Authenticated(credentials) => {
            credentials.refresh_token().expose().to_owned()
        }
        #[cfg(feature = "auth-fortify")]
        openrust::auth::LoginAttempt::TwoFactorRequired(_) => {
            return Err("expected completed subject-scoped login".into());
        }
    };
    assert_eq!(
        external
            .process_backchannel_logout("test-idp", "test-provider-subject-token", &session_store,)
            .await?,
        BackchannelLogoutOutcome::Processed(1)
    );
    assert!(refresh.rotate(&subject_refresh).await.is_err());
    provider.set_identity(
        "subject-callback-after-logout",
        identity("sub-1", "first@example.test", true)?,
    )?;
    let subject_retry = external.authorize("test-idp", "127.0.0.18").await?;
    let subject_retry_state = state_from_url(subject_retry.url())?;
    assert!(
        external
            .complete_with_handle(
                "test-idp",
                &subject_retry_state,
                "subject-callback-after-logout",
                subject_retry.browser_binding_cookie(),
                "127.0.0.19",
            )
            .await
            .is_err()
    );

    provider.set_identity(
        "rp-retry-login",
        identity("sub-rp-retry", "retry@example.test", true)?,
    )?;
    let retry_login = external.authorize("test-idp", "127.0.0.20").await?;
    let retry_state = state_from_url(retry_login.url())?;
    let retry_result = external
        .complete_with_handle(
            "test-idp",
            &retry_state,
            "rp-retry-login",
            retry_login.browser_binding_cookie(),
            "127.0.0.21",
        )
        .await
        .map_err(|error| format!("RP cleanup retry login failed: {error:?}"))?;
    let (retry_attempt, retry_handle) = retry_result.into_parts();
    let retry_user_id = match retry_attempt {
        openrust::auth::LoginAttempt::Authenticated(credentials) => credentials.user_id(),
        #[cfg(feature = "auth-fortify")]
        openrust::auth::LoginAttempt::TwoFactorRequired(_) => {
            return Err("expected direct credentials for logout retry test".into());
        }
    };
    #[cfg(feature = "session-redis")]
    let retry_store: Arc<dyn openrust::session::SessionStore> =
        if let Ok(redis_url) = std::env::var("OPENRUST_TEST_SESSION_REDIS_URL") {
            Arc::new(
                openrust::auth::RedisSessionStore::connect(
                    redis_url,
                    format!("oidc-retry-{}", std::process::id()),
                )
                .await?,
            )
        } else if std::env::var("OPENRUST_REQUIRE_SHARED_SESSION").as_deref() == Ok("1") {
            return Err(
                "shared-session retry acceptance requires OPENRUST_TEST_SESSION_REDIS_URL".into(),
            );
        } else {
            Arc::new(openrust::session::MemorySessionStore::default())
        };
    #[cfg(not(feature = "session-redis"))]
    let retry_store: Arc<dyn openrust::session::SessionStore> =
        Arc::new(openrust::session::MemorySessionStore::default());
    let retry_store_wrapper = FailDeleteOnceStore {
        inner: Arc::clone(&retry_store),
        fail_next_delete: Arc::new(AtomicBool::new(true)),
    };
    let retry_session_store = SessionStoreHandle::new(retry_store_wrapper.clone());
    let retry_session = Session::new(None, Arc::new(retry_store_wrapper), None)?;
    retry_session.start().await?;
    retry_session.put_typed("openrust.auth.user_id", retry_user_id)?;
    retry_session.save().await?;
    let retry_session_id = retry_session.id()?;
    assert!(
        retry_store
            .load(
                &retry_session_id,
                openrust::session::DEFAULT_MAX_BYTES,
                time::OffsetDateTime::now_utc() + time::Duration::minutes(120)
            )
            .await?
            .is_some()
    );
    external
        .attach_session(retry_user_id, &retry_handle, &retry_session)
        .await
        .map_err(|error| format!("RP cleanup retry Session attach failed: {error:?}"))?;
    assert!(
        external
            .begin_logout(
                retry_user_id,
                &retry_handle,
                "https://idp.example.test/callback",
                &retry_session_store,
            )
            .await
            .is_err()
    );
    assert!(
        retry_store
            .load(
                &retry_session_id,
                openrust::session::DEFAULT_MAX_BYTES,
                time::OffsetDateTime::now_utc() + time::Duration::minutes(120)
            )
            .await?
            .is_some()
    );
    let retried_logout = external
        .begin_logout(
            retry_user_id,
            &retry_handle,
            "https://idp.example.test/callback",
            &retry_session_store,
        )
        .await
        .map_err(|error| format!("RP cleanup retry logout failed: {error:?}"))?;
    assert!(retried_logout.is_none());
    assert!(
        retry_store
            .load(
                &retry_session_id,
                openrust::session::DEFAULT_MAX_BYTES,
                time::OffsetDateTime::now_utc() + time::Duration::minutes(120)
            )
            .await?
            .is_none()
    );
    assert_eq!(provider.authentication_calls.load(Ordering::SeqCst), 6);
    assert_eq!(
        count_rows(&connection, &db, "external_identities").await?,
        2
    );

    // 未验证邮箱不建立新本地账户；错误邮箱冲突不会自动合并。
    provider.set_identity(
        "unverified-login",
        identity("sub-unverified", "unverified@example.test", false)?,
    )?;
    let unverified = external.authorize("test-idp", "127.0.0.5").await?;
    let unverified_state = state_from_url(unverified.url())?;
    assert!(
        external
            .complete(
                "test-idp",
                &unverified_state,
                "unverified-login",
                unverified.browser_binding_cookie(),
                "127.0.0.6",
            )
            .await
            .is_err()
    );
    assert_eq!(
        count_rows(&connection, &db, "external_identities").await?,
        2
    );

    provider.set_identity(
        "email-collision",
        identity("sub-collision", "first@example.test", true)?,
    )?;
    let collision = external.authorize("test-idp", "127.0.0.7").await?;
    let collision_state = state_from_url(collision.url())?;
    assert!(
        external
            .complete(
                "test-idp",
                &collision_state,
                "email-collision",
                collision.browser_binding_cookie(),
                "127.0.0.8",
            )
            .await
            .is_err()
    );
    assert_eq!(
        count_rows(&connection, &db, "external_identities").await?,
        2
    );

    // 本地账户经密码重验后才能绑定；绑定身份唯一，解绑也需重新认证。
    let registration = auth
        .register("second@example.test", "local-password", "127.0.0.9")
        .await?
        .ok_or("expected local test account registration")?;
    assert!(
        auth.verify_email("second@example.test", registration.code(), "127.0.0.9",)
            .await?
    );
    let local_user_id = lookup_user_id(&connection, &db, "second@example.test").await?;
    provider.set_identity(
        "bind-identity",
        identity("sub-bind", "different@example.test", true)?,
    )?;
    let bind = external.authorize("test-idp", "127.0.0.10").await?;
    let bind_state = state_from_url(bind.url())?.clone();
    assert!(
        external
            .bind(
                local_user_id,
                "second@example.test",
                "wrong-password",
                "127.0.0.11",
                "test-idp",
                &bind_state,
                "bind-identity",
                bind.browser_binding_cookie(),
            )
            .await
            .is_err()
    );
    external
        .bind(
            local_user_id,
            "second@example.test",
            "local-password",
            "127.0.0.12",
            "test-idp",
            &bind_state,
            "bind-identity",
            bind.browser_binding_cookie(),
        )
        .await?;
    let links = external.linked_identities(local_user_id).await?;
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].provider(), "test-idp");
    assert_eq!(links[0].issuer(), "https://idp.example.test");
    assert_eq!(links[0].subject(), "sub-bind");
    assert!(
        external
            .bind(
                local_user_id,
                "second@example.test",
                "local-password",
                "127.0.0.13",
                "test-idp",
                &bind_state,
                "bind-identity",
                bind.browser_binding_cookie(),
            )
            .await
            .is_err()
    );
    assert!(
        external
            .unbind(
                local_user_id,
                "second@example.test",
                "wrong-password",
                "127.0.0.14",
                "test-idp",
                "https://idp.example.test",
                "sub-bind",
            )
            .await
            .is_err()
    );
    assert!(
        external
            .unbind(
                local_user_id,
                "second@example.test",
                "local-password",
                "127.0.0.15",
                "test-idp",
                "https://idp.example.test",
                "sub-bind",
            )
            .await?
    );
    assert!(
        !external
            .unbind(
                local_user_id,
                "second@example.test",
                "local-password",
                "127.0.0.16",
                "test-idp",
                "https://idp.example.test",
                "sub-bind",
            )
            .await?
    );

    // 清理仅删除已过期的登录事务，且有明确的批次上限。
    let expired = external.authorize("test-idp", "127.0.0.17").await?;
    let expired_state = state_from_url(expired.url())?;
    expire_request(&connection, &db, &unverified_state).await?;
    expire_request(&connection, &db, &collision_state).await?;
    expire_request(&connection, &db, &expired_state).await?;
    assert_eq!(external.purge_expired(10).await?, 3);
    assert!(external.purge_expired(0).await.is_err());
    assert_eq!(
        count_rows(&connection, &db, "external_identity_requests").await?,
        0
    );

    provider.set_identity(
        "local-only-logout",
        identity("sub-local-only", "local-only@example.test", true)?,
    )?;
    let local_only_authorization = external.authorize("test-idp", "127.0.0.22").await?;
    let local_only_state = state_from_url(local_only_authorization.url())?;
    let local_only_result = external
        .complete_with_handle(
            "test-idp",
            &local_only_state,
            "local-only-logout",
            local_only_authorization.browser_binding_cookie(),
            "127.0.0.22",
        )
        .await?;
    let (local_only_attempt, local_only_handle) = local_only_result.into_parts();
    let local_only_credentials = match local_only_attempt {
        openrust::auth::LoginAttempt::Authenticated(credentials) => credentials,
        #[cfg(feature = "auth-fortify")]
        openrust::auth::LoginAttempt::TwoFactorRequired(_) => {
            return Err("expected local-only logout account without MFA".into());
        }
    };
    let local_only_user = local_only_credentials.user_id();
    let local_only_refresh = local_only_credentials.refresh_token().expose().to_owned();
    let local_only_store = openrust::session::MemorySessionStore::default();
    let local_only_sessions = SessionStoreHandle::new(local_only_store.clone());
    let local_only_session = Session::new(None, Arc::new(local_only_store.clone()), None)?;
    local_only_session.start().await?;
    local_only_session.put_typed("openrust.auth.user_id", local_only_user)?;
    local_only_session.save().await?;
    let local_only_session_id = local_only_session.id()?;
    external
        .attach_session(local_only_user, &local_only_handle, &local_only_session)
        .await?;
    provider.logout_available.store(false, Ordering::SeqCst);
    assert!(
        external
            .begin_logout(
                local_only_user,
                &local_only_handle,
                "",
                &local_only_sessions
            )
            .await?
            .is_none(),
        "missing IdP logout support must report local-only logout"
    );
    provider.logout_available.store(true, Ordering::SeqCst);
    assert!(refresh.rotate(&local_only_refresh).await.is_err());
    assert!(
        local_only_store
            .load(
                &local_only_session_id,
                openrust::session::DEFAULT_MAX_BYTES,
                time::OffsetDateTime::now_utc() + time::Duration::minutes(120)
            )
            .await?
            .is_none()
    );

    let audit_table = db.quoted_table_name("audit_records")?;
    let audit_count = connection
        .query_one_raw(Statement::from_string(
            connection.get_database_backend(),
            format!("SELECT COUNT(*) AS row_count FROM {audit_table}"),
        ))
        .await?
        .ok_or("audit count query returned no row")?
        .try_get::<i64>("", "row_count")?;
    assert_eq!(
        audit_count, 12,
        "external logins, binding, and both scoped logout paths must be audited"
    );

    #[cfg(feature = "auth-fortify")]
    Box::pin(verify_external_two_factor_completion(&provider, &auth, &db, &connection, &refresh, &jwt)).await?;

    drop(external);
    drop(auth);
    drop(db);
    drop(connection);
    Ok(())
    }).await;
    let cleanup = database.close().await;
    outcome
        .map_err(|error| -> Box<dyn StdError> { format!("OIDC integration: {error}").into() })?;
    cleanup?;
    Ok(())
}

fn state_from_url(url: &url::Url) -> std::result::Result<String, Box<dyn StdError>> {
    url.query_pairs()
        .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
        .ok_or_else(|| "authorization URL has no state query parameter".into())
}

#[cfg(feature = "auth-fortify")]
#[allow(
    clippy::too_many_lines,
    reason = "同一双库用例贯通外部身份绑定、本地 MFA challenge、凭据/Session 关联与退出撤销"
)]
async fn verify_external_two_factor_completion(
    provider: &Arc<TestProvider>,
    auth: &AuthFlows,
    db: &Db,
    connection: &openrust::database::DatabaseConnection,
    refresh: &RefreshTokens,
    jwt: &JwtTokens,
) -> TestResult {
    use openrust::auth::LoginAttempt;

    use openrust::auth::UserProvider as _;
    let encryptor = Encryptor::new(EncryptionConfig::new(
        "test-key",
        BTreeMap::from([("test-key".into(), STANDARD.encode([0xa3_u8; 32]))]),
    ))?;
    let parts = web::Parts::new(
        db.clone(),
        std::sync::Arc::new(openrust::hashing::HashManager::new(
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
        )?),
        encryptor.clone(),
        auth.clone(),
    )
    .await?;
    let auth = &parts.auth;
    let external = ExternalIdentityFlows::new(
        db.clone(),
        auth.clone(),
        encryptor,
        [provider.clone() as Arc<dyn ExternalIdentityProvider>],
    )?;
    let email = "oidc-mfa@example.test";
    let password = "correct horse battery staple";
    let issued = auth
        .register(email, password, "oidc-mfa-register")
        .await?
        .ok_or("MFA OIDC test account was not created")?;
    assert!(
        auth.verify_email(email, issued.code(), "oidc-mfa-verify")
            .await?
    );
    let users = db.quoted_table_name("users")?;
    let backend = connection.get_database_backend();
    let user_id = connection
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            format!(
                "SELECT id FROM {users} WHERE email = {}",
                if backend == openrust::database::sea_orm::DbBackend::Postgres {
                    "$1"
                } else {
                    "?"
                }
            ),
            [email.into()],
        ))
        .await?
        .ok_or("MFA OIDC test user was not stored")?
        .try_get::<i64>("", "id")?;

    let user = parts
        .users
        .retrieve_by_id(&serde_json::json!(user_id))
        .await?
        .ok_or("missing factor user")?;
    parts.actions.enable(&user, false).await?;
    let encoded_secret = parts.actions.secret_key(&user).await?;
    let secret = decode_base32_secret(&encoded_secret)?;
    parts
        .actions
        .confirm(&user, &totp_code(&secret, 0)?)
        .await?;
    let recovery_codes = parts.actions.recovery_codes(&user).await?;
    assert_eq!(recovery_codes.len(), 8);

    provider.set_identity("mfa-bind", identity("sub-mfa", email, true)?)?;
    let binding = external.authorize("test-idp", "127.0.0.42").await?;
    let binding_state = state_from_url(binding.url())?;
    external
        .bind(
            user_id,
            email,
            password,
            "oidc-mfa-bind",
            "test-idp",
            &binding_state,
            "mfa-bind",
            binding.browser_binding_cookie(),
        )
        .await
        .map_err(|error| format!("MFA test external identity bind failed: {error:?}"))?;

    provider.set_identity("mfa-login", identity("sub-mfa", email, true)?)?;
    let login = external.authorize("test-idp", "127.0.0.43").await?;
    let login_state = state_from_url(login.url())?;
    let result = external
        .complete_with_handle(
            "test-idp",
            &login_state,
            "mfa-login",
            login.browser_binding_cookie(),
            "127.0.0.43",
        )
        .await
        .map_err(|error| format!("MFA test external callback failed: {error:?}"))?;
    let (attempt, handle) = result.into_parts();
    let challenge = match attempt {
        LoginAttempt::TwoFactorRequired(challenge) => challenge.expose().to_owned(),
        LoginAttempt::Authenticated(_) => {
            return Err("external login bypassed the enabled local MFA factor".into());
        }
    };
    let associations = db.quoted_table_name("auth_external_sessions")?;
    let association = connection
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            format!(
                "SELECT challenge_hash, refresh_family_id FROM {associations} WHERE id_hash = {}",
                if backend == openrust::database::sea_orm::DbBackend::Postgres {
                    "$1"
                } else {
                    "?"
                }
            ),
            [format!("{:x}", sha2::Sha256::digest(handle.expose())).into()],
        ))
        .await?
        .ok_or("external MFA association was not persisted")?;
    assert_eq!(
        association.try_get::<Option<String>>("", "challenge_hash")?,
        Some(format!("{:x}", sha2::Sha256::digest(&challenge)))
    );
    assert_eq!(
        association.try_get::<Option<String>>("", "refresh_family_id")?,
        None,
        "the external login must not have refresh credentials before MFA"
    );

    let credentials = auth
        .complete_two_factor_login(
            &challenge,
            Some(&totp_code(&secret, 1)?),
            None,
            "127.0.0.43",
        )
        .await
        .map_err(|error| format!("MFA test challenge completion failed: {error:?}"))?;
    let refresh_token = credentials.refresh_token().expose().to_owned();
    let associated_family: Option<String> = connection
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            format!(
                "SELECT refresh_family_id FROM {associations} WHERE id_hash = {}",
                if backend == openrust::database::sea_orm::DbBackend::Postgres {
                    "$1"
                } else {
                    "?"
                }
            ),
            [format!("{:x}", sha2::Sha256::digest(handle.expose())).into()],
        ))
        .await?
        .ok_or("MFA OIDC association disappeared after completion")?
        .try_get("", "refresh_family_id")?;
    assert!(associated_family.is_some_and(|value| !value.is_empty()));
    let session_store = openrust::session::MemorySessionStore::default();
    let sessions = SessionStoreHandle::new(session_store.clone());
    // 直接凭据挑战与官方Web登录是独立入口；先撤销前一真实家族，再验证新的Web关联。
    let direct_logout = external
        .begin_logout(
            user_id,
            &handle,
            "https://idp.example.test/callback",
            &sessions,
        )
        .await?
        .ok_or("missing direct logout")?;
    external
        .complete_logout("test-idp", direct_logout.state())
        .await?;
    assert!(refresh.rotate(&refresh_token).await.is_err());
    let web_authorization = external.authorize("test-idp", "127.0.0.44").await?;
    let web_state = state_from_url(web_authorization.url())?;
    let (attempt, handle) = external
        .complete_with_handle(
            "test-idp",
            &web_state,
            "mfa-login",
            web_authorization.browser_binding_cookie(),
            "127.0.0.44",
        )
        .await?
        .into_parts();
    let LoginAttempt::TwoFactorRequired(web_challenge) = attempt else {
        return Err("Web flow skipped challenge".into());
    };
    let mut browser = web::Fixture::new(parts.clone(), external.clone(), sessions.clone()).await?;
    let result: TestResult = async {
        let pending = browser.begin(&web_challenge, &handle).await?;
        assert_eq!(pending.status, http::StatusCode::ACCEPTED);
        assert!(
            pending.outcome.is_none(),
            "pending Web challenge must not issue credentials"
        );
        let cookie = web::cookie_header(&pending.headers)?;
        let csrf = pending
            .headers
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok())
            .ok_or("missing actual CSRF token")?;
        let rejected = browser
            .complete(&cookie, None, None, Some(&recovery_codes[1]))
            .await?;
        assert_eq!(rejected.status, http::StatusCode::from_u16(419)?);
        assert!(rejected.outcome.is_none());
        let completed = browser
            .complete(&cookie, Some(csrf), None, Some(&recovery_codes[1]))
            .await?;
        assert_eq!(completed.status, http::StatusCode::OK);
        let logged_in = completed.outcome.ok_or("Web login omitted outcome")?;
        assert_eq!(logged_in.user_id, user_id);
        assert!(<str as PartialEq>::eq(
            logged_in.handle.expose(),
            handle.expose()
        ));
        assert_eq!(jwt.verify_access_token(&logged_in.access_token)?, user_id);
        let session_id = logged_in.session_id;
        let refresh_token = logged_in.refresh_token;
        let replaced_codes = parts.actions.recovery_codes(&user).await?;
        assert_eq!(replaced_codes.len(), 8);
        assert!(!replaced_codes.contains(&recovery_codes[1]));
        assert_eq!(
            replaced_codes
                .iter()
                .filter(|code| recovery_codes.contains(code))
                .count(),
            7,
            "真实Web完成只替换一份恢复码，保留其余七份"
        );

        let redirect = external
            .begin_logout(
                user_id,
                &handle,
                "https://idp.example.test/callback",
                &sessions,
            )
            .await
            .map_err(|error| format!("MFA test logout failed: {error:?}"))?
            .ok_or("MFA OIDC logout endpoint was not available")?;
        assert!(refresh.rotate(&refresh_token).await.is_err());
        assert!(
            session_store
                .load(
                    &session_id,
                    openrust::session::DEFAULT_MAX_BYTES,
                    time::OffsetDateTime::now_utc() + time::Duration::minutes(120)
                )
                .await?
                .is_none()
        );
        assert!(
            external
                .complete_logout("test-idp", redirect.state())
                .await
                .is_ok()
        );
        // 禁用后另一次真实IdP首因子直接取得凭据；Web只复用该家族，不额外签发脱离SLO的家族。
        parts.actions.disable(&user).await?;
        let authorization = external.authorize("test-idp", "oidc-web-no-factor").await?;
        let state = state_from_url(authorization.url())?;
        let (attempt, plain_handle) = external
            .complete_with_handle(
                "test-idp",
                &state,
                "mfa-login",
                authorization.browser_binding_cookie(),
                "oidc-web-no-factor",
            )
            .await?
            .into_parts();
        let LoginAttempt::Authenticated(plain_credentials) = attempt else {
            return Err("disabled factor unexpectedly challenged".into());
        };
        let refresh_rows = count_rows(connection, db, "auth_refresh_tokens").await?;
        let ready = browser
            .begin_authenticated(&plain_credentials, &plain_handle)
            .await?;
        assert_eq!(ready.status, http::StatusCode::OK);
        assert_eq!(
            count_rows(connection, db, "auth_refresh_tokens").await?,
            refresh_rows,
            "Web attachment must not mint a second family"
        );
        let ready = ready.outcome.ok_or("missing no-factor Web identity")?;
        assert_eq!(ready.access_token, plain_credentials.access_token());
        assert_eq!(
            ready.refresh_token,
            plain_credentials.refresh_token().expose()
        );
        let redirect = external
            .begin_logout(
                user_id,
                &plain_handle,
                "https://idp.example.test/callback",
                &sessions,
            )
            .await?
            .ok_or("missing no-factor logout")?;
        external
            .complete_logout("test-idp", redirect.state())
            .await?;
        assert!(refresh.rotate(&ready.refresh_token).await.is_err());
        assert!(
            session_store
                .load(
                    &ready.session_id,
                    openrust::session::DEFAULT_MAX_BYTES,
                    time::OffsetDateTime::now_utc()
                )
                .await?
                .is_none()
        );
        Ok(())
    }
    .await;
    let cleanup = browser.shutdown().await;
    result?;
    cleanup?;
    Ok(())
}

#[cfg(feature = "auth-fortify")]
fn decode_base32_secret(encoded: &str) -> TestResult<Vec<u8>> {
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

#[cfg(feature = "auth-fortify")]
fn totp_code(secret: &[u8], step_offset: u64) -> TestResult<String> {
    let step = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        / 30
        + step_offset;
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret)?;
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[19] & 0x0f);
    let value = (u32::from(digest[offset] & 0x7f) << 24)
        | (u32::from(digest[offset + 1]) << 16)
        | (u32::from(digest[offset + 2]) << 8)
        | u32::from(digest[offset + 3]);
    Ok(format!("{:06}", value % 1_000_000))
}

async fn count_rows(
    connection: &openrust::database::DatabaseConnection,
    db: &Db,
    logical_table: &str,
) -> std::result::Result<i64, Box<dyn StdError>> {
    let table = db.quoted_table_name(logical_table)?;
    Ok(connection
        .query_one_raw(Statement::from_string(
            connection.get_database_backend(),
            format!("SELECT COUNT(*) AS row_count FROM {table}"),
        ))
        .await?
        .ok_or("count query returned no row")?
        .try_get::<i64>("", "row_count")?)
}

async fn lookup_user_id(
    connection: &openrust::database::DatabaseConnection,
    db: &Db,
    email: &str,
) -> std::result::Result<i64, Box<dyn StdError>> {
    let users = db.quoted_table_name("users")?;
    let backend = connection.get_database_backend();
    let sql = match backend {
        openrust::database::DbBackend::Postgres => {
            format!("SELECT id FROM {users} WHERE email = $1")
        }
        openrust::database::DbBackend::MySql => format!("SELECT id FROM {users} WHERE email = ?"),
        _ => return Err("unexpected database backend".into()),
    };
    Ok(connection
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            &sql,
            [email.into()],
        ))
        .await?
        .ok_or("user lookup returned no row")?
        .try_get::<i64>("", "id")?)
}

async fn expire_request(
    connection: &openrust::database::DatabaseConnection,
    db: &Db,
    state: &str,
) -> std::result::Result<(), Box<dyn StdError>> {
    use sha2::{Digest, Sha256};
    let table = db.quoted_table_name("external_identity_requests")?;
    let hash = format!("{:x}", Sha256::digest(state.as_bytes()));
    let backend = connection.get_database_backend();
    let sql = match backend {
        openrust::database::DbBackend::Postgres => {
            format!(
                "UPDATE {table} SET expires_at = CURRENT_TIMESTAMP - INTERVAL '1 minute' WHERE state_hash = $1"
            )
        }
        openrust::database::DbBackend::MySql => {
            format!(
                "UPDATE {table} SET expires_at = CURRENT_TIMESTAMP(6) - INTERVAL 1 MINUTE WHERE state_hash = ?"
            )
        }
        _ => return Err("unexpected database backend".into()),
    };
    connection
        .execute_raw(Statement::from_sql_and_values(backend, &sql, [hash.into()]))
        .await?;
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

#[cfg(feature = "auth-fortify")]
#[path = "oidc/authority.rs"]
mod authority;

/// 两个独立test binary共享同一真实Web夹具，不重复编译另一个测试模块。
#[cfg(feature = "auth-fortify")]
#[path = "oidc/web.rs"]
mod web;
