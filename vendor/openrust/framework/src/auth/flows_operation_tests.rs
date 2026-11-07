//! 登录查询与双秘密域 reset 的已选叶回归；服务 fixture 显式清理。

use super::{
    AuthFlows, Db, EncodedPasswordHash, Error, JwtTokens, RateLimiter, RefreshTokens, Result, Utc,
    VerificationCodePolicy, VerificationCodes,
};
use crate::auth::password::operation_test_support::{
    Counts, Probe, count, install, live, test_error,
};
use crate::database::sea_orm_migration::{MigrationTrait, MigratorTrait};
use crate::hashing::{BcryptConfig, BcryptHasher, HashManager, HashOptions, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Migrations;
impl MigratorTrait for Migrations {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(crate::auth::AuthMigration),
            Box::new(crate::auth::AuthRefreshTokenMigration),
            Box::new(crate::auth::AuthVerificationCodeMigration),
            Box::new(crate::database::DatabaseRateLimitMigration),
        ]
    }
}
fn flows(db: Db, manager: Arc<HashManager>, codes: VerificationCodes) -> Result<AuthFlows> {
    let jwt = JwtTokens::new(crate::config::JwtConfig::new(
        "issuer",
        "audience",
        "test",
        std::collections::BTreeMap::from([(
            "test".into(),
            "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=".into(),
        )]),
    ))?;
    Ok(AuthFlows::new(
        db.clone(),
        manager,
        jwt,
        RefreshTokens::new(db.clone()),
        codes,
        RateLimiter::new(crate::database::DatabaseRateLimitStore::new(db)),
    ))
}
async fn insert_user(db: &Db, id: i64, email: &str, encoded: &str) -> Result<()> {
    let backend = db.connection()?.get_database_backend();
    let now = crate::database::dialect::utc_value(backend, Utc::now());
    db.table("users")?
        .insert(vec![
            ("id", id.into()),
            ("email", email.into()),
            ("password_hash", encoded.into()),
            ("email_verified_at", now.clone()),
            ("created_at", now.clone()),
            ("updated_at", now),
        ])
        .await?;
    Ok(())
}
async fn login_cases(connection: crate::database::sea_orm::DatabaseConnection) -> Result<()> {
    let setup = Db::default();
    setup.initialize(connection.clone())?;
    let plain = "z".repeat(73);
    let (seed, _) = Probe::new(false)?;
    let good = seed.make(plain.as_bytes(), &HashOptions::new()).await?;
    let bcrypt = BcryptHasher::new(BcryptConfig {
        rounds: 4,
        verify: true,
        limit: None,
    })?
    .make(b"short-password", &HashOptions::new())
    .await?;
    insert_user(&setup, 41, "present@example.test", &good).await?;
    insert_user(&setup, 42, "legacy@example.test", &bcrypt).await?;
    for (email, password, expected_check, expected_dummy) in [
        ("present@example.test", plain.as_str(), 1, 0),
        ("missing@example.test", plain.as_str(), 0, 1),
        ("legacy@example.test", plain.as_str(), 0, 1),
        ("legacy@example.test", "short-password", 1, 1),
    ] {
        let (manager, repository) = live()?;
        let (a, ac) = Probe::new(false)?;
        let (b, bc) = Probe::new(true)?;
        install(&manager, "a", a)?;
        install(&manager, "b", b)?;
        let errors = Arc::new(AtomicBool::new(false));
        let failed = Arc::clone(&errors);
        let queries = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&queries);
        let mut connection = connection.clone();
        connection.set_metric_callback(move |info| {
            if info.statement.sql.contains("SELECT id, password_hash FROM") {
                observed.fetch_add(1, Ordering::SeqCst);
                if repository
                    .set("hashing.driver", serde_json::json!("b"))
                    .is_err()
                {
                    failed.store(true, Ordering::SeqCst);
                }
            }
        });
        let db = Db::default();
        db.initialize(connection)?;
        let (outer, _) = live()?;
        install(&outer, "a", manager)?;
        let service = flows(
            db.clone(),
            Arc::clone(&outer),
            VerificationCodes::new(
                crate::auth::DatabaseVerificationCodeStore::new(db.clone()),
                outer,
            ),
        )?;
        let result = service.verify_password(email, password).await;
        if expected_dummy == 0 {
            assert_eq!(result?.0, 41);
        } else {
            assert!(matches!(result, Err(Error::Unauthorized)));
        }
        assert!(!errors.load(Ordering::SeqCst));
        assert_eq!(queries.load(Ordering::SeqCst), 1);
        assert_eq!(count(&ac.selections), 1);
        assert_eq!(count(&ac.checks), expected_check);
        assert_eq!(count(&ac.makes), expected_dummy);
        assert_eq!(
            *ac.made_lengths.lock().map_err(|_| test_error())?,
            vec![1; expected_dummy]
        );
        assert_eq!(count(&bc.selections), 0);
        assert_eq!(count(&bc.checks), 0);
        assert_eq!(count(&bc.makes), 0);
        assert!(
            matches!(service.login(email, &plain, "next-operation").await, Err(error) if error.code() == "password_bcrypt_input_ambiguous")
        );
        assert_eq!(count(&bc.selections), 1);
        assert_eq!(count(&bc.checks), 0);
        assert_eq!(count(&bc.makes), 0);
        assert_eq!(db.table("auth_refresh_tokens")?.count().await?, 0);
    }
    assert_eq!(setup.table("users")?.count().await?, 2);
    assert_eq!(
        setup
            .table("users")?
            .where_eq("id", 41_i64)?
            .first()
            .await?
            .ok_or_else(test_error)?
            .try_get::<String>("", "password_hash")
            .map_err(Error::internal)?,
        good
    );
    reset_domains(&setup).await
}

#[derive(Clone)]
struct ResetStore {
    counts: Arc<Counts>,
    encoded: String,
    loaded: Arc<AtomicUsize>,
}
impl crate::auth::VerificationCodeStore for ResetStore {
    fn issue<'a>(
        &'a self,
        _key: &'a str,
        _id: &'a str,
        _hash: &'a EncodedPasswordHash,
        _policy: VerificationCodePolicy,
    ) -> crate::auth::VerificationCodeFuture<'a, Result<()>> {
        Box::pin(async { Err(test_error()) })
    }
    fn load_active<'a>(
        &'a self,
        _key: &'a str,
    ) -> crate::auth::VerificationCodeFuture<'a, Result<Option<crate::auth::VerificationChallenge>>>
    {
        Box::pin(async move {
            assert_eq!(count(&self.counts.makes), 1);
            self.loaded.fetch_add(1, Ordering::SeqCst);
            Ok(Some(crate::auth::VerificationChallenge::new(
                "reset-challenge",
                EncodedPasswordHash::from_encoded(self.encoded.clone()),
                6,
            )?))
        })
    }
    fn finish_attempt<'a>(
        &'a self,
        _key: &'a str,
        _id: &'a str,
        matches: bool,
    ) -> crate::auth::VerificationCodeFuture<'a, Result<bool>> {
        Box::pin(async move {
            assert!(matches);
            Ok(false)
        })
    }
}
async fn reset_domains(db: &Db) -> Result<()> {
    let (passwords, _) = live()?;
    let (password_probe, pc) = Probe::new(false)?;
    install(&passwords, "a", password_probe)?;
    let (codes_manager, _) = live()?;
    let (code_probe, cc) = Probe::new(false)?;
    let encoded = code_probe.make(b"123456", &HashOptions::new()).await?;
    install(&codes_manager, "a", code_probe)?;
    let loaded = Arc::new(AtomicUsize::new(0));
    let store = ResetStore {
        counts: Arc::clone(&pc),
        encoded,
        loaded: Arc::clone(&loaded),
    };
    let service = flows(
        db.clone(),
        passwords,
        VerificationCodes::new(store, codes_manager),
    )?;
    assert!(
        !service
            .reset_password(
                "present@example.test",
                "123456",
                "new-password",
                "reset-two-domains"
            )
            .await?
    );
    assert_eq!(count(&pc.selections), 1);
    assert_eq!(count(&pc.makes), 1);
    assert_eq!(count(&pc.checks), 0);
    assert_eq!(count(&cc.selections), 1);
    assert_eq!(count(&cc.checks), 1);
    assert_eq!(loaded.load(Ordering::SeqCst), 1);
    Ok(())
}
async fn run(database: crate::testing::TestDatabase) -> Result<()> {
    let connection = match database.connection() {
        Ok(connection) => connection,
        Err(error) => {
            let cleanup = database.close().await;
            return finish(Err(error), cleanup);
        }
    };
    let outcome = tokio::spawn(async move { login_cases(connection).await }).await;
    let cleanup = database.close().await;
    match outcome {
        Ok(result) => finish(result, cleanup),
        Err(error) => {
            if error.is_panic() {
                if let Err(cleanup) = cleanup {
                    eprintln!("HA 测试 panic 后数据库清理失败 code={}", cleanup.code());
                }
                std::panic::resume_unwind(error.into_panic());
            }
            finish(Err(Error::internal(error)), cleanup)
        }
    }
}
fn finish(primary: Result<()>, cleanup: Result<()>) -> Result<()> {
    if let Err(error) = &cleanup {
        eprintln!("HA 测试数据库清理失败 code={}", error.code());
    }
    primary.and(cleanup)
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
#[ignore = "发布SQLite矩阵：真实数据库操作，须显式专跑HA操作回归"]
async fn sqlite_operation_leaf_login_and_reset_domains() -> Result<()> {
    run(crate::testing::TestDatabase::sqlite_memory::<Migrations>().await?).await
}
#[cfg(feature = "db-mysql")]
#[tokio::test]
#[ignore = "日常MySQL8.4服务验收：须配置OPENRUST_TEST_DATABASE_URL并显式专跑"]
async fn mysql_operation_leaf_login_and_reset_domains() -> Result<()> {
    let url = crate::test_database_environment::database_url().map_err(Error::internal)?;
    if !url.starts_with("mysql://") {
        return Err(test_error());
    }
    run(crate::testing::TestDatabase::create::<Migrations>(&url).await?).await
}

#[tokio::test]
async fn flow_basic_invalid_input_precedes_broken_driver_and_database_access() -> Result<()> {
    let (manager, repository) = live()?;
    repository.set("hashing.driver", serde_json::json!("invalid"))?;
    let db = Db::default();
    let service = flows(
        db.clone(),
        Arc::clone(&manager),
        VerificationCodes::new(crate::auth::DatabaseVerificationCodeStore::new(db), manager),
    )?;
    for password in [String::new(), "x".repeat(1025)] {
        assert!(matches!(
            service
                .verify_password("user@example.test", &password)
                .await,
            Err(Error::BadRequest(_))
        ));
    }
    assert!(
        matches!(service.verify_password("user@example.test", "valid-password").await, Err(error) if error.code() == "hashing_driver_unsupported")
    );
    Ok(())
}
