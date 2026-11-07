//! 真实SQLite、数据库账户模型、Session收据与主库父凭据重核的PAT回归。
use super::*;
use crate::auth::AuthUser;
use crate::auth::Authenticatable;
use crate::auth::DatabaseUserProvider;
use crate::auth::GenericUser;
use crate::auth::SessionGuard;
use crate::auth::StatefulGuard;
use crate::auth::UserProvider;
use crate::auth::fortify::CredentialFactorMigration;
use crate::auth::fortify::DatabaseFactorAuthority;
use crate::auth::fortify::DatabaseTwoFactorState;
use crate::auth::fortify::FactorAuthorityMigration;
use crate::auth::fortify::FactorAuthorityMutation;
use crate::auth::fortify::FortifyTwoFactorMigration;
use crate::auth::fortify::GoogleTwoFactorAuthenticationProvider;
use crate::auth::fortify::TwoFactorActions;
use crate::auth::fortify::TwoFactorAuthenticatable;
use crate::auth::fortify::TwoFactorState;
use crate::database::sea_orm_migration::MigrationTrait;
use crate::database::sea_orm_migration::MigratorTrait;
use crate::events::Events;
use crate::events::EventsServiceProvider;
use crate::foundation::ProviderBuilder;
use crate::http::CookieFactory;
use crate::http::CookieJar;
use crate::session::MemorySessionStore;
use crate::session::Session;
use crate::support::BoxFuture;
use crate::testing::TestDatabase;
use serde_json::{Map, Value, json};
use std::sync::Arc;

struct Migrations;
impl MigratorTrait for Migrations {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(crate::auth::AuthMigration),
            Box::new(crate::auth::AuthRefreshTokenMigration),
            Box::new(crate::auth::AuthRememberTokenMigration),
            Box::new(crate::auth::AuthVerificationCodeMigration),
            Box::new(crate::database::DatabaseRateLimitMigration),
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
    state: DatabaseTwoFactorState,
    authority: DatabaseFactorAuthority,
}
impl Authenticatable for Account {
    fn auth_identifier_name(&self) -> &str {
        self.attributes.auth_identifier_name()
    }
    fn auth_identifier(&self) -> Result<Value> {
        self.attributes.auth_identifier()
    }
    fn auth_password_name(&self) -> &'static str {
        "password_hash"
    }
    fn auth_password(&self) -> Result<Option<String>> {
        Ok(None)
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
        value: Option<String>,
    ) -> BoxFuture<'a, Result<Option<crate::auth::RememberTokenUpdate>>> {
        self.attributes.begin_remember_token_update(expected, value)
    }
    fn remember_token_name(&self) -> &str {
        self.attributes.remember_token_name()
    }
    fn as_two_factor(&self) -> Option<&dyn TwoFactorAuthenticatable> {
        Some(self)
    }
}
impl TwoFactorAuthenticatable for Account {
    fn two_factor_account_name(&self, _: &str) -> Result<Option<String>> {
        Ok(None)
    }
    fn two_factor_authority(&self) -> Option<&DatabaseFactorAuthority> {
        Some(&self.authority)
    }
    fn two_factor_state(&self) -> BoxFuture<'_, Result<TwoFactorState>> {
        self.state.load()
    }
    fn compare_and_set_two_factor_state<'a>(
        &'a self,
        before: &'a TwoFactorState,
        replacement: &'a TwoFactorState,
    ) -> BoxFuture<'a, Result<bool>> {
        self.state.compare_and_set(before, replacement)
    }
}
struct Fixture {
    db: Db,
    policy: CredentialAuthority,
    authority: DatabaseFactorAuthority,
    guard: SessionGuard,
    session: Session,
    tokens: PersonalAccessTokens,
    runtime: crate::foundation::ProviderRuntime,
}
async fn account(db: &Db, id: i64) -> Result<()> {
    let now = crate::database::dialect::utc_value(DbBackend::Sqlite, Utc::now());
    db.table("users")?
        .insert(vec![
            ("id", id.into()),
            ("email", format!("pat-{id}@example.test").into()),
            ("password_hash", "unused-by-trusted-session-fixture".into()),
            ("created_at", now.clone()),
            ("updated_at", now),
        ])
        .await?;
    Ok(())
}
async fn fixture(db: Db) -> Result<Fixture> {
    account(&db, 42).await?;
    account(&db, 43).await?;
    let state = DatabaseTwoFactorState::new(db.clone(), "users", "id", json!(42))?;
    let authority = DatabaseFactorAuthority::new(state.clone());
    let factory_db = db.clone();
    let provider: Arc<dyn UserProvider> = Arc::new(
        DatabaseUserProvider::new(
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
        .with_user_factory(move |attributes: Map<String, Value>| {
            let identifier = attributes.get("id").cloned().ok_or(Error::Unauthorized)?;
            let state = DatabaseTwoFactorState::new(factory_db.clone(), "users", "id", identifier)?;
            Ok(Arc::new(Account {
                attributes: GenericUser::new(attributes),
                authority: DatabaseFactorAuthority::new(state.clone()),
                state,
            }) as AuthUser)
        }),
    );
    let user = provider
        .retrieve_by_id(&json!(42))
        .await?
        .ok_or(Error::Unauthorized)?;
    let session = Session::new(None, Arc::new(MemorySessionStore::default()), None)?;
    session.start().await?;
    let guard = SessionGuard::new(
        "web".into(),
        provider.clone(),
        session.clone(),
        CookieJar::new(CookieFactory::default()),
        None,
        None,
        None,
    )
    .with_provider_name(Some("users".into()));
    let mut builder = ProviderBuilder::new();
    builder.provider(EventsServiceProvider::new());
    let runtime = builder.start(figment::Figment::new()).await?;
    let events = (*runtime.container().resolve::<Events>().await?).clone();
    let field_crypto = crate::auth::fortify::test_field_encrypter(STANDARD_KEY)?;
    let actions = TwoFactorActions::new(
        Arc::new(
            GoogleTwoFactorAuthenticationProvider::new(None).with_clock(Arc::new(|| Ok(3_000))),
        ),
        field_crypto.clone(),
        events,
    );
    actions.enable(&user, false).await?;
    let before = authority.load().await?;
    let mut fixed = before.state.clone();
    fixed.secret = Some(field_crypto.encrypt_serialized_string(b"JBSWY3DPEHPK3PXP")?);
    authority
        .replace(&before, &fixed, FactorAuthorityMutation::Enable)
        .await?;
    let receipt = actions
        .confirm_with_receipt(&user, "594318", "web", Some("users"), provider.as_ref())
        .await?
        .ok_or(Error::Unauthorized)?;
    guard
        .login_with_factor_receipt(user, false, receipt)
        .await?;
    let policy = CredentialAuthority::new(db.clone(), false);
    let tokens = PersonalAccessTokens::new(db.clone()).with_factor_authority(policy.clone())?;
    Ok(Fixture {
        db,
        policy,
        authority,
        guard,
        session,
        tokens,
        runtime,
    })
}
const STANDARD_KEY: &str = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
async fn owned<F>(callback: F) -> Result<()>
where
    F: FnOnce(Fixture) -> BoxFuture<'static, Result<()>>,
{
    let database = TestDatabase::sqlite_file::<Migrations>().await?;
    let db = Db::default();
    let outcome = async {
        db.initialize(database.connection()?)?;
        callback(Box::pin(fixture(db.clone())).await?).await
    }
    .await;
    drop(db);
    let closed = database.close().await;
    outcome?;
    closed
}
async fn prepare(fixture: &Fixture) -> Result<VerifiedCredential> {
    fixture.policy.prepare_session(&fixture.guard, "web").await
}
fn active_deadline(parent: &VerifiedCredential) -> Result<DateTime<Utc>> {
    let expiry = parent.expires_at().ok_or(Error::Unauthorized)?;
    assert!(expiry > Utc::now());
    Ok(expiry)
}

async fn session_pat(
    fixture: &Fixture,
    name: &str,
    abilities: &[String],
) -> Result<IssuedPersonalAccessToken> {
    let parent = prepare(fixture).await?;
    let expiry = active_deadline(&parent)?;
    fixture
        .tokens
        .issue_from_verified(parent, name, abilities, Some(expiry))
        .await
}

async fn context_text(db: &Db) -> Result<String> {
    db.table("personal_access_tokens")?
        .select(&["factor_context"])?
        .first()
        .await?
        .ok_or(Error::Unauthorized)?
        .try_get("", "factor_context")
        .map_err(Error::internal)
}

#[tokio::test]
async fn receipt_session_issues_real_pat_domain_and_revocation_is_current() -> Result<()> {
    owned(|fixture| Box::pin(async move {
        assert!(fixture.tokens.issue(42, "raw", &[], None).await.err().is_some_and(|error| error.code() == "auth_credential_proof_required"));
        assert!(PersonalAccessTokens::new(Db::default()).with_factor_authority(fixture.policy.clone()).is_err());
        let expiry = DateTime::from_timestamp(Utc::now().timestamp() + 1_800, 123_456_000).ok_or(Error::Unauthorized)?;
        let parent = prepare(&fixture).await?;
        assert!(expiry <= active_deadline(&parent)?);
        let token = fixture.tokens.issue_from_verified(parent, "managed", &["articles.read".into()], Some(expiry)).await?;
        let verified = fixture.tokens.authenticate_verified(token.expose()).await?;
        assert_eq!(verified.user_id(), 42);
        assert_eq!(verified.expires_at(), Some(expiry));
        assert_eq!(verified.token_abilities(), Some(["articles.read".into()].as_slice()));
        assert!(matches!(verified.factor_origin(), Some(crate::auth::credential::FactorOrigin::Guard { guard, provider }) if guard == "web" && provider.as_deref() == Some("users")));
        let metadata = fixture.tokens.list_for_user(42).await?;
        let row = metadata.first().ok_or(Error::Unauthorized)?;
        assert!(matches!(verified.domain(), CredentialDomain::PersonalAccessToken { id, token_hash } if *id == row.id && token_hash == &sha256_hex(token.expose())));
        assert!(row.last_used_at.is_some());
        assert!(fixture.tokens.revoke(42, row.id).await?);
        assert!(fixture.tokens.authenticate_verified(token.expose()).await.is_err());
        Ok(())
    })).await
}

#[tokio::test]
async fn null_corrupt_wrong_source_and_wrong_owner_contexts_never_authenticate() -> Result<()> {
    owned(|fixture| {
        Box::pin(async move {
            let token = session_pat(&fixture, "context", &[]).await?;
            let original = context_text(&fixture.db).await?;
            fixture
                .db
                .table("personal_access_tokens")?
                .update(vec![("factor_context", sea_orm::Value::String(None))])
                .await?;
            assert!(
                fixture
                    .tokens
                    .authenticate_verified(token.expose())
                    .await
                    .is_err()
            );
            for invalid in ["{}", "not-json"] {
                fixture
                    .db
                    .table("personal_access_tokens")?
                    .update(vec![("factor_context", invalid.into())])
                    .await?;
                assert!(
                    fixture
                        .tokens
                        .authenticate_verified(token.expose())
                        .await
                        .is_err()
                );
            }
            let mut altered: Value = serde_json::from_str(&original).map_err(Error::internal)?;
            altered["source"] = json!("different-authority");
            fixture
                .db
                .table("personal_access_tokens")?
                .update(vec![(
                    "factor_context",
                    serde_json::to_string(&altered)
                        .map_err(Error::internal)?
                        .into(),
                )])
                .await?;
            assert!(
                fixture
                    .tokens
                    .authenticate_verified(token.expose())
                    .await
                    .is_err()
            );
            fixture
                .db
                .table("personal_access_tokens")?
                .update(vec![
                    ("factor_context", original.into()),
                    ("user_id", 43_i64.into()),
                ])
                .await?;
            assert!(
                fixture
                    .tokens
                    .authenticate_verified(token.expose())
                    .await
                    .is_err()
            );
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn factor_generation_change_invalidates_pat_and_prepared_session() -> Result<()> {
    owned(|fixture| {
        Box::pin(async move {
            let parent = prepare(&fixture).await?;
            let deadline = active_deadline(&parent)?;
            let token = session_pat(&fixture, "generation", &[]).await?;
            let before = fixture.authority.load().await?;
            fixture
                .authority
                .replace(&before, &before.state, FactorAuthorityMutation::Enable)
                .await?;
            assert!(
                fixture
                    .tokens
                    .authenticate_verified(token.expose())
                    .await
                    .is_err()
            );
            assert!(matches!(
                fixture
                    .tokens
                    .issue_from_verified(parent, "stale-parent", &[], Some(deadline))
                    .await,
                Err(Error::Unauthorized)
            ));
            assert!(prepare(&fixture).await.is_err());
            Ok(())
        })
    })
    .await
}

async fn assert_pat_parent_limits(
    fixture: &Fixture,
    parent: &IssuedPersonalAccessToken,
    expiry: DateTime<Utc>,
    child_expiry: DateTime<Utc>,
) -> Result<()> {
    for abilities in [vec!["articles.write".into()], vec!["*".into()]] {
        assert!(
            fixture
                .tokens
                .issue_from_verified(
                    fixture
                        .tokens
                        .authenticate_verified(parent.expose())
                        .await?,
                    "broader",
                    &abilities,
                    Some(child_expiry)
                )
                .await
                .is_err()
        );
    }
    for expiry in [None, Some(expiry + chrono::Duration::seconds(1))] {
        let verified = fixture
            .tokens
            .authenticate_verified(parent.expose())
            .await?;
        assert!(matches!(
            fixture
                .tokens
                .issue_from_verified(verified, "longer", &[], expiry)
                .await,
            Err(Error::Forbidden(_))
        ));
    }
    let prepared_before_shortening = fixture
        .tokens
        .authenticate_verified(parent.expose())
        .await?;
    let shortened = expiry - chrono::Duration::minutes(2);
    fixture
        .db
        .table("personal_access_tokens")?
        .where_eq("token_hash", sha256_hex(parent.expose()))?
        .update(vec![(
            "expires_at",
            crate::database::dialect::utc_value(DbBackend::Sqlite, shortened),
        )])
        .await?;
    assert!(
        fixture
            .tokens
            .issue_from_verified(
                prepared_before_shortening,
                "stale-expiry",
                &[],
                Some(child_expiry)
            )
            .await
            .is_err()
    );
    fixture
        .db
        .table("personal_access_tokens")?
        .where_eq("token_hash", sha256_hex(parent.expose()))?
        .update(vec![(
            "expires_at",
            crate::database::dialect::utc_value(DbBackend::Sqlite, expiry),
        )])
        .await?;
    Ok(())
}

#[tokio::test]
async fn pat_parent_cannot_expand_abilities_expiry_or_survive_current_revocation() -> Result<()> {
    owned(|fixture| {
        Box::pin(async move {
            let expiry = DateTime::from_timestamp(Utc::now().timestamp() + 1_200, 123_456_000)
                .ok_or(Error::Unauthorized)?;
            let parent = fixture
                .tokens
                .issue_from_verified(
                    prepare(&fixture).await?,
                    "parent",
                    &["articles.read".into()],
                    Some(expiry),
                )
                .await?;
            let child_expiry = expiry - chrono::Duration::minutes(1);
            let child = fixture
                .tokens
                .issue_from_verified(
                    fixture
                        .tokens
                        .authenticate_verified(parent.expose())
                        .await?,
                    "subset",
                    &["articles.read".into()],
                    Some(child_expiry),
                )
                .await?;
            assert_eq!(
                fixture
                    .tokens
                    .authenticate_verified(child.expose())
                    .await?
                    .expires_at(),
                Some(child_expiry)
            );
            assert_pat_parent_limits(&fixture, &parent, expiry, child_expiry).await?;
            let wildcard = session_pat(&fixture, "wildcard", &["*".into()]).await?;
            let verified = fixture
                .tokens
                .authenticate_verified(wildcard.expose())
                .await?;
            let wildcard_deadline = active_deadline(&verified)?;
            let child = fixture
                .tokens
                .issue_from_verified(
                    verified,
                    "wildcard-child",
                    &["articles.write".into()],
                    Some(wildcard_deadline),
                )
                .await?;
            let child = fixture.tokens.authenticate_verified(child.expose()).await?;
            assert_eq!(child.expires_at(), Some(wildcard_deadline));
            assert_eq!(
                child.token_abilities(),
                Some(["articles.write".into()].as_slice())
            );
            let prepared = fixture
                .tokens
                .authenticate_verified(parent.expose())
                .await?;
            let id = match prepared.domain() {
                CredentialDomain::PersonalAccessToken { id, .. } => *id,
                _ => return Err(Error::Unauthorized),
            };
            fixture.tokens.revoke(42, id).await?;
            assert!(
                fixture
                    .tokens
                    .issue_from_verified(prepared, "revoked-parent", &[], Some(child_expiry))
                    .await
                    .is_err()
            );
            Ok(())
        })
    })
    .await
}

fn family_flows(
    db: Db,
    policy: CredentialAuthority,
    lifetime: chrono::Duration,
) -> Result<(crate::auth::AuthFlows, crate::auth::JwtTokens)> {
    use base64::Engine as _;
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
    let jwt = crate::auth::JwtTokens::new(
        crate::auth::JwtConfig::new(
            "https://credential.example.test",
            "credential-family-tests",
            "family-test",
            std::collections::BTreeMap::from([(
                "family-test".into(),
                base64::engine::general_purpose::STANDARD.encode([0x53_u8; 32]),
            )]),
        )
        .with_ttl(600, 600),
    )?;
    let codes = crate::auth::VerificationCodes::new(
        crate::auth::DatabaseVerificationCodeStore::new(db.clone()),
        passwords.clone(),
    );
    let limiter =
        crate::core::RateLimiter::new(crate::database::DatabaseRateLimitStore::new(db.clone()));
    let flows = crate::auth::AuthFlows::new(
        db.clone(),
        passwords,
        jwt.clone(),
        crate::auth::RefreshTokens::new(db),
        codes,
        limiter,
    )
    .with_refresh_lifetime(lifetime)
    .with_factor_authority(policy)?;
    Ok((flows, jwt))
}
async fn family_deadline(db: &Db, family: &str) -> Result<DateTime<Utc>> {
    db.table("auth_refresh_tokens")?
        .select(&["expires_at"])?
        .where_eq("family_id", family)?
        .first()
        .await?
        .ok_or(Error::Unauthorized)?
        .try_get("", "expires_at")
        .map_err(Error::internal)
}
#[tokio::test]
async fn session_family_refresh_preserves_origin_and_rotation_then_replay_revokes_all_jwts()
-> Result<()> {
    owned(|fixture| Box::pin(async move {
        let session = prepare(&fixture).await?;
        let origin = session.factor_origin().cloned().ok_or(Error::Unauthorized)?;
        let verified_at = session.factor_verified_at().ok_or(Error::Unauthorized)?;
        let (flows, jwt) = family_flows(fixture.db.clone(), fixture.policy.clone(), chrono::Duration::hours(1))?;
        let original = flows.issue_credentials_from_session(&fixture.guard, "web").await?;
        let first_claims = jwt.verify_credential(original.access_token())?;
        let first_grant = first_claims.grant.as_ref().ok_or(Error::Unauthorized)?;
        let expected_expiry = family_deadline(&fixture.db, &first_grant.family_id).await?;
        // SQLite标准UTC持久化精度为微秒；家族轮换须保留该实际存储截止，不能重新延长。
        assert_eq!(expected_expiry.timestamp_micros(), original.refresh_expires_at().timestamp_micros());
        let verified = fixture.policy.authenticate_access_token(&jwt, original.access_token()).await?;
        assert_eq!(verified.user_id(), 42);
        assert_eq!(verified.factor_origin(), Some(&origin));
        assert_eq!(verified.factor_verified_at(), Some(verified_at));
        assert!(matches!(verified.domain(), CredentialDomain::Jwt { issuer, audience, jti, family_id } if issuer == &first_claims.issuer && audience == &first_claims.audience && jti == &first_claims.jti && family_id == &first_grant.family_id));
        let successor = flows.refresh(original.refresh_token().expose(), "family-contract").await?;
        let next_claims = jwt.verify_credential(successor.access_token())?;
        let next_grant = next_claims.grant.as_ref().ok_or(Error::Unauthorized)?;
        assert!(next_grant.eq(first_grant));
        assert_ne!(next_claims.jti, first_claims.jti);
        assert_eq!(successor.refresh_expires_at(), expected_expiry);
        let next_verified = fixture.policy.authenticate_access_token(&jwt, successor.access_token()).await?;
        assert_eq!(next_verified.factor_origin(), Some(&origin));
        assert_eq!(next_verified.factor_verified_at(), Some(verified_at));
        assert!(fixture.policy.authenticate_access_token(&jwt, original.access_token()).await.is_ok());
        assert!(flows.refresh(original.refresh_token().expose(), "family-contract").await.is_err());
        assert!(jwt.verify_access_token(original.access_token()).is_ok());
        assert!(jwt.verify_access_token(successor.access_token()).is_ok());
        assert!(fixture.policy.authenticate_access_token(&jwt, original.access_token()).await.is_err());
        assert!(fixture.policy.authenticate_access_token(&jwt, successor.access_token()).await.is_err());
        assert!(flows.refresh(successor.refresh_token().expose(), "family-contract").await.is_err());
        let family_rows = fixture.db.table("auth_refresh_tokens")?.where_eq("family_id", first_grant.family_id.clone())?.get().await?;
        assert_eq!(family_rows.len(), 2);
        for row in family_rows { assert!(row.try_get::<Option<DateTime<Utc>>>("", "revoked_at").map_err(Error::internal)?.is_some()); }
        Ok(())
    })).await
}
#[tokio::test]
async fn high_level_jwt_expiry_is_bounded_by_shorter_refresh_family() -> Result<()> {
    owned(|fixture| {
        Box::pin(async move {
            let (flows, jwt) = family_flows(
                fixture.db.clone(),
                fixture.policy.clone(),
                chrono::Duration::seconds(120),
            )?;
            let issued = flows
                .issue_credentials_from_session(&fixture.guard, "web")
                .await?;
            let claims = jwt.verify_credential(issued.access_token())?;
            let grant = claims.grant.as_ref().ok_or(Error::Unauthorized)?;
            let family = family_deadline(&fixture.db, &grant.family_id).await?;
            let verified = fixture
                .policy
                .authenticate_access_token(&jwt, issued.access_token())
                .await?;
            assert!(DateTime::<Utc>::from(claims.expires_at) > family);
            assert_eq!(verified.expires_at(), Some(family));
            assert_eq!(
                family.timestamp_micros(),
                issued.refresh_expires_at().timestamp_micros()
            );
            Ok(())
        })
    })
    .await
}
#[tokio::test]
async fn failed_successor_insert_rolls_back_refresh_consumption_and_same_raw_can_retry()
-> Result<()> {
    owned(|fixture| Box::pin(async move {
        let (flows, jwt) = family_flows(fixture.db.clone(), fixture.policy.clone(), chrono::Duration::hours(1))?;
        let issued = flows.issue_credentials_from_session(&fixture.guard, "web").await?;
        let raw = issued.refresh_token().expose();
        let before = fixture.db.table("auth_refresh_tokens")?.where_eq("token_hash", sha256_hex(raw))?.first().await?.ok_or(Error::Unauthorized)?;
        let context: String = before.try_get("", "factor_context").map_err(Error::internal)?;
        let table = fixture.db.quoted_table_name("auth_refresh_tokens")?;
        fixture.db.connection()?.execute_unprepared(&format!("CREATE TRIGGER reject_managed_refresh_successor BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT, 'injected successor insert failure'); END")).await.map_err(Error::internal)?;
        let failed = flows.refresh(raw, "rollback-contract").await;
        // 先移除本测试故障注入，再检查失败原因与事务状态；不调整框架快照。
        fixture.db.connection()?.execute_unprepared("DROP TRIGGER reject_managed_refresh_successor").await.map_err(Error::internal)?;
        let error = failed.err().ok_or(Error::Unauthorized)?;
        assert!(error.to_string().contains("injected successor insert failure"));
        let after = fixture.db.table("auth_refresh_tokens")?.where_eq("token_hash", sha256_hex(raw))?.first().await?.ok_or(Error::Unauthorized)?;
        assert!(after.try_get::<Option<DateTime<Utc>>>("", "consumed_at").map_err(Error::internal)?.is_none());
        assert!(after.try_get::<Option<DateTime<Utc>>>("", "revoked_at").map_err(Error::internal)?.is_none());
        assert_eq!(after.try_get::<String>("", "factor_context").map_err(Error::internal)?, context);
        assert_eq!(fixture.db.table("auth_refresh_tokens")?.get().await?.len(), 1);
        assert!(fixture.policy.authenticate_access_token(&jwt, issued.access_token()).await.is_ok());
        let retried = flows.refresh(raw, "rollback-contract").await?;
        assert_eq!(fixture.db.table("auth_refresh_tokens")?.get().await?.len(), 2);
        assert!(fixture.policy.authenticate_access_token(&jwt, retried.access_token()).await.is_ok());
        Ok(())
    })).await
}
#[tokio::test]
async fn family_tokens_reject_factor_change_and_session_parent_from_another_store() -> Result<()> {
    owned(|fixture| {
        Box::pin(async move {
            let (flows, jwt) = family_flows(
                fixture.db.clone(),
                fixture.policy.clone(),
                chrono::Duration::hours(1),
            )?;
            let parent = prepare(&fixture).await?;
            let other_database = TestDatabase::sqlite_file::<Migrations>().await?;
            let other_db = Db::default();
            let cross_store: Result<()> = async {
                other_db.initialize(other_database.connection()?)?;
                let other_policy = CredentialAuthority::new(other_db.clone(), false);
                let (other_flows, _) =
                    family_flows(other_db.clone(), other_policy, chrono::Duration::hours(1))?;
                assert!(other_flows.issue_credentials_from(parent).await.is_err());
                Ok(())
            }
            .await;
            drop(other_db);
            let closed = other_database.close().await;
            cross_store?;
            closed?;
            let issued = flows
                .issue_credentials_from_session(&fixture.guard, "web")
                .await?;
            let before = fixture.authority.load().await?;
            fixture
                .authority
                .replace(&before, &before.state, FactorAuthorityMutation::Enable)
                .await?;
            assert!(jwt.verify_access_token(issued.access_token()).is_ok());
            assert!(
                fixture
                    .policy
                    .authenticate_access_token(&jwt, issued.access_token())
                    .await
                    .is_err()
            );
            assert!(
                flows
                    .refresh(issued.refresh_token().expose(), "generation-contract")
                    .await
                    .is_err()
            );
            Ok(())
        })
    })
    .await
}

fn default_pat_credential(raw: &str) -> crate::auth::AuthCredential<'_> {
    crate::auth::AuthCredential {
        source: crate::auth::AuthSource::PersonalAccessToken,
        value: Some(raw),
        session: None,
        csrf_token: None,
        csrf_verified: false,
        method: &http::Method::GET,
    }
}

#[tokio::test]
async fn default_database_authenticator_rejects_managed_and_malformed_nonnull_pat_context()
-> Result<()> {
    use crate::auth::Authenticator;
    owned(|fixture| {
        Box::pin(async move {
            let managed = session_pat(
                &fixture,
                "managed-default-boundary",
                &["articles.read".into()],
            )
            .await?;
            let authenticator = crate::auth::database::DatabaseAuthenticator::default();
            authenticator.initialize(fixture.db.clone())?;
            let error = authenticator
                .authenticate(default_pat_credential(managed.expose()))
                .await
                .err()
                .ok_or(Error::Unauthorized)?;
            assert_eq!(error.code(), "auth_managed_credential_validator_required");
            fixture
                .db
                .table("personal_access_tokens")?
                .where_eq("token_hash", sha256_hex(managed.expose()))?
                .update(vec![("factor_context", "not-json".into())])
                .await?;
            let error = authenticator
                .authenticate(default_pat_credential(managed.expose()))
                .await
                .err()
                .ok_or(Error::Unauthorized)?;
            assert_eq!(error.code(), "auth_managed_credential_validator_required");
            let row = fixture
                .db
                .table("personal_access_tokens")?
                .where_eq("token_hash", sha256_hex(managed.expose()))?
                .first()
                .await?
                .ok_or(Error::Unauthorized)?;
            assert!(
                row.try_get::<Option<DateTime<Utc>>>("", "last_used_at")
                    .map_err(Error::internal)?
                    .is_none()
            );
            let ordinary = PersonalAccessTokens::new(fixture.db.clone())
                .issue(42, "ordinary-null-context", &["articles.read".into()], None)
                .await?;
            let row = fixture
                .db
                .table("personal_access_tokens")?
                .where_eq("token_hash", sha256_hex(ordinary.expose()))?
                .first()
                .await?
                .ok_or(Error::Unauthorized)?;
            assert!(
                row.try_get::<Option<String>>("", "factor_context")
                    .map_err(Error::internal)?
                    .is_none()
            );
            let principal = authenticator
                .authenticate(default_pat_credential(ordinary.expose()))
                .await?;
            assert_eq!(principal.user_id, 42);
            assert_eq!(
                principal.token_abilities.as_deref(),
                Some(["articles.read".into()].as_slice())
            );
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn selected_auth_flows_rejects_refresh_store_from_another_database_without_writes()
-> Result<()> {
    owned(|fixture| {
        Box::pin(async move {
            let other_database = TestDatabase::sqlite_file::<Migrations>().await?;
            let other_db = Db::default();
            let outcome: Result<()> = async {
                other_db.initialize(other_database.connection()?)?;
                let before_main = fixture.db.table("auth_refresh_tokens")?.get().await?.len();
                let before_other = other_db.table("auth_refresh_tokens")?.get().await?.len();
                let (_, jwt) = family_flows(
                    fixture.db.clone(),
                    fixture.policy.clone(),
                    chrono::Duration::hours(1),
                )?;
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
                let codes = crate::auth::VerificationCodes::new(
                    crate::auth::DatabaseVerificationCodeStore::new(fixture.db.clone()),
                    passwords.clone(),
                );
                let limiter = crate::core::RateLimiter::new(
                    crate::database::DatabaseRateLimitStore::new(fixture.db.clone()),
                );
                let flows = crate::auth::AuthFlows::new(
                    fixture.db.clone(),
                    passwords,
                    jwt,
                    crate::auth::RefreshTokens::new(other_db.clone()),
                    codes,
                    limiter,
                );
                assert!(flows.with_factor_authority(fixture.policy.clone()).is_err());
                assert_eq!(
                    fixture.db.table("auth_refresh_tokens")?.get().await?.len(),
                    before_main
                );
                assert_eq!(
                    other_db.table("auth_refresh_tokens")?.get().await?.len(),
                    before_other
                );
                Ok(())
            }
            .await;
            drop(other_db);
            let closed = other_database.close().await;
            outcome?;
            closed
        })
    })
    .await
}

async fn direct_login_flows(
    fixture: &Fixture,
) -> Result<(crate::auth::AuthFlows, crate::auth::JwtTokens)> {
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
    let hash =
        crate::auth::EncodedPasswordHash::make(passwords.as_ref(), "direct-password").await?;
    fixture
        .db
        .table("users")?
        .where_eq("id", 42_i64)?
        .update(vec![
            ("password_hash", hash.as_str().into()),
            (
                "email_verified_at",
                crate::database::dialect::utc_value(DbBackend::Sqlite, Utc::now()),
            ),
        ])
        .await?;
    let events = (*fixture.runtime.container().resolve::<Events>().await?).clone();
    let actions = TwoFactorActions::new(
        Arc::new(
            GoogleTwoFactorAuthenticationProvider::new(None).with_clock(Arc::new(|| Ok(3_030))),
        ),
        crate::auth::fortify::test_field_encrypter(STANDARD_KEY)?,
        events,
    );
    let (flows, jwt) = family_flows(
        fixture.db.clone(),
        fixture.policy.clone(),
        chrono::Duration::hours(1),
    )?;
    Ok((flows.with_factor_actions(actions), jwt))
}
#[tokio::test]
async fn direct_password_challenge_mints_current_family_and_replay_does_not_mint_again()
-> Result<()> {
    owned(|fixture| Box::pin(async move {
        let (flows, jwt) = direct_login_flows(&fixture).await?;
        let challenge = match flows.begin_login("pat-42@example.test", "direct-password", "owned-source").await? {
            crate::auth::LoginAttempt::TwoFactorRequired(challenge) => challenge,
            crate::auth::LoginAttempt::Authenticated(_) => return Err(Error::Unauthorized),
        };
        assert!(challenge.expose().starts_with("openrust_factor_"));
        assert!(fixture.db.table("auth_refresh_tokens")?.get().await?.is_empty());
        let code = crate::auth::totp::totp_code(b"Hello!\xde\xad\xbe\xef", 101, 6)?;
        assert!(flows.complete_two_factor_login(challenge.expose(), Some(&code), None, "wrong-source").await.is_err());
        let signed = flows.complete_two_factor_login(challenge.expose(), Some(&code), None, "owned-source").await?;
        let verified = fixture.policy.authenticate_access_token(&jwt, signed.access_token()).await?;
        assert_eq!(verified.user_id(), 42);
        assert!(matches!(verified.factor_origin(), Some(crate::auth::FactorOrigin::DatabaseFlow { purpose, .. }) if purpose == "password"));
        assert!(flows.complete_two_factor_login(challenge.expose(), Some(&code), None, "owned-source").await.is_err());
        fixture.policy.authenticate_access_token(&jwt, signed.access_token()).await?;
        assert_eq!(fixture.db.table("auth_refresh_tokens")?.get().await?.len(), 1);
        Ok(())
    })).await
}
#[tokio::test]
async fn direct_challenge_child_insert_failure_rolls_back_consumption_and_allows_real_retry()
-> Result<()> {
    owned(|fixture| Box::pin(async move {
        let (flows, jwt) = direct_login_flows(&fixture).await?;
        let challenge = match flows.begin_login("pat-42@example.test", "direct-password", "owned-source").await? {
            crate::auth::LoginAttempt::TwoFactorRequired(challenge) => challenge,
            crate::auth::LoginAttempt::Authenticated(_) => return Err(Error::Unauthorized),
        };
        fixture.db.connection()?.execute_unprepared("CREATE TRIGGER fail_direct_refresh BEFORE INSERT ON auth_refresh_tokens BEGIN SELECT RAISE(ABORT, 'owned-direct-refresh-failure'); END;").await.map_err(Error::internal)?;
        let before = fixture.authority.load().await?;
        let code = crate::auth::totp::totp_code(b"Hello!\xde\xad\xbe\xef", 101, 6)?;
        assert!(flows.complete_two_factor_login(challenge.expose(), Some(&code), None, "owned-source").await.is_err());
        assert_eq!(fixture.authority.load().await?, before);
        assert!(fixture.db.table("auth_refresh_tokens")?.get().await?.is_empty());
        let row = fixture.db.table("auth_factor_challenges")?.where_eq("token_hash", sha256_hex(challenge.expose()))?.first().await?.ok_or(Error::Unauthorized)?;
        assert!(row.try_get::<Option<DateTime<Utc>>>("", "consumed_at").map_err(Error::internal)?.is_none());
        fixture.db.connection()?.execute_unprepared("DROP TRIGGER fail_direct_refresh").await.map_err(Error::internal)?;
        let signed = flows.complete_two_factor_login(challenge.expose(), Some(&code), None, "owned-source").await?;
        fixture.policy.authenticate_access_token(&jwt, signed.access_token()).await?;
        Ok(())
    })).await
}

mod commit_boundary {
    use super::*;
    use crate::database::{DatabaseServiceProvider, TransactionBeforeCommit};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;

    #[derive(Default)]
    struct Boundary {
        reached: AtomicUsize,
        cancel: Notify,
    }

    impl Boundary {
        fn listeners(self: &Arc<Self>) -> EventsServiceProvider {
            let boundary = self.clone();
            let mut listeners = EventsServiceProvider::new();
            listeners.listen::<TransactionBeforeCommit, _>(move |_, context| {
                let boundary = boundary.clone();
                Box::pin(async move {
                    boundary
                        .interfere(context.transaction().ok_or(Error::Unauthorized)?)
                        .await
                })
            });
            listeners
        }

        async fn interfere(&self, tx: &DatabaseTransaction) -> Result<()> {
            let row = tx.query_one_raw(Statement::from_string(DbBackend::Sqlite,
                "SELECT name,expires_at FROM personal_access_tokens WHERE name LIKE 'boundary-%' OR (name LIKE 'use-%' AND last_used_at IS NOT NULL)"))
                .await.map_err(Error::internal)?;
            let Some(row) = row else {
                return Ok(());
            };
            let name: String = row.try_get("", "name").map_err(Error::internal)?;
            self.reached.fetch_add(1, Ordering::SeqCst);
            match name.as_str() {
                "boundary-parent-revoke" => {
                    let affected = tx.execute_unprepared("UPDATE personal_access_tokens SET revoked_at=CURRENT_TIMESTAMP WHERE name='parent' AND revoked_at IS NULL").await.map_err(Error::internal)?;
                    assert_eq!(affected.rows_affected(), 1);
                }
                "boundary-family-revoke" => {
                    let affected = tx.execute_unprepared("UPDATE auth_refresh_tokens SET revoked_at=CURRENT_TIMESTAMP WHERE user_id=42 AND revoked_at IS NULL").await.map_err(Error::internal)?;
                    assert_eq!(affected.rows_affected(), 1);
                }
                "boundary-child-escalate" => {
                    let affected = tx.execute_unprepared(r#"UPDATE personal_access_tokens SET abilities='["*"]' WHERE name='boundary-child-escalate'"#).await.map_err(Error::internal)?;
                    assert_eq!(affected.rows_affected(), 1);
                }
                "use-revoke" => {
                    let affected = tx.execute_unprepared("UPDATE personal_access_tokens SET revoked_at=CURRENT_TIMESTAMP WHERE name='use-revoke'").await.map_err(Error::internal)?;
                    assert_eq!(affected.rows_affected(), 1);
                }
                "use-expand" => {
                    let affected = tx.execute_unprepared(r#"UPDATE personal_access_tokens SET abilities='["*"]' WHERE name='use-expand'"#).await.map_err(Error::internal)?;
                    assert_eq!(affected.rows_affected(), 1);
                }
                "use-factor" => {
                    let affected = tx.execute_unprepared("UPDATE users SET two_factor_authority_generation=lower(hex(randomblob(32))) WHERE id=42").await.map_err(Error::internal)?;
                    assert_eq!(affected.rows_affected(), 1);
                }
                "boundary-expiry" | "use-expiry" => {
                    let expiry: DateTime<Utc> =
                        row.try_get("", "expires_at").map_err(Error::internal)?;
                    let delay = (expiry - Utc::now()).to_std().map_err(Error::internal)?;
                    tokio::time::sleep(delay + std::time::Duration::from_millis(25)).await;
                    assert!(Utc::now() >= expiry);
                }
                "boundary-cancel" => {
                    self.cancel.notify_one();
                    std::future::pending::<()>().await;
                }
                _ => return Err(Error::Unauthorized),
            }
            Ok(())
        }
    }

    async fn verify(fixture: &Fixture, boundary: &Boundary) -> Result<()> {
        fixture.session.save().await?;
        let abilities = vec!["articles.read".to_owned()];
        let parent = session_pat(fixture, "parent", &abilities).await?;
        let verified = fixture
            .tokens
            .authenticate_verified(parent.expose())
            .await?;
        let parent_deadline = active_deadline(&verified)?;
        for (index, name) in [
            "boundary-parent-revoke",
            "boundary-child-escalate",
            "boundary-expiry",
        ]
        .into_iter()
        .enumerate()
        {
            let expiry = if name == "boundary-expiry" {
                parent_deadline.min(Utc::now() + chrono::Duration::seconds(2))
            } else {
                parent_deadline
            };
            let verified = fixture
                .tokens
                .authenticate_verified(parent.expose())
                .await?;
            let result = fixture
                .tokens
                .issue_from_verified(verified, name, &abilities, Some(expiry))
                .await;
            assert!(matches!(result, Err(Error::Unauthorized)), "{name}");
            assert_eq!(boundary.reached.load(Ordering::SeqCst), index + 1);
            assert_eq!(
                fixture.db.table("personal_access_tokens")?.count().await?,
                1
            );
            // 对父行的监听器修改和子行插入属于同一事务，拒绝后原父令牌仍可用。
            fixture
                .tokens
                .authenticate_verified(parent.expose())
                .await?;
        }
        let verified = fixture
            .tokens
            .authenticate_verified(parent.expose())
            .await?;
        {
            let request = fixture.tokens.issue_from_verified(
                verified,
                "boundary-cancel",
                &abilities,
                Some(parent_deadline),
            );
            tokio::pin!(request);
            tokio::select! {
                result = &mut request => { result?; return Err(Error::custom("pat_cancel_missed","派生在取消屏障前意外结束")); },
                () = boundary.cancel.notified() => {},
                () = tokio::time::sleep(std::time::Duration::from_secs(5)) => return Err(Error::custom("pat_cancel_timeout","派生未到达提交前")),
            }
        }
        // 实际取得后继写事务并通过原父凭据后，取消的子行不能再提交。
        let verified = fixture
            .tokens
            .authenticate_verified(parent.expose())
            .await?;
        let child = fixture
            .tokens
            .issue_from_verified(verified, "retry", &abilities, Some(parent_deadline))
            .await?;
        assert_eq!(
            fixture
                .tokens
                .authenticate_verified(child.expose())
                .await?
                .token_abilities(),
            Some(abilities.as_slice())
        );
        assert_eq!(boundary.reached.load(Ordering::SeqCst), 4);
        assert_eq!(
            fixture.db.table("personal_access_tokens")?.count().await?,
            2
        );
        Ok(())
    }

    async fn verify_jwt_parent(fixture: &Fixture, boundary: &Boundary) -> Result<()> {
        let (flows, jwt) = family_flows(
            fixture.db.clone(),
            fixture.policy.clone(),
            chrono::Duration::hours(1),
        )?;
        let pair = flows
            .issue_credentials_from_session(&fixture.guard, "web")
            .await?;
        let parent = fixture
            .policy
            .authenticate_access_token(&jwt, pair.access_token())
            .await?;
        let expiry = parent.expires_at().ok_or(Error::Unauthorized)?;
        let before = fixture.db.table("personal_access_tokens")?.count().await?;
        let start = boundary.reached.load(Ordering::SeqCst);
        let result = fixture
            .tokens
            .issue_from_verified(parent, "boundary-family-revoke", &[], Some(expiry))
            .await;
        assert!(matches!(result, Err(Error::Unauthorized)));
        assert_eq!(boundary.reached.load(Ordering::SeqCst), start + 1);
        assert_eq!(
            fixture.db.table("personal_access_tokens")?.count().await?,
            before
        );
        fixture
            .policy
            .authenticate_access_token(&jwt, pair.access_token())
            .await?;
        Ok(())
    }

    async fn verify_authentication(fixture: &Fixture, boundary: &Boundary) -> Result<()> {
        let abilities = vec!["articles.read".to_owned()];
        let start = boundary.reached.load(Ordering::SeqCst);
        for (index, name) in ["use-revoke", "use-expand", "use-factor", "use-expiry"]
            .into_iter()
            .enumerate()
        {
            let parent = prepare(fixture).await?;
            let parent_deadline = active_deadline(&parent)?;
            let expiry = if name == "use-expiry" {
                parent_deadline.min(Utc::now() + chrono::Duration::seconds(2))
            } else {
                parent_deadline
            };
            let token = fixture
                .tokens
                .issue_from_verified(parent, name, &abilities, Some(expiry))
                .await?;
            let result = fixture.tokens.authenticate_verified(token.expose()).await;
            assert!(matches!(result, Err(Error::Unauthorized)), "{name}");
            assert_eq!(boundary.reached.load(Ordering::SeqCst), start + index + 1);
            let metadata = fixture
                .tokens
                .list_for_user(42)
                .await?
                .into_iter()
                .find(|item| item.name == name)
                .ok_or(Error::Unauthorized)?;
            assert!(metadata.last_used_at.is_none());
            assert!(metadata.revoked_at.is_none());
            assert_eq!(metadata.abilities, abilities);
        }
        Ok(())
    }

    #[tokio::test]
    async fn before_commit_changes_cannot_publish_a_derived_pat_or_a_stale_pat_identity()
    -> Result<()> {
        let database = TestDatabase::sqlite_file::<Migrations>().await?;
        let boundary = Arc::new(Boundary::default());
        let result: Result<()> = Box::pin(async {
            let mut builder = ProviderBuilder::new();
            builder
                .provider(DatabaseServiceProvider)
                .provider(boundary.listeners());
            let runtime = builder.start(figment::Figment::new()).await?;
            let outcome: Result<()> = Box::pin(async {
                let db = (*runtime.container().resolve::<Db>().await?).clone();
                db.initialize(database.connection()?)?;
                let fixture = Box::pin(fixture(db)).await?;
                verify(&fixture, &boundary).await?;
                verify_jwt_parent(&fixture, &boundary).await?;
                verify_authentication(&fixture, &boundary).await
            })
            .await;
            outcome.and(
                runtime
                    .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
                    .await,
            )
        })
        .await;
        result.and(database.close().await)
    }
}
