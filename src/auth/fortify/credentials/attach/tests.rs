//! 真实Provider/密码登录与同请求未保存Session的关联授权边界。
use super::*;
use crate::{
    auth::{
        AuthFlows, AuthUser, Authenticatable, Credentials, DatabaseUserProvider,
        DatabaseVerificationCodeStore, GenericUser, JwtTokens, RefreshTokens, SessionGuard,
        VerificationCodes,
        fortify::{
            DatabaseFactorAuthority, DatabaseTwoFactorState, FactorAuthorityMigration,
            FactorAuthorityMutation, FortifyTwoFactorMigration, TwoFactorAuthenticatable,
            TwoFactorState,
        },
    },
    config::{EncryptionConfig, JwtConfig},
    core::{BoxFuture, RateLimiter},
    db::{
        DatabaseRateLimitMigration, DatabaseRateLimitStore, Db,
        sea_orm_migration::{MigrationTrait, MigratorTrait},
    },
    http::{CookieFactory, CookieJar},
    security::Encryptor,
    session::MemorySessionStore,
    testing::TestDatabase,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::collections::BTreeMap;

struct Migrator;
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(crate::auth::AuthMigration),
            Box::new(crate::auth::AuthRememberTokenMigration),
            Box::new(crate::auth::AuthVerificationCodeMigration),
            Box::new(DatabaseRateLimitMigration),
            Box::new(FortifyTwoFactorMigration::new()),
            Box::new(FactorAuthorityMigration::new()),
        ]
    }
}
struct Account {
    attributes: GenericUser,
    store: DatabaseTwoFactorState,
    authority: DatabaseFactorAuthority,
}
fn text(attributes: &GenericUser, name: &str) -> Result<Option<String>> {
    match attributes.attribute(name)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        _ => Err(Error::Unauthorized),
    }
}
impl Authenticatable for Account {
    fn auth_identifier_name(&self) -> &'static str {
        "id"
    }
    fn auth_identifier(&self) -> Result<Value> {
        self.attributes.auth_identifier()
    }
    fn auth_password_name(&self) -> &'static str {
        "password_hash"
    }
    fn auth_password(&self) -> Result<Option<String>> {
        text(&self.attributes, "password_hash")
    }
    fn remember_token(&self) -> Result<Option<String>> {
        self.attributes.remember_token()
    }
    fn remember_token_for_cookie(&self) -> Result<Option<String>> {
        self.attributes.remember_token_for_cookie()
    }
    fn set_remember_token(&self, value: Option<String>) -> crate::core::BoxFuture<'_, Result<()>> {
        self.attributes.set_remember_token(value)
    }
    fn begin_remember_token_update<'a>(
        &'a self,
        expected: Option<&'a str>,
        replacement: Option<String>,
    ) -> crate::core::BoxFuture<'a, Result<Option<crate::auth::RememberTokenUpdate>>> {
        self.attributes
            .begin_remember_token_update(expected, replacement)
    }
    fn remember_token_name(&self) -> &str {
        self.attributes.remember_token_name()
    }
    fn as_two_factor(&self) -> Option<&dyn TwoFactorAuthenticatable> {
        Some(self)
    }
}
impl TwoFactorAuthenticatable for Account {
    fn two_factor_account_name(&self, name: &str) -> Result<Option<String>> {
        text(&self.attributes, name)
    }
    fn two_factor_authority(&self) -> Option<&DatabaseFactorAuthority> {
        Some(&self.authority)
    }
    fn two_factor_state(&self) -> BoxFuture<'_, Result<TwoFactorState>> {
        self.store.load()
    }
    fn compare_and_set_two_factor_state<'a>(
        &'a self,
        before: &'a TwoFactorState,
        after: &'a TwoFactorState,
    ) -> BoxFuture<'a, Result<bool>> {
        self.store.compare_and_set(before, after)
    }
}
struct Fixture {
    database: TestDatabase,
    db: Db,
    policy: CredentialAuthority,
    authority: DatabaseFactorAuthority,
    flows: AuthFlows,
    passwords: std::sync::Arc<dyn crate::hashing::Hasher>,
    encrypter: Encryptor,
}
impl Fixture {
    async fn new() -> Result<Self> {
        let database = TestDatabase::sqlite_file::<Migrator>().await?;
        let setup: Result<_> = async {
            let db = Db::default();
            db.initialize(database.connection()?)?;
            let key = STANDARD.encode([21_u8; 32]);
            let encrypter = Encryptor::new(EncryptionConfig::new(
                "owned",
                BTreeMap::from([("owned".into(), key.clone())]),
            ))?;
            let passwords = std::sync::Arc::new(crate::hashing::HashManager::new(
                crate::hashing::HashingConfig {
                    bcrypt: crate::hashing::BcryptConfig {
                        rounds: 4,
                        verify: true,
                        limit: None,
                    },
                    ..Default::default()
                },
            )?) as std::sync::Arc<dyn crate::hashing::Hasher>;
            let hash = crate::auth::EncodedPasswordHash::make(
                passwords.as_ref(),
                "attachment-owned-password",
            )
            .await?;
            let now = crate::db::dialect::utc_value(crate::db::DbBackend::Sqlite, Utc::now());
            db.table("users")?
                .insert(vec![
                    ("id", 42_i64.into()),
                    ("email", "attachment@example.test".into()),
                    ("password_hash", hash.as_str().into()),
                    ("email_verified_at", now.clone()),
                    ("created_at", now.clone()),
                    ("updated_at", now),
                ])
                .await?;
            let policy = CredentialAuthority::new(db.clone(), encrypter.clone(), true);
            let authority = DatabaseFactorAuthority::new(
                DatabaseTwoFactorState::new(db.clone(), "users", "id", Value::from(42))?,
                encrypter.clone(),
            );
            let jwt = JwtTokens::new(JwtConfig::new(
                "owned",
                "attachment",
                "owned",
                BTreeMap::from([("owned".into(), key)]),
            ))?;
            let flows = AuthFlows::new(
                db.clone(),
                passwords.clone(),
                jwt,
                RefreshTokens::new(db.clone()),
                VerificationCodes::new(
                    DatabaseVerificationCodeStore::new(db.clone()),
                    passwords.clone(),
                ),
                RateLimiter::new(DatabaseRateLimitStore::new(db.clone())),
            )
            .with_factor_authority(policy.clone())?;
            Ok((db, policy, authority, flows, passwords, encrypter))
        }
        .await;
        match setup {
            Ok((db, policy, authority, flows, passwords, encrypter)) => Ok(Self {
                database,
                db,
                policy,
                authority,
                flows,
                passwords,
                encrypter,
            }),
            Err(error) => {
                database.close().await?;
                Err(error)
            }
        }
    }
    async fn named(&self, session: Session) -> Result<SessionGuard> {
        let db = self.db.clone();
        let crypto = self.encrypter.clone();
        let provider = DatabaseUserProvider::new(self.db.clone(), self.passwords.clone(), "users")?
            .with_user_factory(move |attributes| {
                let id = attributes.get("id").cloned().ok_or(Error::Unauthorized)?;
                let store = DatabaseTwoFactorState::new(db.clone(), "users", "id", id)?;
                Ok(Arc::new(Account {
                    attributes: GenericUser::new(attributes),
                    authority: DatabaseFactorAuthority::new(store.clone(), crypto.clone()),
                    store,
                }) as AuthUser)
            });
        let guard = SessionGuard::new(
            "tenant-web".to_owned(),
            Arc::new(provider),
            session,
            CookieJar::new(CookieFactory::default()),
            None,
            None,
            None,
        )
        .with_provider_name(Some("app-users".into()))
        .with_remember_cookie_protected(true);
        check(guard.attempt(&credentials(), false).await?)?;
        Ok(guard)
    }
}
fn credentials() -> Credentials {
    Credentials::new(serde_json::Map::from_iter([
        ("email".into(), Value::from("attachment@example.test")),
        ("password".into(), Value::from("attachment-owned-password")),
    ]))
}
fn check(value: bool) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(Error::custom(
            "attachment_contract",
            "Session关联授权契约断言失败",
        ))
    }
}
async fn new_session() -> Result<Session> {
    let session = Session::new(None, Arc::new(MemorySessionStore::default()), None)?;
    session.start().await?;
    Ok(session)
}
async fn validate(
    policy: &CredentialAuthority,
    prepared: &PreparedSessionAttachment,
) -> Result<()> {
    let connection = policy.db.connection()?;
    let tx = crate::db::begin_for_update(&connection).await?;
    let result = policy
        .validate_attachment_in_transaction(&tx, prepared)
        .await;
    let rollback = tx.rollback().await.map_err(Error::internal);
    result?;
    rollback
}

#[tokio::test]
async fn session_context_attachment_rejects_copied_context_wrong_identity_rotation_and_generation()
-> Result<()> {
    let fixture = Fixture::new().await?;
    let outcome: Result<()> = async {
        let session = new_session().await?;
        fixture
            .flows
            .login_session(
                "attachment@example.test",
                "attachment-owned-password",
                "owned-source",
                &session,
            )
            .await?;
        let prepared = fixture
            .policy
            .prepare_session_context_attachment(42, &session)
            .await?;
        check(prepared.user_id() == 42)?;
        prepared.recheck_session(&session)?;
        validate(&fixture.policy, &prepared).await?;
        check(
            fixture
                .policy
                .prepare_session_context_attachment(43, &session)
                .await
                .is_err(),
        )?;
        let copied = new_session().await?;
        copied.put_typed(crate::auth::AUTH_USER_ID_KEY, 42_i64)?;
        let stored = session
            .get_typed::<StoredSessionFactorContext>(
                crate::auth::credential::SESSION_FACTOR_CONTEXT_KEY,
            )?
            .ok_or(Error::Unauthorized)?;
        copied.put_typed(crate::auth::credential::SESSION_FACTOR_CONTEXT_KEY, stored)?;
        check(
            fixture
                .policy
                .prepare_session_context_attachment(42, &copied)
                .await
                .is_err(),
        )?;
        check(prepared.recheck_session(&copied).is_err())?;
        let before = fixture.authority.load().await?;
        fixture
            .authority
            .replace(
                &before,
                &TwoFactorState::default(),
                FactorAuthorityMutation::Disable,
            )
            .await?;
        check(matches!(
            validate(&fixture.policy, &prepared).await,
            Err(Error::Unauthorized)
        ))?;
        session.migrate(true).await?;
        check(prepared.recheck_session(&session).is_err())
    }
    .await;
    let cleanup = fixture.database.close().await;
    outcome?;
    cleanup
}
#[tokio::test]
async fn guard_attachment_binds_actual_session_rejects_once_and_current_generation() -> Result<()> {
    let fixture = Fixture::new().await?;
    let outcome: Result<()> = async {
        let session = new_session().await?;
        let guard = fixture.named(session.clone()).await?;
        let prepared = fixture
            .policy
            .prepare_guard_attachment(42, &session, &guard)
            .await?;
        prepared.recheck_guard(&session, &guard).await?;
        validate(&fixture.policy, &prepared).await?;
        let AttachmentOrigin::Guard { proof_key, .. } = &prepared.origin else {
            return Err(Error::Unauthorized);
        };
        session.put_typed(proof_key, serde_json::json!({"not-a-proof":true}))?;
        check(prepared.recheck_session(&session).is_err())?;
        session.remove(proof_key)?;
        prepared.recheck_session(&session)?;
        let copied = new_session().await?;
        copied.put_typed(&guard.session_name(), 42_i64)?;
        check(
            fixture
                .policy
                .prepare_guard_attachment(42, &copied, &guard)
                .await
                .is_err(),
        )?;
        check(prepared.recheck_session(&copied).is_err())?;
        let before = fixture.authority.load().await?;
        fixture
            .authority
            .replace(
                &before,
                &TwoFactorState::default(),
                FactorAuthorityMutation::Disable,
            )
            .await?;
        check(matches!(
            validate(&fixture.policy, &prepared).await,
            Err(Error::Unauthorized)
        ))?;
        check(prepared.recheck_guard(&session, &guard).await.is_err())?;
        check(guard.once(&credentials()).await?)?;
        check(
            fixture
                .policy
                .prepare_guard_attachment(42, &session, &guard)
                .await
                .is_err(),
        )?;
        check(prepared.recheck_guard(&session, &guard).await.is_err())?;
        session.migrate(true).await?;
        check(prepared.recheck_session(&session).is_err())
    }
    .await;
    let cleanup = fixture.database.close().await;
    outcome?;
    cleanup
}
