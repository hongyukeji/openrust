//! 真实SQLite的挑战尝试预算、事务外验证、原子因子消费与失败回滚。
use super::*;
use crate::auth::fortify::{
    DatabaseFactorAuthority, DatabaseTwoFactorState, FactorAuthorityMutation,
    GoogleTwoFactorAuthenticationProvider, TwoFactorAuthenticationProvider,
};
use crate::database::sea_orm_migration::MigrationTrait;
use crate::database::sea_orm_migration::MigratorTrait;
use crate::events::Events;
use crate::events::EventsServiceProvider;
use crate::foundation::ProviderBuilder;
use crate::support::BoxFuture;
use crate::testing::TestDatabase;
use serde_json::Value;

struct NoMigrations;
impl MigratorTrait for NoMigrations {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}
struct Fixture {
    db: Db,
    policy: CredentialAuthority,
    authority: DatabaseFactorAuthority,
    actions: TwoFactorActions,
    _runtime: crate::foundation::ProviderRuntime,
}
async fn owned<F>(callback: F) -> Result<()>
where
    F: FnOnce(Fixture) -> BoxFuture<'static, Result<()>>,
{
    let database = TestDatabase::sqlite_file::<NoMigrations>().await?;
    let db = Db::default();
    let outcome: Result<()> = async {
        db.initialize(database.connection()?)?;
        db.connection()?.execute_unprepared("CREATE TABLE users (id INTEGER PRIMARY KEY, disabled_at datetime, password_hash TEXT DEFAULT 'fixture-password', probe INTEGER NOT NULL DEFAULT 0, two_factor_secret TEXT, two_factor_recovery_codes TEXT, two_factor_confirmed_at datetime, two_factor_authority_source TEXT, two_factor_authority_generation TEXT, two_factor_consumed_codes TEXT, two_factor_management_session_hash TEXT, two_factor_management_generation TEXT, two_factor_management_authorized_at datetime)").await.map_err(Error::internal)?;
        db.connection()?.execute_unprepared("CREATE TABLE auth_factor_challenges (token_hash TEXT PRIMARY KEY, user_id INTEGER NOT NULL, source_hash TEXT NOT NULL, context TEXT NOT NULL, purpose TEXT NOT NULL, password_hash_digest TEXT, attempts INTEGER NOT NULL, expires_at datetime NOT NULL, created_at datetime NOT NULL, consumed_at datetime)").await.map_err(Error::internal)?;
        db.table("users")?.insert(vec![("id", 42_i64.into())]).await?;
        let authority = DatabaseFactorAuthority::new(DatabaseTwoFactorState::new(db.clone(), "users", "id", Value::from(42))?);
        let mut builder = ProviderBuilder::new(); builder.provider(EventsServiceProvider::new());
        let runtime = builder.start(figment::Figment::new()).await?;
        let events = (*runtime.container().resolve::<Events>().await?).clone();
        let actions = TwoFactorActions::new(Arc::new(GoogleTwoFactorAuthenticationProvider::new(None).with_clock(Arc::new(|| Ok(3_000)))), crate::auth::fortify::test_field_encrypter("KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=")?, events)
            .with_recovery_code_generator(Arc::new(|| Ok("replacement-recovery".into())));
        let policy = CredentialAuthority::new(db.clone(), false);
        callback(Fixture { db: db.clone(), policy, authority, actions, _runtime: runtime }).await
    }.await;
    drop(db);
    let closed = database.close().await;
    outcome?;
    closed
}
async fn enable(fixture: &Fixture) -> Result<()> {
    let before = fixture.authority.load().await?;
    let encrypter = fixture.actions.default_encrypter().await?;
    fixture
        .authority
        .replace(
            &before,
            &TwoFactorState {
                secret: Some(encrypter.encrypt_serialized_string(b"JBSWY3DPEHPK3PXP")?),
                recovery_codes: Some(encrypter.encrypt_serialized_string(
                    br#"["first","second","third","fourth","fifth","sixth","seventh","eighth"]"#,
                )?),
                confirmed_at: Some(Utc::now()),
            },
            FactorAuthorityMutation::Enable,
        )
        .await?;
    Ok(())
}
async fn begin(fixture: &Fixture) -> Result<CredentialLoginDecision> {
    let policy = fixture.policy.clone();
    fixture
        .db
        .transaction_for_update(move |tx| {
            Box::pin(async move {
                policy
                    .begin_login_in_transaction(tx, 42, "source-a", "password-login")
                    .await
            })
        })
        .await
}
async fn challenge(fixture: &Fixture) -> Result<TwoFactorLoginChallenge> {
    match begin(fixture).await? {
        CredentialLoginDecision::Challenge(challenge) => Ok(challenge),
        CredentialLoginDecision::Ready(_) => Err(Error::Unauthorized),
    }
}
async fn finish(
    fixture: &Fixture,
    prepared: PreparedCredentialChallenge,
    verified: CredentialChallengeVerification,
) -> Result<Option<CompletedCredentialChallenge>> {
    let policy = fixture.policy.clone();
    fixture
        .db
        .transaction_for_update(move |tx| {
            Box::pin(async move {
                policy
                    .finish_challenge_in_transaction(tx, prepared, verified)
                    .await
            })
        })
        .await
}
async fn attempts(fixture: &Fixture, raw: &str) -> Result<i64> {
    fixture
        .db
        .table("auth_factor_challenges")?
        .where_eq("token_hash", digest(raw))?
        .first()
        .await?
        .ok_or(Error::Unauthorized)?
        .try_get("", "attempts")
        .map_err(Error::internal)
}
#[tokio::test]
async fn disabled_account_is_ready_and_modern_success_preserves_real_database_flow_origin()
-> Result<()> {
    owned(|fixture| Box::pin(async move {
        match begin(&fixture).await? {
            CredentialLoginDecision::Ready(context) => { assert!(context.factor.is_none()); assert_eq!(context.identifier, 42); },
            CredentialLoginDecision::Challenge(_) => return Err(Error::Unauthorized),
        }
        assert!(fixture.db.table("auth_factor_challenges")?.get().await?.is_empty());
        enable(&fixture).await?;
        let challenge = challenge(&fixture).await?;
        let prepared = fixture.policy.prepare_challenge(challenge.expose(), "source-a").await?;
        assert_eq!(prepared.user_id(), 42);
        assert_eq!(prepared.expires_at(), challenge.expires_at());
        let verification = prepared.verify(&fixture.actions, Some("594318"), None).await?;
        let verified_at = verification.verified_at;
        let completed = finish(&fixture, prepared, verification).await?.ok_or(Error::Unauthorized)?;
        assert_eq!(completed.expires_at, challenge.expires_at());
        let factor = completed.context.factor.as_ref().ok_or(Error::Unauthorized)?;
        assert_eq!(factor.method, FactorVerificationMethod::Totp);
        assert_eq!(factor.verified_at, verified_at);
        assert!(matches!(&factor.origin, FactorOrigin::DatabaseFlow { source, purpose } if source == &completed.context.source && purpose == "password-login"));
        assert!(fixture.policy.prepare_challenge(challenge.expose(), "source-a").await.is_err());
        assert_eq!(attempts(&fixture, challenge.expose()).await?, 1);
        Ok(())
    })).await
}
#[tokio::test]
async fn wrong_source_and_malformed_raw_do_not_spend_attempts_but_five_real_failures_do()
-> Result<()> {
    owned(|fixture| {
        Box::pin(async move {
            enable(&fixture).await?;
            let challenge = challenge(&fixture).await?;
            assert!(
                fixture
                    .policy
                    .prepare_challenge("not-a-challenge", "source-a")
                    .await
                    .is_err()
            );
            assert!(
                fixture
                    .policy
                    .prepare_challenge(challenge.expose(), "source-b")
                    .await
                    .is_err()
            );
            assert_eq!(attempts(&fixture, challenge.expose()).await?, 0);
            for expected in 1..=5 {
                let prepared = fixture
                    .policy
                    .prepare_challenge(challenge.expose(), "source-a")
                    .await?;
                assert!(
                    prepared
                        .verify(&fixture.actions, Some("000000"), None)
                        .await
                        .is_err()
                );
                assert_eq!(attempts(&fixture, challenge.expose()).await?, expected);
            }
            assert!(
                fixture
                    .policy
                    .prepare_challenge(challenge.expose(), "source-a")
                    .await
                    .is_err()
            );
            Ok(())
        })
    })
    .await
}
#[tokio::test]
async fn prepared_verification_cannot_be_swapped_and_replay_finishes_only_once() -> Result<()> {
    owned(|fixture| {
        Box::pin(async move {
            enable(&fixture).await?;
            let challenge = challenge(&fixture).await?;
            let first = fixture
                .policy
                .prepare_challenge(challenge.expose(), "source-a")
                .await?;
            let second = fixture
                .policy
                .prepare_challenge(challenge.expose(), "source-a")
                .await?;
            let wrong_pair = first.verify(&fixture.actions, Some("594318"), None).await?;
            assert!(finish(&fixture, second, wrong_pair).await.is_err());
            let second = fixture
                .policy
                .prepare_challenge(challenge.expose(), "source-a")
                .await?;
            let first_verified = first.verify(&fixture.actions, Some("594318"), None).await?;
            let second_verified = second
                .verify(&fixture.actions, Some("594318"), None)
                .await?;
            let (left, right) = tokio::join!(
                finish(&fixture, first, first_verified),
                finish(&fixture, second, second_verified)
            );
            let left = left?;
            let right = right?;
            assert_eq!(
                usize::from(left.is_some()) + usize::from(right.is_some()),
                1
            );
            Ok(())
        })
    })
    .await
}
#[tokio::test]
async fn downstream_transaction_failure_rolls_back_factor_nonce_and_challenge_consumption()
-> Result<()> {
    owned(|fixture| {
        Box::pin(async move {
            enable(&fixture).await?;
            let challenge = challenge(&fixture).await?;
            let prepared = fixture
                .policy
                .prepare_challenge(challenge.expose(), "source-a")
                .await?;
            let verified = prepared
                .verify(&fixture.actions, Some("594318"), None)
                .await?;
            let policy = fixture.policy.clone();
            let failed: Result<()> = fixture
                .db
                .transaction_for_update(move |tx| {
                    Box::pin(async move {
                        policy
                            .finish_challenge_in_transaction(tx, prepared, verified)
                            .await?
                            .ok_or(Error::Unauthorized)?;
                        Err(Error::custom(
                            "downstream_signing_failed",
                            "测试后继签发失败",
                        ))
                    })
                })
                .await;
            assert!(
                failed
                    .err()
                    .is_some_and(|error| error.code() == "downstream_signing_failed")
            );
            assert_eq!(attempts(&fixture, challenge.expose()).await?, 1);
            let prepared = fixture
                .policy
                .prepare_challenge(challenge.expose(), "source-a")
                .await?;
            let verified = prepared
                .verify(&fixture.actions, Some("594318"), None)
                .await?;
            assert!(finish(&fixture, prepared, verified).await?.is_some());
            Ok(())
        })
    })
    .await
}
#[tokio::test]
async fn recovery_precedes_code_and_generation_change_after_verify_rejects_publication()
-> Result<()> {
    owned(|fixture| {
        Box::pin(async move {
            enable(&fixture).await?;
            let first = challenge(&fixture).await?;
            let prepared = fixture
                .policy
                .prepare_challenge(first.expose(), "source-a")
                .await?;
            let verified = prepared
                .verify(&fixture.actions, Some("000000"), Some("first"))
                .await?;
            let completed = finish(&fixture, prepared, verified)
                .await?
                .ok_or(Error::Unauthorized)?;
            assert!(
                completed
                    .context
                    .factor
                    .as_ref()
                    .is_some_and(|factor| factor.method == FactorVerificationMethod::RecoveryCode)
            );
            let state = fixture.authority.load().await?;
            let ciphertext = state
                .state
                .recovery_codes
                .as_ref()
                .ok_or(Error::Unauthorized)?;
            let decoded = fixture
                .actions
                .default_encrypter()
                .await?
                .decrypt_serialized_string(ciphertext)?;
            let codes: Vec<String> = serde_json::from_slice(&decoded).map_err(Error::internal)?;
            assert!(
                !codes.iter().any(|code| code == "first")
                    && codes.iter().any(|code| code == "replacement-recovery")
            );
            let second = challenge(&fixture).await?;
            let prepared = fixture
                .policy
                .prepare_challenge(second.expose(), "source-a")
                .await?;
            let verified = prepared
                .verify(&fixture.actions, Some("594318"), None)
                .await?;
            fixture
                .authority
                .replace(&state, &state.state, FactorAuthorityMutation::Enable)
                .await?;
            assert!(finish(&fixture, prepared, verified).await.is_err());
            Ok(())
        })
    })
    .await
}
struct LockProbe {
    db: Db,
    gate: Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>,
    inner: Arc<dyn TwoFactorAuthenticationProvider>,
}
impl TwoFactorAuthenticationProvider for LockProbe {
    fn generate_secret_key(&self, length: usize) -> Result<String> {
        self.inner.generate_secret_key(length)
    }
    fn qr_code_url(&self, company: &str, account: &str, secret: &str) -> Result<String> {
        self.inner.qr_code_url(company, account, secret)
    }
    fn verify_with_context<'a>(
        &'a self,
        secret: &'a str,
        code: &'a str,
    ) -> BoxFuture<'a, Result<crate::auth::fortify::TwoFactorCodeVerification>> {
        Box::pin(async move {
            let db = self.db.clone();
            self.db
                .transaction_for_update(move |tx| {
                    Box::pin(async move {
                        db.table("users")?
                            .using_transaction(tx)
                            .where_eq("id", 42_i64)?
                            .update(vec![("probe", 1_i64.into())])
                            .await?;
                        Ok(())
                    })
                })
                .await?;
            let fact = self.inner.verify_with_context(secret, code).await?;
            if let Some((entered, release)) = &self.gate {
                entered.notify_one();
                release.notified().await;
            }
            Ok(fact)
        })
    }
}
#[tokio::test]
async fn provider_can_await_its_own_user_write_after_prepare_commits() -> Result<()> {
    owned(|mut fixture| {
        Box::pin(async move {
            enable(&fixture).await?;
            let challenge = challenge(&fixture).await?;
            fixture.actions.provider = Arc::new(LockProbe {
                db: fixture.db.clone(),
                gate: None,
                inner: fixture.actions.provider.clone(),
            });
            let prepared = fixture
                .policy
                .prepare_challenge(challenge.expose(), "source-a")
                .await?;
            let verified = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                prepared.verify(&fixture.actions, Some("594318"), None),
            )
            .await
            .map_err(Error::internal)??;
            assert!(finish(&fixture, prepared, verified).await?.is_some());
            let row = fixture
                .db
                .table("users")?
                .where_eq("id", 42_i64)?
                .first()
                .await?
                .ok_or(Error::Unauthorized)?;
            assert_eq!(row.try_get::<i64>("", "probe").map_err(Error::internal)?, 1);
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn expired_or_corrupt_challenge_and_disabled_account_cannot_publish_factor_context()
-> Result<()> {
    owned(|fixture| {
        Box::pin(async move {
            enable(&fixture).await?;
            let expired = challenge(&fixture).await?;
            let now = Utc::now().trunc_subsecs(6);
            fixture
                .db
                .table("auth_factor_challenges")?
                .where_eq("token_hash", digest(expired.expose()))?
                .update(vec![
                    (
                        "created_at",
                        crate::database::dialect::utc_value(
                            DbBackend::Sqlite,
                            now - chrono::Duration::seconds(301),
                        ),
                    ),
                    (
                        "expires_at",
                        crate::database::dialect::utc_value(
                            DbBackend::Sqlite,
                            now - chrono::Duration::seconds(1),
                        ),
                    ),
                ])
                .await?;
            assert!(
                fixture
                    .policy
                    .prepare_challenge(expired.expose(), "source-a")
                    .await
                    .is_err()
            );
            assert_eq!(attempts(&fixture, expired.expose()).await?, 0);
            let corrupt = challenge(&fixture).await?;
            fixture
                .db
                .table("auth_factor_challenges")?
                .where_eq("token_hash", digest(corrupt.expose()))?
                .update(vec![("context", "{}".into())])
                .await?;
            assert!(
                fixture
                    .policy
                    .prepare_challenge(corrupt.expose(), "source-a")
                    .await
                    .is_err()
            );
            let active = challenge(&fixture).await?;
            let prepared = fixture
                .policy
                .prepare_challenge(active.expose(), "source-a")
                .await?;
            let verified = prepared
                .verify(&fixture.actions, Some("594318"), None)
                .await?;
            fixture
                .db
                .table("users")?
                .where_eq("id", 42_i64)?
                .update(vec![(
                    "disabled_at",
                    crate::database::dialect::utc_value(DbBackend::Sqlite, Utc::now()),
                )])
                .await?;
            assert!(finish(&fixture, prepared, verified).await.is_err());
            let row = fixture
                .db
                .table("auth_factor_challenges")?
                .where_eq("token_hash", digest(active.expose()))?
                .first()
                .await?
                .ok_or(Error::Unauthorized)?;
            assert!(
                row.try_get::<Option<DateTime<Utc>>>("", "consumed_at")
                    .map_err(Error::internal)?
                    .is_none()
            );
            Ok(())
        })
    })
    .await
}

struct CountProvider {
    fail: bool,
    inner: Arc<dyn TwoFactorAuthenticationProvider>,
    calls: Arc<std::sync::atomic::AtomicUsize>,
}
impl TwoFactorAuthenticationProvider for CountProvider {
    fn generate_secret_key(&self, length: usize) -> Result<String> {
        self.inner.generate_secret_key(length)
    }
    fn qr_code_url(&self, company: &str, account: &str, secret: &str) -> Result<String> {
        self.inner.qr_code_url(company, account, secret)
    }
    fn verify_with_context<'a>(
        &'a self,
        secret: &'a str,
        code: &'a str,
    ) -> BoxFuture<'a, Result<crate::auth::fortify::TwoFactorCodeVerification>> {
        Box::pin(async move {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.fail {
                return Err(Error::custom("provider_context_failed", "测试验证器失败"));
            }
            self.inner.verify_with_context(secret, code).await
        })
    }
}
#[tokio::test]
async fn direct_verification_uses_actual_window_clock_and_nonce_ttl_not_actions_options()
-> Result<()> {
    owned(|mut fixture| {
        Box::pin(async move {
            enable(&fixture).await?;
            fixture.actions.provider = Arc::new(
                GoogleTwoFactorAuthenticationProvider::new(None)
                    .with_clock(Arc::new(|| Ok(3_000)))
                    .with_window(3)?,
            );
            let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            fixture.actions.provider = Arc::new(CountProvider {
                fail: false,
                inner: fixture.actions.provider.clone(),
                calls: calls.clone(),
            });
            fixture.actions.options.window = Some(0);
            let secret = crate::auth::fortify::engine::decode_secret("JBSWY3DPEHPK3PXP")?;
            let code = crate::auth::totp::totp_code(&secret, 102, 6)?;
            let challenge = challenge(&fixture).await?;
            let prepared = fixture
                .policy
                .prepare_challenge(challenge.expose(), "source-a")
                .await?;
            let verified = prepared.verify(&fixture.actions, Some(&code), None).await?;
            match &verified.candidate {
                VerificationCandidate::ModernTotp {
                    verification: fact, ..
                } => {
                    assert_eq!(fact.window(), Some(3));
                    assert_eq!(fact.current_counter(), Some(100));
                    assert_eq!(fact.matched_counter(), Some(102));
                }
                VerificationCandidate::ModernRecovery(_) => return Err(Error::Unauthorized),
            }
            let before = Utc::now().timestamp();
            assert!(finish(&fixture, prepared, verified).await?.is_some());
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
            let row = fixture
                .db
                .table("users")?
                .where_eq("id", 42_i64)?
                .first()
                .await?
                .ok_or(Error::Unauthorized)?;
            let encoded: String = row
                .try_get("", "two_factor_consumed_codes")
                .map_err(Error::internal)?;
            let ledger: serde_json::Value =
                serde_json::from_str(&encoded).map_err(Error::internal)?;
            let expires = ledger
                .get(digest(&code))
                .and_then(Value::as_i64)
                .ok_or(Error::Unauthorized)?;
            assert!(expires >= before + 270);
            assert!(expires <= Utc::now().timestamp() + 270);
            let replay = fixture
                .policy
                .prepare_challenge(challenge.expose(), "source-a")
                .await;
            assert!(replay.is_err());
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn direct_context_error_preserves_recovery_priority_without_second_provider_call()
-> Result<()> {
    owned(|mut fixture| {
        Box::pin(async move {
            enable(&fixture).await?;
            let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            fixture.actions.provider = Arc::new(CountProvider {
                fail: true,
                inner: fixture.actions.provider.clone(),
                calls: calls.clone(),
            });
            let challenge = challenge(&fixture).await?;
            let prepared = fixture
                .policy
                .prepare_challenge(challenge.expose(), "source-a")
                .await?;
            let error = prepared
                .verify(&fixture.actions, Some("594318"), None)
                .await
                .err()
                .ok_or(Error::Unauthorized)?;
            assert_eq!(error.code(), "provider_context_failed");
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
            let prepared = fixture
                .policy
                .prepare_challenge(challenge.expose(), "source-a")
                .await?;
            let verified = prepared
                .verify(&fixture.actions, Some("594318"), Some("first"))
                .await?;
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
            assert!(finish(&fixture, prepared, verified).await?.is_some());
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn cached_distinct_counter_cannot_reuse_the_same_consumed_code() -> Result<()> {
    owned(|mut fixture| {
        Box::pin(async move {
            let before = fixture.authority.load().await?;
            let crypto = fixture.actions.default_encrypter().await?;
            fixture
                .authority
                .replace(
                    &before,
                    &TwoFactorState {
                        secret: Some(
                            crypto
                                .encrypt_serialized_string(b"GAYTEMZUGU3DOOBZMFRGGZDFMYYDCMRT")?,
                        ),
                        recovery_codes: Some(crypto.encrypt_serialized_string(br#"["unused"]"#)?),
                        confirmed_at: Some(Utc::now()),
                    },
                    FactorAuthorityMutation::Enable,
                )
                .await?;
            cached_collision(&mut fixture).await
        })
    })
    .await
}
async fn cached_collision(fixture: &mut Fixture) -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder.provider(crate::cache::CacheServiceProvider::new());
    let runtime = builder.start(figment::Figment::new().merge(figment::providers::Serialized::defaults(serde_json::json!({"cache":{"default":"collision","stores":{"collision":{"driver":"memory","namespace":""}}}})))).await?;
    let outcome: Result<()> = async {
        let cache = (*runtime.container().resolve::<crate::cache::Cache>().await?).clone();
        let key = "fortify.2fa_codes.adebe071e34ad25865219d4a1e3931d5";
        fixture.actions.provider = Arc::new(
            GoogleTwoFactorAuthenticationProvider::new(Some(cache.clone()))
                .with_clock(Arc::new(|| Ok(225_818 * 30)))
                .with_window(0)?,
        );
        let first = challenge(fixture).await?;
        let first = fixture
            .policy
            .prepare_challenge(first.expose(), "source-a")
            .await?;
        let verified = first.verify(&fixture.actions, Some("629723"), None).await?;
        assert!(finish(fixture, first, verified).await?.is_some());
        assert_eq!(cache.get::<i64>(key).await?, Some(225_818));
        let committed_ledger = nonce_ledger(fixture).await?.ok_or(Error::Unauthorized)?;
        fixture.actions.provider = Arc::new(
            GoogleTwoFactorAuthenticationProvider::new(Some(cache.clone()))
                .with_clock(Arc::new(|| Ok(225_820 * 30)))
                .with_window(3)?,
        );
        fixture.actions.options.window = Some(0);
        let seed = b"0123456789abcdef0123";
        assert_eq!(crate::auth::totp::totp_code(seed, 225_818, 6)?, "629723");
        assert_eq!(crate::auth::totp::totp_code(seed, 225_823, 6)?, "629723");
        for counter in 225_819..=225_822 {
            assert_ne!(crate::auth::totp::totp_code(seed, counter, 6)?, "629723");
        }
        let collision = self::challenge(fixture).await?;
        let prepared = fixture
            .policy
            .prepare_challenge(collision.expose(), "source-a")
            .await?;
        let verified = prepared
            .verify(&fixture.actions, Some("629723"), None)
            .await?;
        match &verified.candidate {
            VerificationCandidate::ModernTotp {
                verification: fact, ..
            } => {
                assert!(fact.is_valid());
                assert_eq!(fact.current_counter(), Some(225_820));
                assert_eq!(fact.matched_counter(), Some(225_823));
                assert_eq!(fact.window(), Some(3));
            }
            VerificationCandidate::ModernRecovery(_) => return Err(Error::Unauthorized),
        }
        assert_eq!(cache.get::<i64>(key).await?, Some(225_823));
        assert!(matches!(
            finish(fixture, prepared, verified).await,
            Err(Error::Unauthorized)
        ));
        assert_eq!(
            nonce_ledger(fixture).await?.as_deref(),
            Some(committed_ledger.as_str())
        );
        assert_unconsumed_attempt(fixture, collision.expose()).await?;
        Ok(())
    }
    .await;
    let shutdown = runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await;
    outcome?;
    shutdown?;
    Ok(())
}

async fn nonce_ledger(fixture: &Fixture) -> Result<Option<String>> {
    fixture
        .db
        .table("users")?
        .where_eq("id", 42_i64)?
        .first()
        .await?
        .ok_or(Error::Unauthorized)?
        .try_get("", "two_factor_consumed_codes")
        .map_err(Error::internal)
}
async fn assert_unconsumed_attempt(fixture: &Fixture, raw: &str) -> Result<()> {
    let row = fixture
        .db
        .table("auth_factor_challenges")?
        .where_eq("token_hash", digest(raw))?
        .first()
        .await?
        .ok_or(Error::Unauthorized)?;
    assert!(
        row.try_get::<Option<DateTime<Utc>>>("", "consumed_at")
            .map_err(Error::internal)?
            .is_none()
    );
    assert_eq!(
        row.try_get::<i64>("", "attempts")
            .map_err(Error::internal)?,
        1
    );
    Ok(())
}

#[tokio::test]
async fn cancellation_and_expiry_while_provider_waits_cannot_consume_the_factor() -> Result<()> {
    owned(|mut fixture| {
        Box::pin(async move {
            enable(&fixture).await?;
            let inner = fixture.actions.provider.clone();
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            fixture.actions.provider = Arc::new(LockProbe {
                db: fixture.db.clone(),
                inner: inner.clone(),
                gate: Some((entered.clone(), release.clone())),
            });
            let cancelled = challenge(&fixture).await?;
            let first = fixture
                .policy
                .prepare_challenge(cancelled.expose(), "source-a")
                .await?;
            {
                let mut verifying = Box::pin(first.verify(&fixture.actions, Some("594318"), None));
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    tokio::select! {
                        () = entered.notified() => Ok(()),
                        _ = &mut verifying => Err(Error::Unauthorized),
                    }
                })
                .await
                .map_err(Error::internal)??;
                // 实际Provider已经得到正确验证码；取消待返回的future，挑战只花掉一次尝试。
                drop(verifying);
            }
            assert_unconsumed_attempt(&fixture, cancelled.expose()).await?;
            assert!(nonce_ledger(&fixture).await?.is_none());

            let expiring = challenge(&fixture).await?;
            let deadline = Utc::now().trunc_subsecs(6) + chrono::Duration::seconds(2);
            fixture
                .db
                .table("auth_factor_challenges")?
                .where_eq("token_hash", digest(expiring.expose()))?
                .update(vec![(
                    "expires_at",
                    crate::database::dialect::utc_value(DbBackend::Sqlite, deadline),
                )])
                .await?;
            let second = fixture
                .policy
                .prepare_challenge(expiring.expose(), "source-a")
                .await?;
            assert_eq!(second.expires_at(), deadline);
            let delayed = async {
                entered.notified().await;
                let wait = (deadline - Utc::now()).to_std().unwrap_or_default()
                    + std::time::Duration::from_millis(10);
                tokio::time::sleep(wait).await;
                release.notify_one();
            };
            let (verified, ()) = tokio::time::timeout(std::time::Duration::from_secs(6), async {
                tokio::join!(
                    second.verify(&fixture.actions, Some("594318"), None),
                    delayed
                )
            })
            .await
            .map_err(Error::internal)?;
            assert!(matches!(verified, Err(Error::Unauthorized)));
            assert_unconsumed_attempt(&fixture, expiring.expose()).await?;
            assert!(nonce_ledger(&fixture).await?.is_none());
            // 只有仍在有效期内的原取消挑战可以重新验证并完成。
            fixture.actions.provider = inner;
            let retry = fixture
                .policy
                .prepare_challenge(cancelled.expose(), "source-a")
                .await?;
            let verified = retry.verify(&fixture.actions, Some("594318"), None).await?;
            assert!(finish(&fixture, retry, verified).await?.is_some());
            assert_eq!(attempts(&fixture, cancelled.expose()).await?, 2);
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn challenge_scope_is_checked_before_consuming_an_attempt() -> Result<()> {
    owned(|fixture| {
        Box::pin(async move {
            use crate::auth::credential::CredentialGuardBinding;
            enable(&fixture).await?;
            let scope = CredentialGuardBinding {
                guard: "api".into(),
                provider: "users".into(),
                issuer: "test-issuer".into(),
                audience: "test-client".into(),
            };
            let policy = fixture.policy.clone();
            let selected = scope.clone();
            let decision = fixture
                .db
                .transaction_for_update(move |tx| {
                    Box::pin(async move {
                        policy
                            .begin_login_in_scope_in_transaction(
                                tx,
                                42,
                                "source-a",
                                "jwt-api-password",
                                Some(selected),
                            )
                            .await
                    })
                })
                .await?;
            let CredentialLoginDecision::Challenge(challenge) = decision else {
                return Err(Error::Unauthorized);
            };
            for wrong in [
                None,
                Some(CredentialGuardBinding {
                    guard: "staff".into(),
                    ..scope.clone()
                }),
                Some(CredentialGuardBinding {
                    provider: "operators".into(),
                    ..scope.clone()
                }),
                Some(CredentialGuardBinding {
                    issuer: "other-issuer".into(),
                    ..scope.clone()
                }),
                Some(CredentialGuardBinding {
                    audience: "other-client".into(),
                    ..scope.clone()
                }),
            ] {
                assert!(matches!(
                    fixture
                        .policy
                        .prepare_challenge_in_scope(challenge.expose(), "source-a", wrong.as_ref())
                        .await,
                    Err(Error::Unauthorized)
                ));
                assert_eq!(attempts(&fixture, challenge.expose()).await?, 0);
            }
            let prepared = fixture
                .policy
                .prepare_challenge_in_scope(challenge.expose(), "source-a", Some(&scope))
                .await?;
            let verification = prepared
                .verify(&fixture.actions, Some("594318"), None)
                .await?;
            assert!(finish(&fixture, prepared, verification).await?.is_some());
            Ok(())
        })
    })
    .await
}

struct OneSecondTotp;
impl TwoFactorAuthenticationProvider for OneSecondTotp {
    fn generate_secret_key(&self, _: usize) -> Result<String> {
        Err(Error::Unauthorized)
    }
    fn qr_code_url(&self, _: &str, _: &str, _: &str) -> Result<String> {
        Err(Error::Unauthorized)
    }
    fn verify_with_context<'a>(
        &'a self,
        secret: &'a str,
        code: &'a str,
    ) -> BoxFuture<'a, Result<TwoFactorCodeVerification>> {
        Box::pin(async move {
            let decoded = crate::auth::fortify::engine::decode_secret(secret)?;
            if crate::auth::totp::totp_code(&decoded, 100, 6)? != code {
                return Err(Error::Unauthorized);
            }
            TwoFactorCodeVerification::verified_with_period(code, 0, 100, 100, 1)
        })
    }
}
#[tokio::test]
async fn late_independent_challenge_cannot_reuse_verified_totp_after_nonce_expiry() -> Result<()> {
    owned(|mut fixture| {
        Box::pin(async move {
            enable(&fixture).await?;
            fixture.actions.provider = Arc::new(OneSecondTotp);
            let a = challenge(&fixture).await?;
            let b = challenge(&fixture).await?;
            let first = fixture
                .policy
                .prepare_challenge(a.expose(), "source-a")
                .await?;
            let second = fixture
                .policy
                .prepare_challenge(b.expose(), "source-a")
                .await?;
            let proof_a = first.verify(&fixture.actions, Some("594318"), None).await?;
            let proof_b = second
                .verify(&fixture.actions, Some("594318"), None)
                .await?;
            assert!(finish(&fixture, first, proof_a).await?.is_some());
            // 实际周期 1 秒、window=0，对应3秒 nonce 留存。让第二份已验证事实晚于该期限。
            tokio::time::sleep(std::time::Duration::from_millis(3_100)).await;
            assert!(b.expires_at() > Utc::now());
            assert!(matches!(
                finish(&fixture, second, proof_b).await,
                Err(Error::Unauthorized)
            ));
            assert_unconsumed_attempt(&fixture, b.expose()).await?;
            Ok(())
        })
    })
    .await
}

mod flow_commit {
    use super::*;
    use crate::{
        audit::{Audit, AuditPolicy, DatabaseAuditMigration, DatabaseAuditSink},
        auth::{
            AuthFlows, AuthMigration, AuthRefreshTokenMigration, AuthVerificationCodeMigration,
            DatabaseVerificationCodeStore, JwtTokens, LoginAttempt, RefreshTokens,
            VerificationCodes,
            fortify::{
                CredentialChallengeMigration, CredentialFactorMigration, DatabaseApiLoginAudit,
                FactorAuthorityMigration, FortifyTwoFactorMigration,
            },
        },
        config::JwtConfig,
        core::RateLimiter,
        database::{
            DatabaseRateLimitMigration, DatabaseRateLimitStore, DatabaseServiceProvider,
            TransactionBeforeCommit,
        },
    };
    use std::sync::Mutex;

    const EMAIL: &str = "checked-login@example.test";
    const PASSWORD: &str = "current-correct-password";
    const SOURCE: &str = "source-a";
    struct Migrations;
    impl MigratorTrait for Migrations {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            vec![
                Box::new(AuthMigration),
                Box::new(AuthRefreshTokenMigration),
                Box::new(AuthVerificationCodeMigration),
                Box::new(DatabaseRateLimitMigration),
                Box::new(DatabaseAuditMigration),
                Box::new(FortifyTwoFactorMigration::new()),
                Box::new(FactorAuthorityMigration::new()),
                Box::new(CredentialFactorMigration),
                Box::new(CredentialChallengeMigration),
                #[cfg(feature = "auth-oidc")]
                Box::new(crate::auth::AuthExternalSessionMigration),
            ]
        }
    }

    async fn application<F>(listeners: EventsServiceProvider, callback: F) -> Result<()>
    where
        F: FnOnce(Fixture, AuthFlows) -> BoxFuture<'static, Result<()>>,
    {
        let database = TestDatabase::sqlite_file::<Migrations>().await?;
        let mut builder = ProviderBuilder::new();
        builder.provider(DatabaseServiceProvider);
        builder.provider(listeners);
        let runtime = builder.start(figment::Figment::new()).await?;
        let result = async {
            let db = (*runtime.container().resolve::<Db>().await?).clone();
            db.initialize(database.connection()?)?;
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
            let hash = crate::auth::EncodedPasswordHash::make(passwords.as_ref(), PASSWORD).await?;
            let now = crate::database::dialect::utc_value(DbBackend::Sqlite, Utc::now());
            db.table("users")?
                .insert(vec![
                    ("id", 42_i64.into()),
                    ("email", EMAIL.into()),
                    ("password_hash", hash.as_str().into()),
                    ("email_verified_at", now.clone()),
                    ("created_at", now.clone()),
                    ("updated_at", now),
                ])
                .await?;
            let keys = std::collections::BTreeMap::from([(
                "test".into(),
                "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=".into(),
            )]);
            let jwt = JwtTokens::new(JwtConfig::new("issuer", "consumer", "test", keys))?;
            let authority = DatabaseFactorAuthority::new(DatabaseTwoFactorState::new(
                db.clone(),
                "users",
                "id",
                Value::from(42),
            )?);
            let actions = TwoFactorActions::new(
                Arc::new(OneSecondTotp),
                crate::auth::fortify::test_field_encrypter(
                    "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=",
                )?,
                (*runtime.container().resolve::<Events>().await?).clone(),
            );
            let policy = CredentialAuthority::new(db.clone(), false);
            let audit = DatabaseApiLoginAudit::new(
                db.clone(),
                Audit::new(DatabaseAuditSink::new(db.clone()), AuditPolicy::default()),
            );
            let flow = AuthFlows::new(
                db.clone(),
                passwords.clone(),
                jwt,
                RefreshTokens::new(db.clone()),
                VerificationCodes::new(DatabaseVerificationCodeStore::new(db.clone()), passwords),
                RateLimiter::new(DatabaseRateLimitStore::new(db.clone())),
            )
            .with_factor_authority(policy.clone())?
            .with_factor_actions(actions.clone())
            .with_api_login_audit(Arc::new(audit))?;
            callback(
                Fixture {
                    db,
                    policy,
                    authority,
                    actions,
                    _runtime: runtime,
                },
                flow,
            )
            .await
        }
        .await;
        let cleanup = database.close().await;
        result?;
        cleanup
    }

    async fn login_challenge(flow: &AuthFlows) -> Result<TwoFactorLoginChallenge> {
        match flow.begin_login(EMAIL, PASSWORD, SOURCE).await? {
            LoginAttempt::TwoFactorRequired(challenge) => Ok(challenge),
            LoginAttempt::Authenticated(_) => Err(Error::Unauthorized),
        }
    }

    #[tokio::test]
    async fn completed_password_reset_after_verification_invalidates_prepared_password_challenge()
    -> Result<()> {
        application(EventsServiceProvider::new(), |fixture, flow| {
            Box::pin(async move {
                enable(&fixture).await?;
                let challenge = login_challenge(&flow).await?;
                let prepared = fixture
                    .policy
                    .prepare_challenge(challenge.expose(), SOURCE)
                    .await?;
                let verified = prepared
                    .verify(&fixture.actions, None, Some("first"))
                    .await?;
                let reset = flow
                    .request_password_reset(EMAIL, SOURCE)
                    .await?
                    .ok_or(Error::Unauthorized)?;
                assert!(
                    flow.reset_password(EMAIL, reset.code(), "replacement-reset-password", SOURCE)
                        .await?
                );
                assert!(matches!(
                    finish(&fixture, prepared, verified).await,
                    Err(Error::Unauthorized)
                ));
                assert_unconsumed_attempt(&fixture, challenge.expose()).await?;
                assert!(
                    fixture
                        .policy
                        .prepare_challenge(challenge.expose(), SOURCE)
                        .await
                        .is_err()
                );
                assert_eq!(fixture.db.table("auth_refresh_tokens")?.count().await?, 0);
                Ok(())
            })
        })
        .await
    }

    #[derive(Clone, Copy)]
    enum Hook {
        ChangeReadyPassword,
        ChangeChallengePassword,
        ChangeCompletedPassword,
        DelayTotp,
        ExpireChallenge,
        RevokeRefresh,
    }
    #[derive(Clone, Default)]
    struct BeforeCommit(Arc<Mutex<Option<Hook>>>);
    impl BeforeCommit {
        fn set(&self, hook: Option<Hook>) {
            *self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = hook;
        }
        fn selected(&self) -> Option<Hook> {
            *self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }
        fn listeners(&self) -> EventsServiceProvider {
            let hook = self.clone();
            let mut listeners = EventsServiceProvider::new();
            listeners.listen::<TransactionBeforeCommit, _>(move |_, ctx| {
                let hook = hook.clone();
                Box::pin(async move {
                    if let Some(selected) = hook.selected() {
                        hook.interfere(ctx.transaction().ok_or(Error::Unauthorized)?, selected)
                            .await?;
                    }
                    Ok(())
                })
            });
            listeners
        }
        async fn interfere(&self, tx: &DatabaseTransaction, selected: Hook) -> Result<()> {
            use crate::database::sea_orm::Statement;
            let candidate = match selected {
                Hook::RevokeRefresh => {
                    "SELECT 1 FROM auth_refresh_tokens WHERE consumed_at IS NOT NULL"
                }
                Hook::ChangeChallengePassword => "SELECT 1 FROM auth_factor_challenges",
                Hook::ChangeReadyPassword => "SELECT 1 FROM auth_refresh_tokens",
                _ => "SELECT 1 FROM audit_records WHERE action='auth.two_factor.login'",
            };
            if tx
                .query_one_raw(Statement::from_string(DbBackend::Sqlite, candidate))
                .await
                .map_err(Error::internal)?
                .is_none()
            {
                return Ok(());
            }
            self.set(None);
            match selected {
                Hook::ChangeReadyPassword
                | Hook::ChangeChallengePassword
                | Hook::ChangeCompletedPassword => {
                    tx.execute_unprepared(
                        "UPDATE users SET password_hash='late-hash-change' WHERE id=42",
                    )
                    .await
                    .map_err(Error::internal)?;
                }
                Hook::DelayTotp => {
                    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
                }
                Hook::ExpireChallenge => {
                    let row = tx.query_one_raw(Statement::from_string(DbBackend::Sqlite, "SELECT expires_at FROM auth_factor_challenges WHERE consumed_at IS NOT NULL"))
                        .await.map_err(Error::internal)?.ok_or(Error::Unauthorized)?;
                    let expiry: DateTime<Utc> =
                        row.try_get("", "expires_at").map_err(Error::internal)?;
                    tokio::time::sleep(
                        (expiry - Utc::now()).to_std().unwrap_or_default()
                            + std::time::Duration::from_millis(10),
                    )
                    .await;
                }
                Hook::RevokeRefresh => {
                    tx.execute_unprepared("UPDATE auth_refresh_tokens SET revoked_at=CURRENT_TIMESTAMP WHERE consumed_at IS NULL").await.map_err(Error::internal)?;
                }
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn final_hook_rejects_changed_password_or_late_factor_after_audit_and_rolls_back_issuance()
    -> Result<()> {
        let mode = BeforeCommit::default();
        application(mode.listeners(), move |fixture, flow| {
            Box::pin(async move {
                mode.set(Some(Hook::ChangeReadyPassword));
                assert!(matches!(
                    flow.begin_login(EMAIL, PASSWORD, SOURCE).await,
                    Err(Error::Unauthorized)
                ));
                assert_eq!(fixture.db.table("auth_refresh_tokens")?.count().await?, 0);
                assert!(mode.selected().is_none());
                enable(&fixture).await?;
                mode.set(Some(Hook::ChangeChallengePassword));
                assert!(matches!(
                    flow.begin_login(EMAIL, PASSWORD, SOURCE).await,
                    Err(Error::Unauthorized)
                ));
                assert_eq!(
                    fixture.db.table("auth_factor_challenges")?.count().await?,
                    0
                );
                assert!(mode.selected().is_none());
                let challenge = login_challenge(&flow).await?;
                for selected in [Hook::ChangeCompletedPassword, Hook::DelayTotp] {
                    mode.set(Some(selected));
                    assert!(matches!(
                        flow.complete_two_factor_login(
                            challenge.expose(),
                            Some("594318"),
                            None,
                            SOURCE
                        )
                        .await,
                        Err(Error::Unauthorized)
                    ));
                    assert!(mode.selected().is_none());
                    assert_eq!(fixture.db.table("auth_refresh_tokens")?.count().await?, 0);
                    assert_eq!(fixture.db.table("audit_records")?.count().await?, 0);
                    assert!(nonce_ledger(&fixture).await?.is_none());
                    let row = fixture
                        .db
                        .table("auth_factor_challenges")?
                        .where_eq("token_hash", digest(challenge.expose()))?
                        .first()
                        .await?
                        .ok_or(Error::Unauthorized)?;
                    assert!(
                        row.try_get::<Option<DateTime<Utc>>>("", "consumed_at")
                            .map_err(Error::internal)?
                            .is_none()
                    );
                }
                let issued = flow
                    .complete_two_factor_login(challenge.expose(), Some("594318"), None, SOURCE)
                    .await?;
                assert_eq!(fixture.db.table("audit_records")?.count().await?, 1);
                mode.set(Some(Hook::RevokeRefresh));
                assert!(matches!(
                    flow.refresh(issued.refresh_token().expose(), SOURCE).await,
                    Err(Error::Unauthorized)
                ));
                assert!(mode.selected().is_none());
                assert_eq!(fixture.db.table("auth_refresh_tokens")?.count().await?, 1);
                assert!(
                    flow.refresh(issued.refresh_token().expose(), SOURCE)
                        .await
                        .is_ok()
                );
                Ok(())
            })
        })
        .await
    }
    #[tokio::test]
    async fn recovery_login_waiting_past_challenge_expiry_rolls_back_every_candidate() -> Result<()>
    {
        let hook = BeforeCommit::default();
        application(hook.listeners(), move |fixture, flow| {
            Box::pin(async move {
                enable(&fixture).await?;
                let challenge = login_challenge(&flow).await?;
                let cutoff = Utc::now() + chrono::Duration::seconds(2);
                fixture
                    .db
                    .table("auth_factor_challenges")?
                    .where_eq("token_hash", digest(challenge.expose()))?
                    .update(vec![(
                        "expires_at",
                        crate::database::dialect::utc_value(DbBackend::Sqlite, cutoff),
                    )])
                    .await?;
                let before = fixture.authority.load().await?;
                hook.set(Some(Hook::ExpireChallenge));
                assert!(matches!(
                    flow.complete_two_factor_login(challenge.expose(), None, Some("first"), SOURCE)
                        .await,
                    Err(Error::Unauthorized)
                ));
                assert!(hook.selected().is_none());
                assert_eq!(fixture.authority.load().await?, before);
                assert_unconsumed_attempt(&fixture, challenge.expose()).await?;
                assert_eq!(fixture.db.table("audit_records")?.count().await?, 0);
                assert_eq!(fixture.db.table("auth_refresh_tokens")?.count().await?, 0);
                Ok(())
            })
        })
        .await
    }
}

#[tokio::test]
async fn independently_prepared_recovery_replay_is_unauthorized_without_publication() -> Result<()>
{
    owned(|fixture| {
        Box::pin(async move {
            enable(&fixture).await?;
            let a = challenge(&fixture).await?;
            let b = challenge(&fixture).await?;
            let first = fixture
                .policy
                .prepare_challenge(a.expose(), "source-a")
                .await?;
            let second = fixture
                .policy
                .prepare_challenge(b.expose(), "source-a")
                .await?;
            let proof_a = first.verify(&fixture.actions, None, Some("first")).await?;
            let proof_b = second.verify(&fixture.actions, None, Some("first")).await?;
            assert!(finish(&fixture, first, proof_a).await?.is_some());
            let winner = fixture.authority.load().await?;
            assert!(matches!(
                finish(&fixture, second, proof_b).await,
                Err(Error::Unauthorized)
            ));
            assert_eq!(fixture.authority.load().await?, winner);
            assert_unconsumed_attempt(&fixture, b.expose()).await?;
            Ok(())
        })
    })
    .await
}
