//! 真实 SQLite 三字段状态、同用户并发、事务回滚与持久代际。
use super::*;
use crate::config::FrameworkConfig;
use crate::config::NoConfig;
use crate::database::DatabaseServiceProvider;
use crate::database::Db;
use crate::database::MigrationServiceProvider;
use crate::database::sea_orm::ConnectionTrait;
use crate::database::sea_orm_migration::MigrationName;
use crate::database::sea_orm_migration::MigrationTrait;
use crate::database::sea_orm_migration::MigratorTrait;
use crate::foundation::ApplicationDefinition;
use crate::support::{DefaultProviders, ServiceProvider};
use crate::testing::TestApp;
use crate::testing::TestDatabase;
use base64::{Engine as _, engine::general_purpose::STANDARD};

struct Migrator;
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(crate::auth::AuthMigration),
            Box::new(super::super::migration::FortifyTwoFactorMigration::new()),
            Box::new(super::super::authority_migration::FactorAuthorityMigration::new()),
        ]
    }
}
struct MigrationApp;
impl ApplicationDefinition for MigrationApp {
    type Config = NoConfig;
    fn default_providers() -> DefaultProviders {
        DefaultProviders::empty()
    }
    fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
        vec![
            Box::new(DatabaseServiceProvider),
            Box::new(MigrationServiceProvider::<Migrator>::new()),
        ]
    }
}
fn encrypter() -> Result<std::sync::Arc<dyn crate::encryption::ApplicationEncrypter>> {
    crate::auth::fortify::test_field_encrypter(&STANDARD.encode([7_u8; 32]))
}
async fn fixture() -> Result<(TestDatabase, Db, DatabaseFactorAuthority)> {
    let database = TestDatabase::sqlite_file::<Migrator>().await?;
    let connection = database.connection()?;
    connection.execute_unprepared("INSERT INTO users(id,email,password_hash,created_at,updated_at) VALUES(42,'authority@example.test','hash',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP),(43,'other@example.test','hash',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)").await.map_err(Error::internal)?;
    let db = Db::default();
    db.initialize(connection)?;
    let authority = DatabaseFactorAuthority::new(DatabaseTwoFactorState::new(
        db.clone(),
        "users",
        "id",
        serde_json::json!(42),
    )?);
    Ok((database, db, authority))
}
fn modern() -> Result<TwoFactorState> {
    Ok(TwoFactorState {
        secret: Some(encrypter()?.encrypt_serialized_string(b"GEZDGNBVGY3TQOJQ")?),
        recovery_codes: Some(encrypter()?.encrypt_serialized_string(br#"["one","two"]"#)?),
        confirmed_at: None,
    })
}

#[tokio::test]
async fn persistent_generation_isolates_aba_and_main_state_but_not_single_code_consumption()
-> Result<()> {
    let (database, db, authority) = fixture().await?;
    let outcome: Result<()> = async {
        let (first, second) = tokio::join!(authority.load(), authority.load());
        let first = first?;
        let second = second?;
        assert_eq!(first.source, second.source);
        assert_eq!(first.generation, second.generation);
        assert_eq!(first.status, FactorAuthorityStatus::Disabled);
        let enabled = authority
            .replace(&first, &modern()?, FactorAuthorityMutation::Enable)
            .await?;
        assert!(enabled.challenge_required(false));
        assert!(!enabled.challenge_required(true));
        assert_ne!(first.generation, enabled.generation);
        assert!(
            authority
                .replace(
                    &first,
                    &TwoFactorState::default(),
                    FactorAuthorityMutation::Disable
                )
                .await
                .is_err()
        );
        let mut confirmed = enabled.state.clone();
        confirmed.confirmed_at = Some(Utc::now());
        assert!(
            authority
                .replace(&enabled, &confirmed, FactorAuthorityMutation::Confirm)
                .await
                .is_err()
        );
        let confirmed = authority
            .confirm_verified(&enabled, &confirmed, "111111")
            .await?;
        assert_ne!(enabled.generation, confirmed.generation);
        assert!(confirmed.challenge_required(true));
        let mut replacement = confirmed.state.clone();
        replacement.recovery_codes =
            Some(encrypter()?.encrypt_serialized_string(br#"["replacement","two"]"#)?);
        assert!(
            authority
                .consume_modern(&confirmed, Some(&confirmed.state), None)
                .await
                .is_err()
        );
        assert!(
            authority
                .consume_modern(&confirmed, Some(&replacement), Some("123456"))
                .await
                .is_err()
        );
        let (left, right) = tokio::join!(
            authority.consume_modern(&confirmed, Some(&replacement), None),
            authority.consume_modern(&confirmed, Some(&replacement), None)
        );
        assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
        let current = authority.load().await?;
        assert_eq!(current.generation, confirmed.generation);
        db.table("users")?
            .where_eq("id", 42)?
            .update(vec![(
                "two_factor_secret",
                encrypter()?
                    .encrypt_serialized_string(b"JBSWY3DPEHPK3PXP")?
                    .into(),
            )])
            .await?;
        let externally_changed = authority.load().await?;
        assert_ne!(externally_changed.generation, current.generation);
        let disabled = authority
            .replace(
                &externally_changed,
                &TwoFactorState::default(),
                FactorAuthorityMutation::Disable,
            )
            .await?;
        let repeated = authority
            .replace(&disabled, &modern()?, FactorAuthorityMutation::Enable)
            .await?;
        assert_ne!(repeated.generation, enabled.generation);
        assert_eq!(repeated.source, first.source);
        assert!(!format!("{repeated:?}").contains(&repeated.source));
        Ok(())
    }
    .await;
    let closed = database.close().await;
    outcome?;
    closed
}

#[tokio::test]
async fn account_origin_and_raw_identifier_cannot_alias_another_account() -> Result<()> {
    let (database, db, authority) = fixture().await?;
    let outcome:Result<()>=async {
        use crate::auth::{GenericUser, DatabaseUserProvider};
        let initial = authority.load().await?;
        authority.replace(&initial, &modern()?, FactorAuthorityMutation::Enable).await?;
        let provider=DatabaseUserProvider::new(db.clone(),std::sync::Arc::new(crate::hashing::HashManager::new(crate::hashing::HashingConfig { bcrypt: crate::hashing::BcryptConfig { rounds: 4, verify: true, limit: None }, ..Default::default() })?) as std::sync::Arc<dyn crate::hashing::Hasher>,"users")?;
        let user: AuthUser=std::sync::Arc::new(GenericUser::new(serde_json::json!({"id":42}).as_object().cloned().unwrap_or_default()));
        authority.validate_account(&user,&provider).await?;
        let string_user:AuthUser=std::sync::Arc::new(GenericUser::new(serde_json::json!({"id":"42"}).as_object().cloned().unwrap_or_default()));
        assert!(authority.validate_account(&string_user,&provider).await.is_err());
        let other=DatabaseUserProvider::new(db.clone(),std::sync::Arc::new(crate::hashing::HashManager::new(crate::hashing::HashingConfig { bcrypt: crate::hashing::BcryptConfig { rounds: 4, verify: true, limit: None }, ..Default::default() })?) as std::sync::Arc<dyn crate::hashing::Hasher>,"other_accounts")?;
        assert!(authority.validate_account(&user,&other).await.is_err());
        let string_authority=DatabaseFactorAuthority::new(DatabaseTwoFactorState::new(db.clone(),"users","id",serde_json::json!("42"))?);
        assert!(string_authority.load().await.is_err());
        db.connection()?.execute_unprepared("CREATE TABLE customers(id TEXT PRIMARY KEY,two_factor_secret TEXT,two_factor_recovery_codes TEXT,two_factor_confirmed_at datetime,two_factor_authority_source TEXT,two_factor_authority_generation TEXT,two_factor_consumed_codes TEXT,two_factor_management_session_hash TEXT,two_factor_management_generation TEXT,two_factor_management_authorized_at datetime)").await.map_err(Error::internal)?;
        db.table("customers")?.insert(vec![("id","uuid-customer".into())]).await?;
        let custom=DatabaseFactorAuthority::new(DatabaseTwoFactorState::new(db.clone(),"customers","id",serde_json::json!("uuid-customer"))?);
        let snapshot=custom.load().await?;assert_eq!(snapshot.status,FactorAuthorityStatus::Disabled);
        assert_eq!(authority.load().await?.status,FactorAuthorityStatus::Pending);
        let uppercase=DatabaseFactorAuthority::new(DatabaseTwoFactorState::new(db.clone(),"USERS","id",serde_json::json!(42))?);
        assert_eq!(uppercase.load().await?.status,FactorAuthorityStatus::Pending);
        Ok(())
    }.await;
    let closed = database.close().await;
    outcome?;
    closed
}

#[tokio::test]
async fn partial_authority_identity_is_rejected_without_initializing_missing_metadata() -> Result<()>
{
    let (database, db, authority) = fixture().await?;
    let outcome: Result<()> = async {
        db.table("users")?
            .where_eq("id", 42)?
            .update(vec![(SOURCE, "a".repeat(64).into())])
            .await?;
        assert!(authority.load().await.is_err());
        let row = db
            .table("users")?
            .where_eq("id", 42)?
            .first()
            .await?
            .ok_or(Error::Unauthorized)?;
        assert_eq!(
            row.try_get::<Option<String>>("", GENERATION)
                .map_err(Error::internal)?,
            None
        );
        Ok(())
    }
    .await;
    let closed = database.close().await;
    outcome?;
    closed
}

async fn migration(database: &TestDatabase, args: Vec<String>) -> Result<()> {
    let sources=figment::Figment::from(figment::providers::Serialized::defaults(FrameworkConfig::default()))
        .merge(figment::providers::Serialized::defaults(serde_json::json!({"database":{"url":database.url(),"max_connections":2,"min_connections":0}})));
    TestApp::<MigrationApp>::builder(sources)
        .run_command("migrate", args)
        .await
}
#[tokio::test]
async fn authority_migration_down_refuses_initialized_metadata_and_preserves_original_state()
-> Result<()> {
    let (database, _, authority) = fixture().await?;
    let outcome: Result<()> = async {
        // 未初始化metadata可以正常down/up；通过标准SQLite迁移执行所有者。
        migration(&database, vec!["down".into(), "1".into()]).await?;
        migration(&database, vec![]).await?;
        let snapshot = authority.load().await?;
        assert!(
            migration(&database, vec!["down".into(), "1".into()])
                .await
                .is_err()
        );
        let current = authority.load().await?;
        assert_eq!(snapshot.source, current.source);
        assert_eq!(snapshot.generation, current.generation);
        assert_eq!(current.state, TwoFactorState::default());
        assert_eq!(
            super::super::authority_migration::FactorAuthorityMigration::with_table("customers")?
                .name(),
            super::super::authority_migration::FactorAuthorityMigration::with_table("customers")?
                .name()
        );
        assert_ne!(
            super::super::authority_migration::FactorAuthorityMigration::new().name(),
            super::super::authority_migration::FactorAuthorityMigration::with_table("customers")?
                .name()
        );
        Ok(())
    }
    .await;
    let closed = database.close().await;
    outcome?;
    closed
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "同一真实事务状态机连续核验安全边界及原值保全"
)]
async fn modern_totp_same_code_is_atomic_and_ledger_never_changes_session_generation() -> Result<()>
{
    let (database, db, authority) = fixture().await?;
    let outcome: Result<()> = async {
        let crypto = encrypter()?;
        assert!(
            authority
                .clone()
                .with_verification_window(u64::MAX)
                .is_err()
        );
        let before = authority.load().await?;
        let mut state = modern()?;
        state.confirmed_at = Some(Utc::now());
        let modern = authority
            .replace(&before, &state, FactorAuthorityMutation::Enable)
            .await?;
        assert!(authority.consume_modern(&modern, None, None).await.is_err());
        let (left, right) = tokio::join!(
            authority.consume_modern(&modern, None, Some("123456")),
            authority.consume_modern(&modern, None, Some("123456"))
        );
        assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
        assert_eq!(authority.load().await?.generation, modern.generation);
        assert!(
            authority
                .consume_modern(&modern, None, Some("123456"))
                .await
                .is_err()
        );
        let row = db
            .table("users")?
            .where_eq("id", 42)?
            .first()
            .await?
            .ok_or(Error::Unauthorized)?;
        let text: String = row.try_get("", CONSUMED).map_err(Error::internal)?;
        assert!(!text.contains("123456"));
        assert_eq!(decode_consumed(Some(&text))?.len(), 1);
        // 清除过期摘要再原子插入，不把过期视为损坏状态。
        db.table("users")?
            .where_eq("id", 42)?
            .update(vec![(
                CONSUMED,
                serde_json::json!({format!("{:x}",Sha256::digest(b"123456")):0})
                    .to_string()
                    .into(),
            )])
            .await?;
        let renewed = authority
            .consume_modern(&modern, None, Some("123456"))
            .await?;
        assert_eq!(renewed.generation, modern.generation);
        let mut new_codes = renewed.state.clone();
        new_codes.recovery_codes = Some(crypto.encrypt_serialized_string(br#"["regenerated"]"#)?);
        let regenerated = authority
            .replace(&renewed, &new_codes, FactorAuthorityMutation::Regenerate)
            .await?;
        assert!(
            authority
                .consume_modern(&regenerated, None, Some("123456"))
                .await
                .is_err()
        );
        let forced = authority
            .replace(
                &regenerated,
                &regenerated.state,
                FactorAuthorityMutation::Enable,
            )
            .await?;
        assert!(
            authority
                .consume_modern(&forced, None, Some("123456"))
                .await
                .is_err()
        );
        let disabled = authority
            .replace(
                &forced,
                &TwoFactorState::default(),
                FactorAuthorityMutation::Disable,
            )
            .await?;
        let row = db
            .table("users")?
            .where_eq("id", 42)?
            .first()
            .await?
            .ok_or(Error::Unauthorized)?;
        let retained: Option<String> = row.try_get("", CONSUMED).map_err(Error::internal)?;
        assert!(
            decode_consumed(retained.as_deref())?
                .contains_key(&format!("{:x}", Sha256::digest(b"123456")))
        );
        assert_ne!(disabled.generation, modern.generation);
        Ok(())
    }
    .await;
    let closed = database.close().await;
    outcome?;
    closed
}

#[tokio::test]
async fn malformed_consumption_ledger_is_never_ignored() -> Result<()> {
    let (database, db, authority) = fixture().await?;
    let outcome: Result<()> = async {
        authority.load().await?;
        let hash = format!("{:x}", Sha256::digest(b"123456"));
        for value in [
            "[]".to_owned(),
            "null".to_owned(),
            "{".to_owned(),
            "{\"123456\":123}".to_owned(),
            format!("{{\"{hash}\":-1}}"),
            format!("{{\"{hash}\":1.2}}"),
            format!("{{\"{hash}\":1,\"{hash}\":2}}"),
            format!("{{\"{hash}\":18446744073709551615}}"),
            format!("{{\"{hash}\":1}} trailing"),
        ] {
            db.table("users")?
                .where_eq("id", 42)?
                .update(vec![(CONSUMED, value.into())])
                .await?;
            assert!(authority.load().await.is_err());
        }
        Ok(())
    }
    .await;
    let closed = database.close().await;
    outcome?;
    closed
}

#[tokio::test]
async fn confirmation_replay_and_storage_failure_never_publish_confirmed_state_or_nonce()
-> Result<()> {
    let (database, db, authority) = fixture().await?;
    let outcome:Result<()>=async {
        let before=authority.load().await?;
        let pending=authority.replace(&before,&modern()?,FactorAuthorityMutation::Enable).await?;
        authority.consume_modern(&pending,None,Some("654321")).await?;
        let mut replacement=pending.state.clone();replacement.confirmed_at=Some(Utc::now());
        assert!(authority.confirm_verified(&pending,&replacement,"654321").await.is_err());
        let unchanged=authority.load().await?;
        assert_eq!(unchanged.state.confirmed_at,None);assert_eq!(unchanged.generation,pending.generation);
        db.connection()?.execute_unprepared("CREATE TRIGGER reject_authority_confirmation BEFORE UPDATE OF two_factor_confirmed_at ON users WHEN NEW.two_factor_confirmed_at IS NOT NULL BEGIN SELECT RAISE(ABORT,'confirmation storage failure'); END").await.map_err(Error::internal)?;
        assert!(authority.confirm_verified(&unchanged,&replacement,"111111").await.is_err());
        let unchanged=authority.load().await?;
        assert_eq!(unchanged.state.confirmed_at,None);assert_eq!(unchanged.generation,pending.generation);
        let row=db.table("users")?.where_eq("id",42)?.first().await?.ok_or(Error::Unauthorized)?;
        let text:Option<String>=row.try_get("",CONSUMED).map_err(Error::internal)?;
        assert!(!decode_consumed(text.as_deref())?.contains_key(&format!("{:x}",Sha256::digest(b"111111"))));
        db.connection()?.execute_unprepared("DROP TRIGGER reject_authority_confirmation").await.map_err(Error::internal)?;
        let confirmed=authority.confirm_verified(&unchanged,&replacement,"111111").await?;
        assert!(confirmed.state.confirmed_at.is_some());assert_ne!(confirmed.generation,pending.generation);
        assert!(authority.consume_modern(&confirmed,None,Some("111111")).await.is_err());
        Ok(())
    }.await;
    let closed = database.close().await;
    outcome?;
    closed
}

fn management_context(session: &str) -> Result<FactorManagementContext> {
    // 测试内核直接使用crate-private sealed签发点；真实Session身份核验由Guard HTTP回归覆盖。
    FactorManagementContext::issue(
        serde_json::json!(42),
        "web".into(),
        Some("users".into()),
        format!("{:x}", Sha256::digest(session.as_bytes())),
        None,
    )
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "同一真实事务状态机连续核验安全边界及原值保全"
)]
async fn management_continuation_is_bound_to_one_session_and_exact_new_generation() -> Result<()> {
    let (database, _, authority) = fixture().await?;
    let outcome: Result<()> = async {
        let owner = management_context("owner-session")?;
        let other = management_context("other-session")?;
        assert!(owner.factor_proof().is_none());
        let before = authority.load().await?;
        let enabled = authority
            .replace_with_management(
                &before,
                &modern()?,
                FactorAuthorityMutation::Enable,
                &owner,
                false,
            )
            .await?;
        authority
            .authorize_management(&enabled, &owner, false)
            .await?;
        assert!(
            authority
                .authorize_management(&enabled, &other, false)
                .await
                .is_err()
        );
        let forced = authority
            .replace_with_management(
                &enabled,
                &modern()?,
                FactorAuthorityMutation::Enable,
                &owner,
                false,
            )
            .await?;
        authority
            .authorize_management(&forced, &owner, false)
            .await?;
        assert!(
            authority
                .authorize_management(&forced, &other, false)
                .await
                .is_err()
        );
        assert!(
            authority
                .authorize_management(&enabled, &owner, false)
                .await
                .is_err()
        );
        let mut replacement = forced.state.clone();
        replacement.recovery_codes =
            Some(encrypter()?.encrypt_serialized_string(br#"["new-readable"]"#)?);
        let regenerated = authority
            .replace_with_management(
                &forced,
                &replacement,
                FactorAuthorityMutation::Regenerate,
                &owner,
                false,
            )
            .await?;
        authority
            .authorize_management(&regenerated, &owner, false)
            .await?;
        assert!(
            authority
                .replace_with_management(
                    &regenerated,
                    &modern()?,
                    FactorAuthorityMutation::Enable,
                    &other,
                    false
                )
                .await
                .is_err()
        );
        // 未变代际的真实DB重读保留同一管理授权，模拟DB提交后请求取消再恢复。
        let restored = authority.load().await?;
        authority
            .authorize_management(&restored, &owner, false)
            .await?;
        let mut confirmed = restored.state.clone();
        confirmed.confirmed_at = Some(Utc::now());
        let confirmed = authority
            .confirm_verified_with_management(&restored, &confirmed, "123456", &owner, false)
            .await?;
        assert!(confirmed.management_hash.is_none());
        assert!(owner.factor_proof().is_none());
        assert!(
            authority
                .authorize_management(&confirmed, &owner, false)
                .await
                .is_err()
        );
        Ok(())
    }
    .await;
    let closed = database.close().await;
    outcome?;
    closed
}

#[tokio::test]
async fn failed_management_grant_rolls_back_factor_generation_and_state() -> Result<()> {
    let (database, db, authority) = fixture().await?;
    let outcome:Result<()>=async {
        let before=authority.load().await?;let owner=management_context("owner-session")?;
        db.connection()?.execute_unprepared("CREATE TRIGGER reject_management_grant BEFORE UPDATE OF two_factor_management_session_hash ON users WHEN NEW.two_factor_management_session_hash IS NOT NULL BEGIN SELECT RAISE(ABORT,'management storage failure'); END").await.map_err(Error::internal)?;
        assert!(authority.replace_with_management(&before,&modern()?,FactorAuthorityMutation::Enable,&owner,false).await.is_err());
        let current=authority.load().await?;assert_eq!(current,before);assert!(current.management_hash.is_none());
        Ok(())
    }.await;
    let closed = database.close().await;
    outcome?;
    closed
}

#[tokio::test]
async fn provider_period_controls_real_nonce_deadline_and_code_binding() -> Result<()> {
    let (database, db, authority) = fixture().await?;
    let result: Result<()> = async {
        let initial = authority.load().await?;
        let mut state = modern()?;
        state.confirmed_at = Some(Utc::now());
        let snapshot = authority
            .replace(&initial, &state, FactorAuthorityMutation::Enable)
            .await?;
        let fact = super::super::TwoFactorCodeVerification::verified_with_period(
            "custom-provider-code",
            3,
            100,
            102,
            60,
        )?;
        assert_eq!(fact.period_seconds(), Some(60));
        assert_eq!(fact.replay_lifetime_seconds(), Some(540));
        assert!(
            super::super::TwoFactorCodeVerification::verified_with_period("code", 0, 0, 0, 0)
                .is_err()
        );
        let selected = authority.clone().with_provider_verification(&fact)?;
        assert!(
            selected
                .consume_modern(&snapshot, None, Some("other-code"))
                .await
                .is_err()
        );
        let before = Utc::now().timestamp();
        selected
            .consume_modern(&snapshot, None, Some("custom-provider-code"))
            .await?;
        let row = db
            .table("users")?
            .where_eq("id", 42_i64)?
            .first()
            .await?
            .ok_or(Error::Unauthorized)?;
        let encoded: String = row
            .try_get("", "two_factor_consumed_codes")
            .map_err(Error::internal)?;
        let ledger: serde_json::Value = serde_json::from_str(&encoded).map_err(Error::internal)?;
        let hash = format!("{:x}", Sha256::digest(b"custom-provider-code"));
        let expiry = ledger
            .get(&hash)
            .and_then(serde_json::Value::as_i64)
            .ok_or(Error::Unauthorized)?;
        assert!(expiry >= before + 540 && expiry <= Utc::now().timestamp() + 540);
        assert!(
            ledger
                .get(format!("{:x}", Sha256::digest(b"other-code")))
                .is_none()
        );
        assert!(
            selected
                .consume_modern(&snapshot, None, Some("custom-provider-code"))
                .await
                .is_err()
        );
        Ok(())
    }
    .await;
    let cleanup = database.close().await;
    result?;
    cleanup
}

#[tokio::test]
async fn captured_confirmation_flag_cannot_authorize_pending_management_after_config_change()
-> Result<()> {
    let (database, db, authority) = fixture().await?;
    let outcome: Result<()> = async {
        let repository = crate::config::repository::ConfigRepository::new(figment::Figment::new());
        repository.set(
            "fortify.features",
            serde_json::json!(["two-factor-authentication"]),
        )?;
        repository.set(
            "fortify-options.two-factor-authentication.confirm",
            serde_json::json!(true),
        )?;
        let authority = authority.with_confirmation_config(repository.clone());
        let initial = authority.load().await?;
        let pending = authority
            .replace(&initial, &modern()?, FactorAuthorityMutation::Enable)
            .await?;
        let context = management_context("pending-without-grant")?;
        assert!(context.factor_proof().is_none());
        authority
            .authorize_management(&pending, &context, true)
            .await?;
        for clear_features in [false, true] {
            repository.set(
                "fortify.features",
                serde_json::json!(["two-factor-authentication"]),
            )?;
            repository.set(
                "fortify-options.two-factor-authentication.confirm",
                serde_json::json!(true),
            )?;
            authority
                .authorize_management(&pending, &context, true)
                .await?;
            if clear_features {
                repository.set("fortify.features", serde_json::json!([]))?;
            } else {
                repository.set(
                    "fortify-options.two-factor-authentication.confirm",
                    serde_json::json!(false),
                )?;
            }
            let forced = authority
                .replace_with_management(
                    &pending,
                    &modern()?,
                    FactorAuthorityMutation::Enable,
                    &context,
                    true,
                )
                .await
                .err()
                .ok_or(Error::Unauthorized)?;
            assert_eq!(forced.status(), http::StatusCode::FORBIDDEN);
            let mut confirmed = pending.state.clone();
            confirmed.confirmed_at = Some(Utc::now());
            let error = authority
                .confirm_verified_with_management(&pending, &confirmed, "111111", &context, true)
                .await
                .err()
                .ok_or(Error::Unauthorized)?;
            assert_eq!(error.status(), http::StatusCode::FORBIDDEN);
            assert_eq!(authority.load().await?, pending);
            let row = db
                .table("users")?
                .where_eq("id", 42)?
                .first()
                .await?
                .ok_or(Error::Unauthorized)?;
            assert_eq!(
                row.try_get::<Option<String>>("", "two_factor_consumed_codes")
                    .map_err(Error::internal)?,
                None
            );
        }
        Ok(())
    }
    .await;
    let cleanup = database.close().await;
    outcome?;
    cleanup
}

#[tokio::test]
async fn cancelled_outer_transaction_rolls_back_code_consumption() -> Result<()> {
    let (database, db, authority) = fixture().await?;
    let outcome: Result<()> = async {
        let before = authority.load().await?;
        let pending = authority
            .replace(&before, &modern()?, FactorAuthorityMutation::Enable)
            .await?;
        let (entered, arrived) = tokio::sync::oneshot::channel();
        let owner = db.clone();
        let selected = authority.clone();
        let snapshot = pending.clone();
        let mut consuming = Box::pin(db.transaction_for_update(move |tx| {
            Box::pin(async move {
                selected
                    .consume_modern_in_transaction(&owner, tx, &snapshot, None, Some("654321"))
                    .await?;
                entered.send(()).map_err(|()| Error::Unauthorized)?;
                std::future::pending::<Result<()>>().await
            })
        }));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::select! {
                entered = arrived => entered.map_err(Error::internal),
                result = &mut consuming => result.and(Err(Error::Unauthorized)),
            }
        })
        .await
        .map_err(Error::internal)??;
        // 取消时写入已在外层事务内发生；重试取得写锁后才能证明回滚已生效。
        drop(consuming);
        assert_eq!(authority.load().await?, pending);
        authority
            .consume_modern(&pending, None, Some("654321"))
            .await?;
        assert!(
            authority
                .consume_modern(&pending, None, Some("654321"))
                .await
                .is_err()
        );
        Ok(())
    }
    .await;
    let closed = database.close().await;
    outcome?;
    closed
}
