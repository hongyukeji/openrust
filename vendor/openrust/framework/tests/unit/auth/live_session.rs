use super::*;
use crate::auth::AuthUser;
use crate::auth::Authenticatable;
use crate::auth::DatabaseUserProvider;
use crate::auth::GenericUser;
use crate::auth::SessionGuard;
use crate::auth::fortify::DatabaseFactorAuthority;
use crate::auth::fortify::DatabaseTwoFactorState;
use crate::auth::fortify::FactorAuthorityMigration;
use crate::auth::fortify::FactorAuthorityMutation;
use crate::auth::fortify::FortifyTwoFactorMigration;
use crate::auth::fortify::TwoFactorAuthenticatable;
use crate::auth::fortify::TwoFactorState;
use crate::database::Db;
use crate::database::sea_orm_migration::MigrationTrait;
use crate::database::sea_orm_migration::MigratorTrait;
use crate::http::CookieFactory;
use crate::http::CookieJar;
use crate::session::MemorySessionStore;
use crate::support::BoxFuture;
use crate::testing::TestDatabase;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::collections::BTreeMap;
struct Migrator;
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(crate::auth::AuthMigration),
            Box::new(crate::auth::AuthRememberTokenMigration),
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
        string_attribute(&self.attributes, "password_hash")
    }
    fn remember_token(&self) -> Result<Option<String>> {
        self.attributes.remember_token()
    }
    fn remember_token_for_cookie(&self) -> Result<Option<String>> {
        self.attributes.remember_token_for_cookie()
    }
    fn set_remember_token(&self, value: Option<String>) -> BoxFuture<'_, Result<()>> {
        self.attributes.set_remember_token(value)
    }
    fn begin_remember_token_update<'a>(
        &'a self,
        expected: Option<&'a str>,
        replacement: Option<String>,
    ) -> BoxFuture<'a, Result<Option<crate::auth::RememberTokenUpdate>>> {
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
fn string_attribute(attributes: &GenericUser, key: &str) -> Result<Option<String>> {
    match attributes.attribute(key)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        _ => Err(Error::Unauthorized),
    }
}
impl TwoFactorAuthenticatable for Account {
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
    fn two_factor_account_name(&self, username: &str) -> Result<Option<String>> {
        string_attribute(&self.attributes, username)
    }
}
async fn guard(db: &Db, session: Session) -> Result<SessionGuard> {
    let model_db = db.clone();
    let provider = DatabaseUserProvider::new(
        db.clone(),
        std::sync::Arc::new(crate::hashing::HashManager::new(
            crate::hashing::HashingConfig {
                bcrypt: crate::hashing::BcryptConfig {
                    rounds: 4,
                    verify: true,
                    limit: None,
                },
                ..Default::default()
            },
        )?) as std::sync::Arc<dyn crate::hashing::Hasher>,
        "users",
    )?
    .with_user_factory(move |attributes| {
        let id = attributes.get("id").cloned().ok_or(Error::Unauthorized)?;
        let store = DatabaseTwoFactorState::new(model_db.clone(), "users", "id", id)?;
        Ok(Arc::new(Account {
            attributes: GenericUser::new(attributes),
            authority: DatabaseFactorAuthority::new(store.clone()),
            store,
        }) as AuthUser)
    });
    let user = provider
        .retrieve_by_id(&Value::from(42))
        .await?
        .ok_or(Error::Unauthorized)?;
    let guard = SessionGuard::new(
        "web".to_owned(),
        Arc::new(provider),
        session,
        CookieJar::new(CookieFactory::default()),
        None,
        None,
        None,
    )
    .with_provider_name(Some("users".into()))
    .with_remember_cookie_protected(true);
    // 受信低层第一因素登录；NoFactor不是MFA证明，测试不签发FactorReceipt。
    guard.login(user, false).await?;
    Ok(guard)
}
struct Fixture {
    database: TestDatabase,
    db: Db,
    policy: CredentialAuthority,
    session: Session,
    sessions: super::super::SessionStoreHandle,
    guard: SessionGuard,
}
impl Fixture {
    async fn new() -> Result<Self> {
        let database = TestDatabase::sqlite_file::<Migrator>().await?;
        let setup: Result<_> = async {
            let db = Db::default();
            db.initialize(database.connection()?)?;
            let now = crate::database::dialect::utc_value(
                crate::database::DbBackend::Sqlite,
                chrono::Utc::now(),
            );
            db.table("users")?
                .insert(vec![
                    ("id", 42_i64.into()),
                    ("email", "live-owned@example.test".into()),
                    ("password_hash", "not-used-low-level-login".into()),
                    ("created_at", now.clone()),
                    ("updated_at", now),
                ])
                .await?;
            let store = MemorySessionStore::default();
            let sessions = super::super::SessionStoreHandle::new(store.clone());
            let session = Session::new(None, Arc::new(store), None)?;
            session.start().await?;
            let guard = guard(&db, session.clone()).await?;
            session.save().await?;
            let policy = CredentialAuthority::new(db.clone(), true);
            Ok((db, policy, session, sessions, guard))
        }
        .await;
        match setup {
            Ok((db, policy, session, sessions, guard)) => Ok(Self {
                database,
                db,
                policy,
                session,
                sessions,
                guard,
            }),
            Err(error) => {
                database.close().await?;
                Err(error)
            }
        }
    }
    async fn ticket(&self) -> Result<LiveSessionCredential> {
        self.sessions
            .prepare_live_guard(&self.session, &self.guard)
            .await
    }
}
fn check(value: bool) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(Error::custom(
            "live_session_contract",
            "服务器端实时Session契约断言失败",
        ))
    }
}

// 真实Redis撤销发生在数据库全局提交前监听器，验证两个派生入口都会回滚子凭据。
#[cfg(feature = "session-redis")]
#[tokio::test]
#[ignore = "需要显式真实Redis连接；实服务验收必须单独执行"]
#[allow(
    clippy::too_many_lines,
    reason = "同一真实Redis与隔离数据库贯穿两种派生及回滚"
)]
async fn redis_parent_deleted_before_commit_rolls_back_jwt_and_pat() -> Result<()> {
    use crate::auth::AuthFlows;
    use crate::auth::DatabaseVerificationCodeStore;
    use crate::auth::JwtConfig;
    use crate::auth::JwtTokens;
    use crate::auth::PersonalAccessTokens;
    use crate::auth::RefreshTokens;
    use crate::auth::VerificationCodes;
    use crate::core::RateLimiter;
    use crate::database::DatabaseRateLimitMigration;
    use crate::database::DatabaseRateLimitStore;
    use crate::database::DatabaseServiceProvider;
    use crate::database::TransactionBeforeCommit;
    use crate::events::EventsServiceProvider;
    use crate::foundation::ProviderBuilder;
    use crate::session::RedisSessionStore;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn stage(name: &'static str, error: &Error) -> Error {
        eprintln!("redis_parent_test_stage={name} error_code={}", error.code());
        Error::custom(
            "redis_parent_test_stage",
            format!("{name}: {}", error.code()),
        )
    }

    struct RedisMigrator;
    impl MigratorTrait for RedisMigrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            let mut migrations = Migrator::migrations();
            migrations.push(Box::new(crate::auth::AuthRefreshTokenMigration));
            migrations.push(Box::new(crate::auth::AuthVerificationCodeMigration));
            migrations.push(Box::new(DatabaseRateLimitMigration));
            migrations.push(Box::new(
                crate::auth::fortify::CredentialFactorMigration::new(),
            ));
            migrations
        }
    }
    let mut random = [0_u8; 8];
    getrandom::fill(&mut random).map_err(Error::internal)?;
    let namespace = format!(
        "parent-{}-{:x}",
        std::process::id(),
        u64::from_be_bytes(random)
    );
    let url = std::env::var("OPENRUST_TEST_REDIS_URL").map_err(|_| {
        Error::custom(
            "redis_test_url_missing",
            "真实Redis测试需要 OPENRUST_TEST_REDIS_URL",
        )
    })?;
    let database = TestDatabase::sqlite_file::<RedisMigrator>().await?;
    let store = match RedisSessionStore::connect(url, namespace).await {
        Ok(store) => store,
        Err(error) => {
            database.close().await?;
            return Err(error);
        }
    };
    let pending = Arc::new(std::sync::Mutex::new(None::<SessionId>));
    let hits = Arc::new(AtomicUsize::new(0));
    let mut listeners = EventsServiceProvider::new();
    let hook_store = store.clone();
    let hook_pending = pending.clone();
    let hook_hits = hits.clone();
    listeners.listen::<TransactionBeforeCommit, _>(move |_, _| {
        let store = hook_store.clone();
        let pending = hook_pending.clone();
        let hits = hook_hits.clone();
        Box::pin(async move {
            let id = pending.lock().map_err(|_| Error::Unauthorized)?.take();
            if let Some(id) = id {
                store.delete(&id).await?;
                hits.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        })
    });
    let mut builder = ProviderBuilder::new();
    builder
        .provider(DatabaseServiceProvider)
        .provider(crate::events::EventServiceProvider)
        .provider(listeners);
    let runtime = match builder.start(figment::Figment::new()).await {
        Ok(runtime) => runtime,
        Err(error) => {
            database.close().await?;
            return Err(error);
        }
    };
    let mut owned_ids = Vec::new();
    let outcome: Result<()> = Box::pin(async {
        let db = (*runtime.container().resolve::<Db>().await?).clone();
        db.initialize(database.connection()?)?;
        let now = crate::database::dialect::utc_value(
            crate::database::DbBackend::Sqlite,
            chrono::Utc::now(),
        );
        db.table("users")?
            .insert(vec![
                ("id", 42_i64.into()),
                ("email", "redis-parent@example.test".into()),
                ("password_hash", "not-used-low-level-login".into()),
                ("created_at", now.clone()),
                ("updated_at", now),
            ])
            .await?;
        let policy = CredentialAuthority::new(db.clone(), true);
        let passwords = std::sync::Arc::new(crate::hashing::HashManager::new(
            crate::hashing::HashingConfig {
                bcrypt: crate::hashing::BcryptConfig {
                    rounds: 4,
                    verify: true,
                    limit: None,
                },
                ..Default::default()
            },
        )?);
        let jwt = JwtTokens::new(JwtConfig::new(
            "redis-parent",
            "redis-parent",
            "owned",
            BTreeMap::from([("owned".into(), STANDARD.encode([42_u8; 32]))]),
        ))?;
        let flows = AuthFlows::new(
            db.clone(),
            passwords.clone(),
            jwt.clone(),
            RefreshTokens::new(db.clone()),
            VerificationCodes::new(DatabaseVerificationCodeStore::new(db.clone()), passwords),
            RateLimiter::new(DatabaseRateLimitStore::new(db.clone())),
        )
        .with_factor_authority(policy.clone())?;
        let pats = PersonalAccessTokens::new(db.clone()).with_factor_authority(policy.clone())?;
        for pat in [false, true] {
            let session = Session::new(None, Arc::new(store.clone()), None)?;
            session
                .start()
                .await
                .map_err(|error| stage("session.start", &error))?;
            let guard = guard(&db, session.clone())
                .await
                .map_err(|error| stage("guard.login", &error))?;
            session
                .save()
                .await
                .map_err(|error| stage("session.save", &error))?;
            let id = session.id()?;
            owned_ids.push(id.clone());
            check(
                store
                    .load_authorized(&id, DEFAULT_MAX_BYTES)
                    .await
                    .map_err(|error| stage("redis.load_saved", &error))?
                    .is_some(),
            )?;
            if pat {
                let parent = policy
                    .prepare_session(&guard, "web")
                    .await
                    .map_err(|error| stage("pat.prepare_positive", &error))?;
                let expiry = parent.expires_at().ok_or(Error::Unauthorized)?;
                let token = pats
                    .issue_from_verified(
                        parent,
                        "redis-parent-positive",
                        &["read".into()],
                        Some(expiry),
                    )
                    .await
                    .map_err(|error| stage("pat.issue_positive", &error))?;
                pats.authenticate_verified(token.expose())
                    .await
                    .map_err(|error| stage("pat.authenticate_positive", &error))?;
            } else {
                let token = flows
                    .issue_credentials_from_session(&guard, "web")
                    .await
                    .map_err(|error| stage("jwt.issue_positive", &error))?;
                policy
                    .authenticate_access_token(&jwt, token.access_token())
                    .await
                    .map_err(|error| stage("jwt.authenticate_positive", &error))?;
            }
            let parent = policy
                .prepare_session(&guard, "web")
                .await
                .map_err(|error| {
                    stage(
                        if pat {
                            "pat.prepare_negative"
                        } else {
                            "jwt.prepare_negative"
                        },
                        &error,
                    )
                })?;
            let expiry = parent.expires_at().ok_or(Error::Unauthorized)?;
            let table = if pat {
                "personal_access_tokens"
            } else {
                "auth_refresh_tokens"
            };
            let before = db.table(table)?.count().await?;
            *pending.lock().map_err(|_| Error::Unauthorized)? = Some(id.clone());
            let rejected = if pat {
                pats.issue_from_verified(
                    parent,
                    "redis-parent-rejected",
                    &["read".into()],
                    Some(expiry),
                )
                .await
                .map(|_| ())
            } else {
                flows.issue_credentials_from(parent).await.map(|_| ())
            };
            check(matches!(rejected, Err(Error::Unauthorized)))?;
            check(
                store
                    .load_authorized(&id, DEFAULT_MAX_BYTES)
                    .await?
                    .is_none(),
            )?;
            check(db.table(table)?.count().await? == before)?;
        }
        check(hits.load(Ordering::SeqCst) == 2)
    })
    .await;
    let mut redis_cleanup = Ok(());
    for id in owned_ids {
        if let Err(error) = store.delete(&id).await {
            redis_cleanup = Err(error);
        }
    }
    let stopped = runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    let closed = database.close().await;
    outcome?;
    redis_cleanup?;
    stopped?;
    closed
}

#[tokio::test]
async fn live_ticket_reads_persisted_identity_and_logout_not_request_snapshot() -> Result<()> {
    let fixture = Fixture::new().await?;
    let outcome: Result<()> = async {
        let ticket = fixture.ticket().await?;
        ticket.authorize(&fixture.policy).await?;
        // 修改请求内属性但不保存，不得把缓存访客误当持久记录已退出。
        fixture.session.remove(&ticket.session_key)?;
        ticket.authorize(&fixture.policy).await?;
        // 另一请求保存为guest，原票据和guard缓存都还在，也必须拒绝。
        let mut record = ticket
            .handler
            .load_authorized(&ticket.session_id, DEFAULT_MAX_BYTES)
            .await?
            .ok_or(Error::Unauthorized)?;
        crate::support::data::set(&mut record.data, &ticket.session_key, Value::Null)?;
        ticket
            .handler
            .write(
                &record,
                crate::session::SessionWriteMode::Update,
                DEFAULT_MAX_BYTES,
            )
            .await?;
        check(matches!(
            ticket.authorize(&fixture.policy).await,
            Err(Error::Unauthorized)
        ))?;
        Ok(())
    }
    .await;
    let cleanup = fixture.database.close().await;
    outcome?;
    cleanup
}
#[tokio::test]
async fn live_ticket_does_not_promote_management_attributes_to_factor_proof() -> Result<()> {
    let fixture = Fixture::new().await?;
    let outcome: Result<()> = async {
        let field_crypto =
            crate::auth::fortify::test_field_encrypter(&STANDARD.encode([18_u8; 32]))?;
        let ticket = fixture.ticket().await?;
        ticket.authorize(&fixture.policy).await?;
        let authority = DatabaseFactorAuthority::new(DatabaseTwoFactorState::new(
            fixture.db.clone(),
            "users",
            "id",
            Value::from(42),
        )?);
        let before = authority.load().await?;
        authority
            .replace(
                &before,
                &TwoFactorState {
                    secret: Some(field_crypto.encrypt_serialized_string(b"JBSWY3DPEHPK3PXP")?),
                    recovery_codes: Some(field_crypto.encrypt_serialized_string(b"[]")?),
                    confirmed_at: Some(chrono::Utc::now()),
                },
                FactorAuthorityMutation::Enable,
            )
            .await?;
        // 同Session密码身份及管理标记不是通过第二因子的资源凭据。
        check(matches!(
            ticket.authorize(&fixture.policy).await,
            Err(Error::Unauthorized)
        ))?;
        let record = ticket
            .handler
            .load_authorized(&ticket.session_id, DEFAULT_MAX_BYTES)
            .await?
            .ok_or(Error::Unauthorized)?;
        check(crate::support::data::get(&record.data, &ticket.proof_key).is_none())?;
        Ok(())
    }
    .await;
    let cleanup = fixture.database.close().await;
    outcome?;
    cleanup
}
#[tokio::test]
async fn live_ticket_rejects_wrong_actual_database_handle_and_deleted_session() -> Result<()> {
    let fixture = Fixture::new().await?;
    let outcome: Result<()> = async {
        let ticket = fixture.ticket().await?;
        let other = Db::default();
        other.initialize(fixture.database.connection()?)?;
        let other_policy = CredentialAuthority::new(other, true);
        check(matches!(
            ticket.authorize(&other_policy).await,
            Err(Error::Unauthorized)
        ))?;
        ticket.handler.delete(&ticket.session_id).await?;
        check(matches!(
            ticket.authorize(&fixture.policy).await,
            Err(Error::Unauthorized)
        ))
    }
    .await;
    let cleanup = fixture.database.close().await;
    outcome?;
    cleanup
}
#[tokio::test]
async fn native_live_guard_uses_real_activity_deadline_and_rejects_expiry_or_delete() -> Result<()>
{
    let fixture = Fixture::new().await?;
    let outcome: Result<()> = async {
        let session = Session::new(
            None,
            Arc::new(
                crate::session::DatabaseSessionStore::new(fixture.db.clone())
                    .configured("sessions", 1)?,
            ),
            None,
        )?;
        session.start().await?;
        let guard = guard(&fixture.db, session.clone()).await?;
        session.save().await?;
        let sessions = super::super::SessionStoreHandle::new(
            crate::session::DatabaseSessionStore::new(fixture.db.clone())
                .configured("sessions", 1)?,
        );
        let id = session.id()?;
        let activity = OffsetDateTime::now_utc().unix_timestamp() - 30;
        fixture
            .db
            .table("sessions")?
            .where_eq("id", id.as_str())?
            .update(vec![("last_activity", activity.into())])
            .await?;
        let ticket = sessions.prepare_live_guard(&session, &guard).await?;
        let deadline = ticket.authorize(&fixture.policy).await?;
        let deadline = deadline
            .duration_since(UNIX_EPOCH)
            .map_err(Error::internal)?
            .as_secs();
        check(deadline == u64::try_from(activity + 60).map_err(Error::internal)?)?;
        let old_helper = sessions.authorized_until_guard(&session, &guard).await?;
        check(
            old_helper
                .duration_since(UNIX_EPOCH)
                .map_err(Error::internal)?
                .as_secs()
                == deadline,
        )?;
        let row = fixture
            .db
            .table("sessions")?
            .where_eq("id", id.as_str())?
            .first()
            .await?
            .ok_or(Error::Unauthorized)?;
        check(
            row.try_get::<i64>("", "last_activity")
                .map_err(Error::internal)?
                == activity,
        )?;
        fixture
            .db
            .table("sessions")?
            .where_eq("id", id.as_str())?
            .update(vec![("last_activity", (activity - 120).into())])
            .await?;
        check(matches!(
            ticket.authorize(&fixture.policy).await,
            Err(Error::Unauthorized)
        ))?;
        check(
            sessions
                .authorized_until_guard(&session, &guard)
                .await
                .is_err(),
        )?;
        // 读取过期状态既不续期也不删除；重新恢复活动行后同票据只受原最短截止约束。
        check(
            fixture
                .db
                .table("sessions")?
                .where_eq("id", id.as_str())?
                .first()
                .await?
                .is_some(),
        )?;
        fixture
            .db
            .table("sessions")?
            .where_eq("id", id.as_str())?
            .update(vec![("last_activity", activity.into())])
            .await?;
        ticket.authorize(&fixture.policy).await?;
        // 原生低层DELETE不宣称ServerSide防复活；强保证句柄delete仍应拒Native。
        ticket.handler.delete(&id).await?;
        check(matches!(
            ticket.authorize(&fixture.policy).await,
            Err(Error::Unauthorized)
        ))
    }
    .await;
    let cleanup = fixture.database.close().await;
    outcome?;
    cleanup
}
#[tokio::test]
async fn live_ticket_binds_actual_store_and_actual_guard_session_id() -> Result<()> {
    let fixture = Fixture::new().await?;
    let outcome: Result<()> = async {
        let actual = fixture.session.handler()?;
        let original_id = fixture.session.id()?;
        let record = actual
            .load_authorized(&original_id, DEFAULT_MAX_BYTES)
            .await?
            .ok_or(Error::Unauthorized)?;
        let copied_store = MemorySessionStore::default();
        copied_store
            .write(
                &record,
                crate::session::SessionWriteMode::Insert,
                DEFAULT_MAX_BYTES,
            )
            .await?;
        let wrong_handle = super::super::SessionStoreHandle::new(copied_store.clone());
        let ticket = wrong_handle
            .prepare_live_guard(&fixture.session, &fixture.guard)
            .await?;
        ticket.authorize(&fixture.policy).await?;
        // 同user/key的另一个真实Session，不是此Guard的Session；不能只比前后context稳定。
        let unrelated = Session::new(None, Arc::new(copied_store.clone()), None)?;
        unrelated.start().await?;
        unrelated.put_typed(&ticket.session_key, 42_i64)?;
        unrelated.save().await?;
        check(unrelated.id()? != original_id)?;
        check(matches!(
            wrong_handle
                .prepare_live_guard(&unrelated, &fixture.guard)
                .await,
            Err(Error::Unauthorized)
        ))?;
        actual.delete(&original_id).await?;
        check(
            copied_store
                .load_authorized(&original_id, DEFAULT_MAX_BYTES)
                .await?
                .is_some(),
        )?;
        check(matches!(
            ticket.authorize(&fixture.policy).await,
            Err(Error::Unauthorized)
        ))
    }
    .await;
    let cleanup = fixture.database.close().await;
    outcome?;
    cleanup
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "连续验证同一真实Session跨代际与策略变化，保留完整因果链"
)]
async fn live_generation_and_actual_mfa_proof_do_not_survive_disable_or_pending_policy()
-> Result<()> {
    use crate::auth::Guard as _;
    use crate::auth::fortify::{GoogleTwoFactorAuthenticationProvider, TwoFactorActions};
    let fixture = Fixture::new().await?;
    let mut builder = crate::foundation::ProviderBuilder::new();
    builder.provider(crate::events::EventServiceProvider);
    let runtime = match builder.start(figment::Figment::new()).await {
        Ok(runtime) => runtime,
        Err(error) => {
            fixture.database.close().await?;
            return Err(error);
        }
    };
    let outcome: Result<()> = Box::pin(async {
        let events = (*runtime
            .container()
            .resolve::<crate::events::Dispatcher>()
            .await?)
            .clone();
        let authority = DatabaseFactorAuthority::new(DatabaseTwoFactorState::new(
            fixture.db.clone(),
            "users",
            "id",
            Value::from(42),
        )?);
        let no_factor = fixture.ticket().await?;
        let copied = no_factor.clone();
        no_factor.authorize(&fixture.policy).await?;
        let field_crypto =
            crate::auth::fortify::test_field_encrypter(&STANDARD.encode([18_u8; 32]))?;
        let secret = field_crypto.encrypt_serialized_string(b"GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ")?;
        let recovery = field_crypto.encrypt_serialized_string(b"[]")?;
        let initial = authority.load().await?;
        let pending = authority
            .replace(
                &initial,
                &TwoFactorState {
                    secret: Some(secret.clone()),
                    recovery_codes: Some(recovery.clone()),
                    confirmed_at: None,
                },
                FactorAuthorityMutation::Enable,
            )
            .await?;
        // confirm=true的Pending不要求challenge，但已绑定NoFactor连接仍不得跨代际。
        check(matches!(
            copied.authorize(&fixture.policy).await,
            Err(Error::Unauthorized)
        ))?;
        let disabled = authority
            .replace(
                &pending,
                &TwoFactorState::default(),
                FactorAuthorityMutation::Disable,
            )
            .await?;
        check(matches!(
            no_factor.authorize(&fixture.policy).await,
            Err(Error::Unauthorized)
        ))?;
        fixture.ticket().await?.authorize(&fixture.policy).await?;
        authority
            .replace(
                &disabled,
                &TwoFactorState {
                    secret: Some(secret.clone()),
                    recovery_codes: Some(recovery.clone()),
                    confirmed_at: None,
                },
                FactorAuthorityMutation::Enable,
            )
            .await?;
        let user = fixture.guard.user().await?.ok_or(Error::Unauthorized)?;
        let actions = TwoFactorActions::new(
            Arc::new(
                GoogleTwoFactorAuthenticationProvider::new(None).with_clock(Arc::new(|| Ok(3_000))),
            ),
            field_crypto,
            events,
        );
        let code = crate::auth::totp::totp_code(b"12345678901234567890", 100, 6)?;
        // 真实TOTP确认、原子消费及受控receipt，再由真实Guard持久登录；不伪造proof。
        let receipt = actions
            .confirm_with_receipt(
                &user,
                &code,
                "web",
                Some("users"),
                fixture.guard.provider().as_ref(),
            )
            .await?
            .ok_or(Error::Unauthorized)?;
        fixture
            .guard
            .login_with_factor_receipt(user, false, receipt)
            .await?;
        fixture.session.save().await?;
        let mfa = fixture.ticket().await?;
        mfa.authorize(&fixture.policy).await?;
        let current = authority.load().await?;
        let disabled = authority
            .replace(
                &current,
                &TwoFactorState::default(),
                FactorAuthorityMutation::Disable,
            )
            .await?;
        // 新ticket还未绑定stamp：必须由持久旧proof与当前state不匹配本身拒绝。
        check(matches!(
            fixture.ticket().await?.authorize(&fixture.policy).await,
            Err(Error::Unauthorized)
        ))?;
        authority
            .replace(
                &disabled,
                &TwoFactorState {
                    secret: Some(secret),
                    recovery_codes: Some(recovery),
                    confirmed_at: None,
                },
                FactorAuthorityMutation::Enable,
            )
            .await?;
        let stale = fixture.ticket().await?;
        check(matches!(
            stale.authorize(&fixture.policy).await,
            Err(Error::Unauthorized)
        ))?;
        let unconfirmed_requires_factor = CredentialAuthority::new(fixture.db.clone(), false);
        check(matches!(
            stale.authorize(&unconfirmed_requires_factor).await,
            Err(Error::Unauthorized)
        ))
    })
    .await;
    let events_cleanup = runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await;
    let cleanup = fixture.database.close().await;
    outcome?;
    events_cleanup?;
    cleanup
}
