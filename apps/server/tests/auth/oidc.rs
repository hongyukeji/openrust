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
        atomic::{AtomicUsize, Ordering},
    },
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use openrust::{
    Error, Result,
    audit::DatabaseAuditMigration,
    auth::{
        AuthExternalIdentityMigration, AuthFlows, AuthMigration, AuthRefreshTokenMigration,
        AuthVerificationCodeMigration, DatabaseVerificationCodeStore, ExternalIdentityFlows,
        ExternalIdentityProvider, JwtConfig, JwtTokens, OidcAuthorization, PasswordConfig,
        Passwords, RefreshTokens, VerifiedExternalIdentity,
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
use testing::TestDatabase;

type TestResult = std::result::Result<(), Box<dyn StdError>>;

struct OidcMigrator;

impl MigratorTrait for OidcMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(AuthMigration),
            Box::new(AuthRefreshTokenMigration),
            Box::new(AuthVerificationCodeMigration),
            Box::new(DatabaseRateLimitMigration),
            Box::new(DatabaseAuditMigration),
            Box::new(AuthExternalIdentityMigration),
        ]
    }
}

struct TestProvider {
    next_authorization: AtomicUsize,
    identities: RwLock<HashMap<String, VerifiedExternalIdentity>>,
    authentication_calls: AtomicUsize,
}

impl TestProvider {
    fn new() -> Self {
        Self {
            next_authorization: AtomicUsize::new(0),
            identities: RwLock::new(HashMap::new()),
            authentication_calls: AtomicUsize::new(0),
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
#[allow(clippy::too_many_lines)]
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
    let connection = database.connection()?;
    let db = Db::with_table_names(names);
    db.initialize(connection.clone())?;

    let passwords = Passwords::new(PasswordConfig {
        memory_kib: 19_456,
        iterations: 1,
        parallelism: 1,
    })?;
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
    let auth = AuthFlows::new(
        db.clone(),
        passwords.clone(),
        jwt,
        RefreshTokens::new(db.clone()),
        codes,
        RateLimiter::new(DatabaseRateLimitStore::new(db.clone())),
    );
    let encryptor = Encryptor::new(EncryptionConfig::new(
        "test-key",
        BTreeMap::from([("test-key".to_owned(), STANDARD.encode([0xa3_u8; 32]))]),
    ))?;
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

    let credentials = external
        .complete(
            "test-idp",
            &state,
            "verified-login",
            login.browser_binding_cookie(),
            "127.0.0.3",
        )
        .await?;
    assert!(credentials.user_id() > 0);
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
    assert_eq!(provider.authentication_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        count_rows(&connection, &db, "external_identities").await?,
        1
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
        1
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
        1
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

    let audit_table = db.quoted_table_name("audit_records")?;
    let audit_count = connection
        .query_one_raw(Statement::from_string(
            connection.get_database_backend(),
            format!("SELECT COUNT(*) AS row_count FROM {audit_table}"),
        ))
        .await?
        .ok_or("audit count query returned no row")?
        .try_get::<i64>("", "row_count")?;
    assert_eq!(audit_count, 3, "login, bind, and unbind must be audited");

    drop(external);
    drop(auth);
    drop(db);
    drop(connection);
    database.close().await?;
    Ok(())
}

fn state_from_url(url: &url::Url) -> std::result::Result<String, Box<dyn StdError>> {
    url.query_pairs()
        .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
        .ok_or_else(|| "authorization URL has no state query parameter".into())
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
