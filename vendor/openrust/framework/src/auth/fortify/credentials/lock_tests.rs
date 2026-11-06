//! 实际users锁等待跨越票据与父JWT期限；计时只推进真实时钟，不修改票据。
#![cfg(any(feature = "db-mysql", feature = "db-postgres"))]

use super::*;
use crate::auth::AuthFlows;
use crate::auth::AuthUser;
use crate::auth::Authenticatable;
use crate::auth::Credentials;
use crate::auth::DatabaseUserProvider;
use crate::auth::DatabaseVerificationCodeStore;
use crate::auth::GenericUser;
use crate::auth::SessionGuard;
use crate::auth::UserProvider;
use crate::auth::VerificationCodes;
use crate::auth::fortify::CredentialFactorMigration;
use crate::auth::fortify::FactorAuthorityMigration;
use crate::auth::fortify::FactorAuthorityStatus;
use crate::auth::fortify::FortifyTwoFactorMigration;
use crate::auth::fortify::TwoFactorAuthenticatable;
use crate::auth::fortify::TwoFactorState;
use crate::config::EncryptionConfig;
use crate::config::JwtConfig;
use crate::core::RateLimiter;
use crate::database::DatabaseRateLimitMigration;
use crate::database::DatabaseRateLimitStore;
use crate::database::sea_orm::QueryResult;
use crate::database::sea_orm::TransactionTrait;
use crate::database::sea_orm_migration::MigrationTrait;
use crate::database::sea_orm_migration::MigratorTrait;
use crate::http::CookieFactory;
use crate::http::CookieJar;
use crate::session::MemorySessionStore;
use crate::session::Session;
use crate::support::BoxFuture;
use crate::testing::TestDatabase;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::Digest as _;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::sync::Notify;

struct Migrator;
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(crate::auth::AuthMigration),
            Box::new(crate::auth::AuthRefreshTokenMigration),
            Box::new(crate::auth::AuthRememberTokenMigration),
            Box::new(crate::auth::AuthVerificationCodeMigration),
            Box::new(DatabaseRateLimitMigration),
            Box::new(FortifyTwoFactorMigration::new()),
            Box::new(FactorAuthorityMigration::new()),
            Box::new(CredentialFactorMigration::new()),
            Box::new(crate::auth::fortify::CredentialChallengeMigration::new()),
            #[cfg(feature = "auth-oidc")]
            Box::new(crate::auth::AuthExternalSessionMigration),
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
        self.attributes
            .attribute("password_hash")?
            .map(|value| value.as_str().map(str::to_owned).ok_or(Error::Unauthorized))
            .transpose()
    }
    fn remember_token(&self) -> Result<Option<String>> {
        self.attributes.remember_token()
    }
    fn remember_token_for_cookie(&self) -> Result<Option<String>> {
        self.attributes.remember_token_for_cookie()
    }
    fn set_remember_token(
        &self,
        token: Option<String>,
    ) -> crate::support::BoxFuture<'_, Result<()>> {
        self.attributes.set_remember_token(token)
    }
    fn begin_remember_token_update<'a>(
        &'a self,
        expected: Option<&'a str>,
        replacement: Option<String>,
    ) -> crate::support::BoxFuture<'a, Result<Option<crate::auth::RememberTokenUpdate>>> {
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
    fn two_factor_account_name(&self, username: &str) -> Result<Option<String>> {
        Ok(self
            .attributes
            .attribute(username)?
            .and_then(|value| value.as_str().map(str::to_owned)))
    }
    fn two_factor_authority(&self) -> Option<&DatabaseFactorAuthority> {
        Some(&self.authority)
    }
    fn two_factor_state(&self) -> BoxFuture<'_, Result<TwoFactorState>> {
        self.store.load()
    }
    fn compare_and_set_two_factor_state<'a>(
        &'a self,
        expected: &'a TwoFactorState,
        replacement: &'a TwoFactorState,
    ) -> BoxFuture<'a, Result<bool>> {
        self.store.compare_and_set(expected, replacement)
    }
}
fn check(value: bool) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(Error::custom(
            "credential_lock_expiry_contract",
            "实际锁等待期限断言失败",
        ))
    }
}
struct Fixture {
    database: TestDatabase,
    db: Db,
    policy: CredentialAuthority,
    flows: AuthFlows,
    jwt: JwtTokens,
    guard: SessionGuard,
}
impl Fixture {
    async fn new() -> Result<Self> {
        let url = crate::test_database_environment::database_url()
            .map_err(|_| Error::custom("test_database_url_required", "真实锁测试必须配置数据库"))?;
        let database = TestDatabase::create::<Migrator>(&url).await?;
        let outcome = Self::setup(&database).await;
        match outcome {
            Ok((db, policy, flows, jwt, guard)) => Ok(Self {
                database,
                db,
                policy,
                flows,
                jwt,
                guard,
            }),
            Err(error) => {
                database.close().await?;
                Err(error)
            }
        }
    }
    async fn setup(
        database: &TestDatabase,
    ) -> Result<(Db, CredentialAuthority, AuthFlows, JwtTokens, SessionGuard)> {
        let db = Db::default();
        db.initialize(database.connection()?)?;
        let key = STANDARD.encode([17_u8; 32]);
        let encrypter = Encrypter::new(EncryptionConfig::new(
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
            "owned-lock-expiry-password",
        )
        .await?;
        let now = crate::database::dialect::utc_value(
            db.connection()?.get_database_backend(),
            Utc::now(),
        );
        db.table("users")?
            .insert(vec![
                ("id", 42_i64.into()),
                ("email", "lock-expiry@example.test".into()),
                ("password_hash", hash.as_str().into()),
                ("created_at", now.clone()),
                ("updated_at", now),
            ])
            .await?;
        let model_db = db.clone();
        let model_encrypter = encrypter.clone();
        let provider = DatabaseUserProvider::new(db.clone(), passwords.clone(), "users")?
            .with_user_factory(move |attributes| {
                let id = attributes.get("id").cloned().ok_or(Error::Unauthorized)?;
                let store = DatabaseTwoFactorState::new(model_db.clone(), "users", "id", id)?;
                Ok(Arc::new(Account {
                    attributes: GenericUser::new(attributes),
                    authority: DatabaseFactorAuthority::new(store.clone(), model_encrypter.clone()),
                    store,
                }) as AuthUser)
            });
        check(provider.uses_database_accounts(&db, "users")?)?;
        let session = Session::new(None, Arc::new(MemorySessionStore::default()), None)?;
        session.start().await?;
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
        let credentials = Credentials::new(serde_json::Map::from_iter([
            ("email".into(), Value::from("lock-expiry@example.test")),
            ("password".into(), Value::from("owned-lock-expiry-password")),
        ]));
        check(guard.attempt(&credentials, false).await?)?;
        let policy = CredentialAuthority::new(db.clone(), encrypter, true);
        check(policy.account(42)?.load().await?.status == FactorAuthorityStatus::Disabled)?;
        let jwt = JwtTokens::new(
            JwtConfig::new(
                "owned-lock-issuer",
                "owned-lock-audience",
                "owned",
                BTreeMap::from([("owned".into(), key)]),
            )
            .with_ttl(5, 60),
        )?;
        let flows = AuthFlows::new(
            db.clone(),
            passwords.clone(),
            jwt.clone(),
            RefreshTokens::new(db.clone()),
            VerificationCodes::new(DatabaseVerificationCodeStore::new(db.clone()), passwords),
            RateLimiter::new(DatabaseRateLimitStore::new(db.clone())),
        )
        .with_factor_authority(policy.clone())?;
        Ok((db, policy, flows, jwt, guard))
    }
}
async fn counts(db: &Db) -> Result<(i64, i64)> {
    async fn count(db: &Db, table: &str) -> Result<i64> {
        let row: QueryResult = db
            .connection()?
            .query_one_raw(Statement::from_string(
                db.connection()?.get_database_backend(),
                format!(
                    "SELECT COUNT(*) AS amount FROM {}",
                    db.quoted_table_name(table)?
                ),
            ))
            .await
            .map_err(Error::internal)?
            .ok_or(Error::Unauthorized)?;
        row.try_get("", "amount").map_err(Error::internal)
    }
    Ok((
        count(db, "auth_refresh_tokens").await?,
        count(db, "personal_access_tokens").await?,
    ))
}
// 通知证明初检已成功；尚未完成+实际users排他锁证明等待点，不用延时模拟竞争。
async fn blocked<T: Send + 'static>(
    db: &Db,
    notify: Arc<Notify>,
    mut reader: tokio::task::JoinHandle<Result<T>>,
    writer: crate::database::sea_orm::DatabaseTransaction,
    hold: Duration,
    baseline: (i64, i64),
) -> Result<()> {
    let mut writer = Some(writer);
    let mut joined = false;
    let limit = Duration::from_secs(if hold >= Duration::from_secs(30) {
        45
    } else {
        15
    });
    let outcome: Result<()> = tokio::time::timeout(limit, async {
        tokio::time::timeout(Duration::from_secs(4), notify.notified())
            .await
            .map_err(|_| Error::custom("parent_precheck_timeout", "未观察到真实父凭据初检"))?;
        check(!reader.is_finished())?;
        tokio::time::sleep(hold).await;
        check(!reader.is_finished())?;
        writer
            .take()
            .ok_or(Error::Unauthorized)?
            .rollback()
            .await
            .map_err(Error::internal)?;
        let completed = tokio::time::timeout(Duration::from_secs(5), &mut reader)
            .await
            .map_err(|_| Error::custom("parent_lock_timeout", "释放行锁后读取未完成"))?;
        joined = true;
        check(matches!(
            completed.map_err(Error::internal)?,
            Err(Error::Unauthorized)
        ))?;
        check(counts(db).await? == baseline)
    })
    .await
    .map_err(|_| Error::custom("parent_expiry_timeout", "真实锁等待期限测试超过总时限"))
    .and_then(|result| result);
    if !joined {
        reader.abort();
    }
    if let Some(writer) = writer {
        let rollback = writer.rollback().await.map_err(Error::internal);
        if outcome.is_ok() {
            rollback?;
        }
    }
    if !joined {
        let _ = reader.await;
    }
    outcome
}
async fn writer(db: &Db) -> Result<crate::database::sea_orm::DatabaseTransaction> {
    let tx = db.connection()?.begin().await.map_err(Error::internal)?;
    tx.query_one_raw(Statement::from_string(
        tx.get_database_backend(),
        format!(
            "SELECT id FROM {} WHERE id=42 FOR UPDATE",
            db.quoted_table_name("users")?
        ),
    ))
    .await
    .map_err(Error::internal)?
    .ok_or(Error::Unauthorized)?;
    Ok(tx)
}
#[tokio::test]
#[ignore = "真实MySQL/PostgreSQL行锁，持锁31秒跨越真实票据有效期"]
async fn prepared_session_ticket_expires_while_waiting_for_real_user_lock() -> Result<()> {
    let fixture = Fixture::new().await?;
    let outcome: Result<()> = async {
        let ticket = fixture
            .policy
            .prepare_session(&fixture.guard, "web")
            .await?;
        check(
            ticket
                .context
                .as_ref()
                .is_some_and(|context| context.factor.is_none()),
        )?;
        let notify = Arc::new(Notify::new());
        let mut policy = fixture.policy.clone();
        policy.parent_prechecked = Some(notify.clone());
        let flows = fixture.flows.clone().with_factor_authority(policy)?;
        let baseline = counts(&fixture.db).await?;
        let writer = tokio::time::timeout(Duration::from_secs(4), writer(&fixture.db))
            .await
            .map_err(|_| Error::custom("writer_lock_timeout", "写事务未取得自有账户行锁"))??;
        let reader = tokio::spawn(async move { flows.issue_credentials_from(ticket).await });
        blocked(
            &fixture.db,
            notify,
            reader,
            writer,
            Duration::from_secs(31),
            baseline,
        )
        .await
    }
    .await;
    let cleanup = fixture.database.close().await;
    outcome?;
    cleanup
}
#[tokio::test]
#[ignore = "真实MySQL/PostgreSQL行锁，跨越实际签名JWT期限直接验证parent"]
async fn signed_jwt_parent_expires_while_waiting_for_real_user_lock() -> Result<()> {
    let fixture = Fixture::new().await?;
    let outcome: Result<()> = async {
        let issued = fixture
            .flows
            .issue_credentials_from_session(&fixture.guard, "web")
            .await?;
        let ticket = fixture
            .policy
            .authenticate_access_token(&fixture.jwt, issued.access_token())
            .await?;
        check(
            ticket
                .context
                .as_ref()
                .is_some_and(|context| context.factor.is_none()),
        )?;
        let deadline = ticket.expires_at.ok_or(Error::Unauthorized)?;
        let notify = Arc::new(Notify::new());
        let mut policy = fixture.policy.clone();
        policy.parent_prechecked = Some(notify.clone());
        let baseline = counts(&fixture.db).await?;
        let writer = tokio::time::timeout(Duration::from_secs(4), writer(&fixture.db))
            .await
            .map_err(|_| Error::custom("writer_lock_timeout", "写事务未取得自有账户行锁"))??;
        let db = fixture.db.clone();
        let reader = tokio::spawn(async move {
            db.transaction_for_update(move |tx| {
                Box::pin(async move { policy.validate_parent_in_transaction(tx, &ticket).await })
            })
            .await
        });
        let hold =
            (deadline - Utc::now()).to_std().map_err(Error::internal)? + Duration::from_millis(100);
        blocked(&fixture.db, notify, reader, writer, hold, baseline).await
    }
    .await;
    let cleanup = fixture.database.close().await;
    outcome?;
    cleanup
}

#[tokio::test]
#[ignore = "真实MySQL/PostgreSQL的直接登录挑战、凭据原子回滚与并发单次消费"]
async fn direct_challenge_outer_rollback_and_concurrent_completion_preserve_one_family()
-> Result<()> {
    let fixture = Fixture::new().await?;
    let outcome: Result<()> = Box::pin(async {
        let policy = CredentialAuthority::new(fixture.db.clone(), fixture.policy.encrypter.clone(), false);
        let user = fixture.guard.provider().retrieve_by_id(&serde_json::json!(42)).await?.ok_or(Error::Unauthorized)?;
        let mut builder = crate::foundation::ProviderBuilder::new();
        builder.provider(crate::events::EventsServiceProvider::new());
        let runtime = builder.start(figment::Figment::new()).await?;
        let events = (*runtime.container().resolve::<crate::events::Events>().await?).clone();
        let actions = crate::auth::fortify::TwoFactorActions::new(
            Arc::new(crate::auth::fortify::GoogleTwoFactorAuthenticationProvider::new(None)),
            fixture.policy.encrypter.clone(), events,
        );
        actions.enable(&user, false).await?;
        let snapshot = policy.account(42)?.load().await?;
        let secret = fixture.policy.encrypter.decrypt(snapshot.state.secret.as_ref().ok_or(Error::Unauthorized)?)?;
        let seed = crate::auth::fortify::engine::decode_secret(std::str::from_utf8(&secret).map_err(Error::internal)?)?;
        let step = u64::try_from(Utc::now().timestamp()).map_err(Error::internal)? / 30;
        let code = crate::auth::totp::totp_code(&seed, step, 6)?;
        fixture.db.table("users")?.where_eq("id", 42_i64)?.update(vec![("email_verified_at", crate::database::dialect::utc_value(fixture.db.backend()?, Utc::now()))]).await?;
        let flows = fixture.flows.clone().with_factor_authority(policy.clone())?.with_factor_actions(actions.clone());
        let challenge = match flows.begin_login("lock-expiry@example.test", "owned-lock-expiry-password", "owned-source").await? {
            crate::auth::LoginAttempt::TwoFactorRequired(challenge) => challenge,
            crate::auth::LoginAttempt::Authenticated(_) => return Err(Error::Unauthorized),
        };
        let prepared = policy.prepare_challenge(challenge.expose(), "owned-source").await?;
        let verified = prepared.verify(&actions, Some(&code), None).await?;
        let policy_for_transaction = policy.clone();
        let refresh = crate::auth::RefreshTokens::new(fixture.db.clone());
        let rolled: Result<()> = fixture.db.transaction_for_update(move |transaction| Box::pin(async move {
            let completed = policy_for_transaction.finish_challenge_in_transaction(transaction, prepared, verified).await?.ok_or(Error::Unauthorized)?;
            refresh.issue_context_in_transaction(transaction, 42, Utc::now() + chrono::Duration::hours(1), &completed.context).await?;
            Err(Error::custom("owned_after_refresh_rollback", "测试在真实凭据写入后回滚"))
        })).await;
        check(matches!(rolled, Err(error) if error.code() == "owned_after_refresh_rollback"))?;
        check(counts(&fixture.db).await? == (0, 0))?;
        check(policy.account(42)?.load().await? == snapshot)?;
        let (left, right) = tokio::join!(
            flows.complete_two_factor_login(challenge.expose(), Some(&code), None, "owned-source"),
            flows.complete_two_factor_login(challenge.expose(), Some(&code), None, "owned-source"),
        );
        let ((Ok(signed), Err(_)) | (Err(_), Ok(signed))) = (left, right) else {
            return Err(Error::custom("challenge_concurrency_contract", "同一挑战必须只产生一个成功家族"));
        };
        let credential = fixture.policy.authenticate_access_token(&fixture.jwt, signed.access_token()).await?;
        check(credential.user_id() == 42 && matches!(credential.factor_origin(), Some(crate::auth::FactorOrigin::DatabaseFlow { purpose, .. }) if purpose == "password"))?;
        check(counts(&fixture.db).await? == (1, 0))?;
        check(flows.complete_two_factor_login(challenge.expose(), Some(&code), None, "owned-source").await.is_err())?;
        fixture.policy.authenticate_access_token(&fixture.jwt, signed.access_token()).await?;
        drop(runtime);
        Ok(())
    }).await;
    let cleanup = fixture.database.close().await;
    outcome?;
    cleanup
}

#[tokio::test]
#[ignore = "真实MySQL/PostgreSQL验证实际Provider窗口、同码防重放与家族签发"]
async fn actual_provider_window_and_clock_bind_nonce_to_one_database_family() -> Result<()> {
    let url = crate::test_database_environment::database_url().map_err(Error::internal)?;
    let database = TestDatabase::create::<Migrator>(&url).await?;
    let result: Result<()> = Box::pin(async {
        let (db, policy, flows, jwt, guard) = Fixture::setup(&database).await?;
        let crypto = policy.encrypter.clone();
        let seed = b"0123456789abcdef0123";
        stage_window_factor(&policy).await?;
        let mut builder = crate::foundation::ProviderBuilder::new();
        builder.provider(crate::events::EventsServiceProvider::new());
        let runtime = builder.start(figment::Figment::new()).await?;
        let events = (*runtime
            .container()
            .resolve::<crate::events::Events>()
            .await?)
            .clone();
        let actions = crate::auth::fortify::TwoFactorActions::new(
            Arc::new(
                crate::auth::fortify::GoogleTwoFactorAuthenticationProvider::new(None)
                    .with_clock(Arc::new(|| Ok(3_000)))
                    .with_window(3)?,
            ),
            crypto,
            events,
        )
        .with_options(crate::auth::fortify::TwoFactorOptions {
            window: Some(0),
            ..Default::default()
        });
        // 确认使用同一真实Provider的当前步；随后直接登录用窗口内的另一时间步。
        let user = guard
            .provider()
            .retrieve_by_id(&Value::from(42))
            .await?
            .ok_or(Error::Unauthorized)?;
        let confirm_code = crate::auth::totp::totp_code(seed, 100, 6)?;
        actions.confirm(&user, &confirm_code).await?;
        check(policy.account(42)?.load().await?.status == FactorAuthorityStatus::Modern)?;
        let flows = flows.with_factor_actions(actions);
        db.table("users")?
            .where_eq("id", 42_i64)?
            .update(vec![(
                "email_verified_at",
                crate::database::dialect::utc_value(db.backend()?, Utc::now()),
            )])
            .await?;
        let code = crate::auth::totp::totp_code(seed, 102, 6)?;
        let challenge = match flows
            .begin_login(
                "lock-expiry@example.test",
                "owned-lock-expiry-password",
                "window-source",
            )
            .await?
        {
            crate::auth::LoginAttempt::TwoFactorRequired(value) => value,
            crate::auth::LoginAttempt::Authenticated(_) => return Err(Error::Unauthorized),
        };
        let before = Utc::now().timestamp();
        let signed = flows
            .complete_two_factor_login(challenge.expose(), Some(&code), None, "window-source")
            .await?;
        policy
            .authenticate_access_token(&jwt, signed.access_token())
            .await?;
        check(counts(&db).await? == (1, 0))?;
        let ledger = check_window_nonce_deadline(&db, &code, before).await?;
        let next = match flows
            .begin_login(
                "lock-expiry@example.test",
                "owned-lock-expiry-password",
                "window-replay-source",
            )
            .await?
        {
            crate::auth::LoginAttempt::TwoFactorRequired(value) => value,
            crate::auth::LoginAttempt::Authenticated(_) => return Err(Error::Unauthorized),
        };
        check(
            flows
                .complete_two_factor_login(next.expose(), Some(&code), None, "window-replay-source")
                .await
                .is_err(),
        )?;
        check(counts(&db).await? == (1, 0))?;
        check(check_window_nonce_deadline(&db, &code, before).await? == ledger)?;
        policy
            .authenticate_access_token(&jwt, signed.access_token())
            .await?;
        drop(runtime);
        Ok(())
    })
    .await;
    let cleanup = database.close().await;
    result?;
    cleanup
}

async fn stage_window_factor(policy: &CredentialAuthority) -> Result<()> {
    let authority = policy.account(42)?;
    let before = authority.load().await?;
    let pending = authority
        .replace(
            &before,
            &TwoFactorState {
                secret: Some(
                    policy
                        .encrypter
                        .encrypt(b"GAYTEMZUGU3DOOBZMFRGGZDFMYYDCMRT")?,
                ),
                recovery_codes: Some(policy.encrypter.encrypt(br#"["owned-recovery"]"#)?),
                confirmed_at: None,
            },
            crate::auth::fortify::FactorAuthorityMutation::Enable,
        )
        .await?;
    check(pending.status == FactorAuthorityStatus::Pending)?;
    Ok(())
}
async fn check_window_nonce_deadline(db: &Db, code: &str, before: i64) -> Result<String> {
    let encoded: String = db
        .table("users")?
        .where_eq("id", 42_i64)?
        .first()
        .await?
        .ok_or(Error::Unauthorized)?
        .try_get("", "two_factor_consumed_codes")
        .map_err(Error::internal)?;
    let ledger: Value = serde_json::from_str(&encoded).map_err(Error::internal)?;
    let expiry = ledger
        .get(format!("{:x}", sha2::Sha256::digest(code.as_bytes())))
        .and_then(Value::as_i64)
        .ok_or(Error::Unauthorized)?;
    check(expiry >= before + 270 && expiry <= Utc::now().timestamp() + 270)?;
    Ok(encoded)
}
