//! 受控凭据的真实数据库回归；普通旧schema路径由原测试继续覆盖。

use super::*;
use crate::{
    auth::{credential::CredentialFactorContext, fortify::CredentialFactorMigration},
    database::sea_orm_migration::{MigrationTrait, MigratorTrait},
    support::BoxFuture,
    testing::TestDatabase,
};

struct Migrator;
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(crate::auth::AuthMigration),
            Box::new(crate::auth::AuthRefreshTokenMigration),
            Box::new(CredentialFactorMigration::new()),
        ]
    }
}
fn context(user_id: i64) -> CredentialFactorContext {
    CredentialFactorContext {
        scope: None,
        version: 1,
        identifier: user_id.into(),
        source: "owned-test-source".into(),
        generation: "owned-test-generation".into(),
        factor: None,
    }
}
fn check(value: bool) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(Error::custom(
            "controlled_refresh_contract",
            "受控refresh契约断言失败",
        ))
    }
}
async fn exercise(db: &Db, user_id: i64) -> Result<()> {
    let service = RefreshTokens::new(db.clone());
    let expiry = Utc::now() + chrono::Duration::hours(2);
    let issued = db
        .transaction_for_update(|tx| {
            let service = service.clone();
            Box::pin(async move {
                service
                    .issue_context_in_transaction(tx, user_id, expiry, &context(user_id))
                    .await
            })
        })
        .await?;
    let family = issued.family_id.clone();
    let raw = issued.token.expose().to_owned();
    let rotated = db
        .transaction_for_update(|tx| {
            let service = service.clone();
            let raw = raw.clone();
            let family = family.clone();
            Box::pin(async move {
                let locked = service
                    .lock_context_in_transaction(tx, &raw)
                    .await?
                    .ok_or(Error::Unauthorized)?;
                check(
                    locked.user_id() == user_id
                        && locked.family_id() == family
                        && locked.expires_at().timestamp() == expiry.timestamp()
                        && locked.context() == &context(user_id),
                )?;
                let deadline = service
                    .validate_family_in_transaction(tx, user_id, &family, &context(user_id))
                    .await?;
                check(deadline.timestamp() == expiry.timestamp())?;
                let mut wrong = context(user_id);
                wrong.generation = "wrong-generation".into();
                check(matches!(
                    service
                        .validate_family_in_transaction(tx, user_id, &family, &wrong)
                        .await,
                    Err(Error::Unauthorized)
                ))?;
                service.rotate_context_in_transaction(locked).await
            })
        })
        .await?;
    check(
        rotated.user_id == user_id
            && rotated.family_id == family
            && rotated.context == context(user_id)
            && rotated.expires_at.timestamp() == expiry.timestamp(),
    )?;
    db.table("auth_refresh_tokens")?
        .where_eq("token_hash", sha256_hex(&raw))?
        .update(vec![(
            "factor_context",
            "{corrupt-after-consumption".into(),
        )])
        .await?;
    // 已消费旧token重放必须Ok(None)，让撤销提交，随后有效后继也失效。
    db.transaction_for_update(|tx| {
        let service = service.clone();
        let raw = raw.clone();
        Box::pin(async move {
            check(
                service
                    .lock_context_in_transaction(tx, &raw)
                    .await?
                    .is_none(),
            )
        })
    })
    .await?;
    db.transaction_for_update(|tx| {
        let service = service.clone();
        let successor = rotated.token.expose().to_owned();
        let family = family.clone();
        Box::pin(async move {
            check(
                service
                    .lock_context_in_transaction(tx, &successor)
                    .await?
                    .is_none(),
            )?;
            check(matches!(
                service
                    .validate_family_in_transaction(tx, user_id, &family, &context(user_id))
                    .await,
                Err(Error::Unauthorized)
            ))
        })
    })
    .await?;
    reject_missing_or_malformed_context(db, &service, user_id, expiry).await
}
async fn reject_missing_or_malformed_context(
    db: &Db,
    service: &RefreshTokens,
    user_id: i64,
    expiry: DateTime<Utc>,
) -> Result<()> {
    for stored in [None, Some("{bad-json".to_owned())] {
        let ordinary = service.issue(user_id, expiry).await?;
        let hash = sha256_hex(ordinary.expose());
        let backend = db.connection()?.get_database_backend();
        let (a, b) = if backend == DbBackend::Postgres {
            ("$1", "$2")
        } else {
            ("?", "?")
        };
        let sql = crate::auth::resolve_auth_sql(
            db,
            &format!("UPDATE tb_auth_refresh_tokens SET factor_context={a} WHERE token_hash={b}"),
        )?;
        db.connection()?
            .execute_raw(Statement::from_sql_and_values(
                backend,
                sql,
                [stored.into(), hash.into()],
            ))
            .await
            .map_err(Error::internal)?;
        db.transaction_for_update(|tx| {
            let service = RefreshTokens::clone(service);
            let raw = ordinary.expose().to_owned();
            Box::pin(async move {
                check(matches!(
                    service.lock_context_in_transaction(tx, &raw).await,
                    Err(Error::Unauthorized)
                ))
            })
        })
        .await?;
    }
    Ok(())
}
async fn owned<F>(real: bool, callback: F) -> Result<()>
where
    F: for<'a> FnOnce(&'a Db, i64) -> BoxFuture<'a, Result<()>>,
{
    let database = if real {
        let url = crate::test_database_environment::database_url().map_err(|_| {
            Error::custom("test_database_url_required", "真实数据库测试必须配置连接")
        })?;
        TestDatabase::create::<Migrator>(&url).await?
    } else {
        TestDatabase::sqlite_file::<Migrator>().await?
    };
    let db = Db::default();
    let outcome = async {
        db.initialize(database.connection()?)?;
        let backend = db.connection()?.get_database_backend();
        let users = db.quoted_table_name("users")?;
        let sql = if backend == DbBackend::Postgres {
            format!("INSERT INTO {users}(id,email,password_hash,created_at,updated_at) VALUES($1,$2,$3,$4,$5)")
        } else { format!("INSERT INTO {users}(id,email,password_hash,created_at,updated_at) VALUES(?,?,?,?,?)") };
        let now = crate::database::dialect::utc_value(backend,Utc::now());
        db.connection()?.execute_raw(Statement::from_sql_and_values(backend,sql,[42_i64.into(),"owned-refresh@example.test".into(),"unused".into(),now.clone(),now])).await.map_err(Error::internal)?;
        callback(&db,42).await
    }.await;
    drop(db);
    let cleanup = database.close().await;
    outcome?;
    cleanup
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_context_rotation_replay_and_strict_context() -> Result<()> {
    owned(false, |db, user| Box::pin(exercise(db, user))).await
}
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
#[tokio::test]
#[ignore = "需要真实数据库，创建和清理独占测试库"]
async fn real_context_rotation_replay_and_strict_context() -> Result<()> {
    owned(true, |db, user| Box::pin(exercise(db, user))).await
}

struct MigrationApp;
impl crate::foundation::ApplicationDefinition for MigrationApp {
    type Config = crate::config::NoConfig;
    fn default_providers() -> crate::support::DefaultProviders {
        crate::support::DefaultProviders::empty()
    }
    fn providers(_: &Self::Config) -> Vec<Box<dyn crate::support::ServiceProvider>> {
        vec![
            Box::new(crate::database::DatabaseServiceProvider),
            Box::new(crate::database::MigrationServiceProvider::<Migrator>::new()),
        ]
    }
}
async fn migrate(database: &TestDatabase, args: Vec<String>) -> Result<()> {
    let sources=figment::Figment::from(figment::providers::Serialized::defaults(crate::config::FrameworkConfig::default()))
        .merge(figment::providers::Serialized::defaults(serde_json::json!({"database":{"url":database.url(),"max_connections":2,"min_connections":0}})));
    crate::testing::TestApp::<MigrationApp>::builder(sources)
        .run_command("migrate", args)
        .await
}
async fn migration_contract(real: bool) -> Result<()> {
    let database = if real {
        let url = crate::test_database_environment::database_url().map_err(|_| {
            Error::custom("test_database_url_required", "真实数据库测试必须配置连接")
        })?;
        TestDatabase::create::<Migrator>(&url).await?
    } else {
        TestDatabase::sqlite_file::<Migrator>().await?
    };
    let db = Db::default();
    let outcome: Result<()> = async {
        db.initialize(database.connection()?)?;
        migrate(&database, vec!["down".into(), "1".into()]).await?;
        migrate(&database, vec![]).await?;
        let now = crate::database::dialect::utc_value(
            db.connection()?.get_database_backend(),
            Utc::now(),
        );
        db.table("users")?
            .insert(vec![
                ("id", 42_i64.into()),
                ("email", "migration@example.test".into()),
                ("password_hash", "unused".into()),
                ("created_at", now.clone()),
                ("updated_at", now.clone()),
            ])
            .await?;
        db.table("personal_access_tokens")?
            .insert(vec![
                ("user_id", 42_i64.into()),
                ("name", "owned".into()),
                ("token_prefix", "owned-prefix".into()),
                ("token_hash", "a".repeat(64).into()),
                ("abilities", serde_json::json!(["*"]).into()),
                ("created_at", now),
                ("factor_context", "{invalid-json".into()),
            ])
            .await?;
        check(
            migrate(&database, vec!["down".into(), "1".into()])
                .await
                .is_err(),
        )?;
        // 第二张表含坏JSON也必须保护；第一张表列尚在，不能先drop再发现失败。
        let issued = RefreshTokens::new(db.clone())
            .issue(42, Utc::now() + chrono::Duration::hours(1))
            .await?;
        db.table("auth_refresh_tokens")?
            .where_eq("token_hash", sha256_hex(issued.expose()))?
            .update(vec![("factor_context", "{also-invalid".into())])
            .await?;
        db.table("personal_access_tokens")?
            .update(vec![("factor_context", Option::<String>::None.into())])
            .await?;
        check(
            migrate(&database, vec!["down".into(), "1".into()])
                .await
                .is_err(),
        )?;
        db.table("auth_refresh_tokens")?
            .update(vec![("factor_context", Option::<String>::None.into())])
            .await?;
        migrate(&database, vec!["down".into(), "1".into()]).await?;
        migrate(&database, vec![]).await?;
        check(
            db.table("users")?
                .where_eq("id", 42)?
                .first()
                .await?
                .is_some(),
        )?;
        check(db.table("personal_access_tokens")?.first().await?.is_some())?;
        check(db.table("auth_refresh_tokens")?.first().await?.is_some())
    }
    .await;
    drop(db);
    let cleanup = database.close().await;
    outcome?;
    cleanup
}
#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_context_migration_protects_nonnull_assets_and_pristine_roundtrip() -> Result<()> {
    migration_contract(false).await
}
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
#[tokio::test]
#[ignore = "需要真实数据库，创建和清理独占测试库"]
async fn real_context_migration_protects_nonnull_assets_and_pristine_roundtrip() -> Result<()> {
    migration_contract(true).await
}

async fn single_rotation(db: &Db, raw: &str) -> Result<Option<ManagedRefreshRotation>> {
    let service = RefreshTokens::new(db.clone());
    let raw = raw.to_owned();
    db.transaction_for_update(|tx| {
        Box::pin(async move {
            let Some(locked) = service.lock_context_in_transaction(tx, &raw).await? else {
                return Ok(None);
            };
            service
                .rotate_context_in_transaction(locked)
                .await
                .map(Some)
        })
    })
    .await
}
async fn concurrency_contract(db: &Db, user_id: i64) -> Result<()> {
    let service = RefreshTokens::new(db.clone());
    let issue = db
        .transaction_for_update(|tx| {
            Box::pin(async move {
                service
                    .issue_context_in_transaction(
                        tx,
                        user_id,
                        Utc::now() + chrono::Duration::hours(1),
                        &context(user_id),
                    )
                    .await
            })
        })
        .await?;
    // 两个真实事务竞争相同token；只有一个插后继，失败方提交整家族撤销。
    let (a, b) = tokio::join!(
        single_rotation(db, issue.token.expose()),
        single_rotation(db, issue.token.expose())
    );
    let a = a?;
    let b = b?;
    check(usize::from(a.is_some()) + usize::from(b.is_some()) == 1)?;
    let family = issue.family_id;
    db.transaction_for_update(|tx| {
        let service = RefreshTokens::new(db.clone());
        Box::pin(async move {
            check(matches!(
                service
                    .validate_family_in_transaction(tx, user_id, &family, &context(user_id))
                    .await,
                Err(Error::Unauthorized)
            ))
        })
    })
    .await
}
#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_same_context_token_race_has_one_successor_and_committed_revoke() -> Result<()> {
    owned(false, |db, user| Box::pin(concurrency_contract(db, user))).await
}
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
#[tokio::test]
#[ignore = "需要真实数据库，创建和清理独占测试库"]
async fn real_same_context_token_race_has_one_successor_and_committed_revoke() -> Result<()> {
    owned(true, |db, user| Box::pin(concurrency_contract(db, user))).await
}

async fn family_shape_contract(db: &Db, user_id: i64) -> Result<()> {
    let service = RefreshTokens::new(db.clone());
    let expiry = Utc::now() + chrono::Duration::hours(1);
    let issued = db
        .transaction_for_update(|tx| {
            let service = service.clone();
            Box::pin(async move {
                service
                    .issue_context_in_transaction(tx, user_id, expiry, &context(user_id))
                    .await
            })
        })
        .await?;
    let family = issued.family_id;
    let first_hash = sha256_hex(issued.token.expose());
    db.transaction_for_update(|tx| {
        let service = service.clone();
        let family = family.clone();
        Box::pin(async move {
            insert_context_token(
                tx,
                &service.db,
                user_id,
                &family,
                &"b".repeat(64),
                expiry,
                &context(user_id).encode(user_id)?,
            )
            .await?;
            // 两个未消费有效leaf不能被当成同一家族的合法当前凭据。
            check(matches!(
                service
                    .validate_family_in_transaction(tx, user_id, &family, &context(user_id))
                    .await,
                Err(Error::Unauthorized)
            ))
        })
    })
    .await?;
    // 仅剩一个有效leaf，但祖先的期限被改短，同样拒绝，不能提升实际期限。
    let backend = db.connection()?.get_database_backend();
    let (a, b, c) = if backend == DbBackend::Postgres {
        ("$1", "$2", "$3")
    } else {
        ("?", "?", "?")
    };
    db.connection()?.execute_raw(Statement::from_sql_and_values(backend,
        crate::auth::resolve_auth_sql(db,&format!("UPDATE tb_auth_refresh_tokens SET consumed_at={a},expires_at={b} WHERE token_hash={c}"))?,
        [crate::database::dialect::utc_value(backend,Utc::now()),crate::database::dialect::utc_value(backend,expiry-chrono::Duration::minutes(5)),first_hash.clone().into()])).await.map_err(Error::internal)?;
    db.transaction_for_update(|tx| {
        let service = service.clone();
        let family = family.clone();
        Box::pin(async move {
            check(matches!(
                service
                    .validate_family_in_transaction(tx, user_id, &family, &context(user_id))
                    .await,
                Err(Error::Unauthorized)
            ))
        })
    })
    .await?;
    db.connection()?
        .execute_raw(Statement::from_sql_and_values(
            backend,
            crate::auth::resolve_auth_sql(
                db,
                &format!("UPDATE tb_auth_refresh_tokens SET expires_at={a} WHERE token_hash={b}"),
            )?,
            [
                crate::database::dialect::utc_value(backend, expiry),
                first_hash.into(),
            ],
        ))
        .await
        .map_err(Error::internal)?;
    db.transaction_for_update(|tx| {
        let service = service.clone();
        Box::pin(async move {
            let actual = service
                .validate_family_in_transaction(tx, user_id, &family, &context(user_id))
                .await?;
            check(actual.timestamp() == expiry.timestamp())
        })
    })
    .await
}
#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_family_requires_one_leaf_and_one_actual_deadline() -> Result<()> {
    owned(false, |db, user| Box::pin(family_shape_contract(db, user))).await
}
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
#[tokio::test]
#[ignore = "需要真实数据库，创建和清理独占测试库"]
async fn real_family_requires_one_leaf_and_one_actual_deadline() -> Result<()> {
    owned(true, |db, user| Box::pin(family_shape_contract(db, user))).await
}

#[cfg(feature = "db-sqlite")]
mod scope_isolation {
    use super::*;
    use crate::auth::credential::CredentialGuardBinding;

    fn api_scope() -> CredentialGuardBinding {
        CredentialGuardBinding {
            guard: "api".into(),
            provider: "users".into(),
            issuer: "https://issuer.example".into(),
            audience: "mobile".into(),
        }
    }

    async fn issue(
        db: &Db,
        user_id: i64,
        scope: Option<&CredentialGuardBinding>,
    ) -> Result<ManagedRefreshIssue> {
        let service = RefreshTokens::new(db.clone());
        let mut context = context(user_id);
        context.scope = scope.cloned();
        db.transaction_for_update(move |tx| {
            Box::pin(async move {
                service
                    .issue_context_in_transaction(
                        tx,
                        user_id,
                        Utc::now() + chrono::Duration::hours(1),
                        &context,
                    )
                    .await
            })
        })
        .await
    }

    async fn rotate(
        db: &Db,
        raw: &str,
        scope: &CredentialGuardBinding,
    ) -> Result<Option<ManagedRefreshRotation>> {
        let service = RefreshTokens::new(db.clone());
        let scope = scope.clone();
        let raw = raw.to_owned();
        db.transaction_for_update(move |tx| {
            Box::pin(async move {
                let Some(locked) = service
                    .lock_context_in_scope_in_transaction(tx, &raw, Some(&scope))
                    .await?
                else {
                    return Ok(None);
                };
                service
                    .rotate_context_in_transaction(locked)
                    .await
                    .map(Some)
            })
        })
        .await
    }

    async fn unrevoked_rows(db: &Db, family: &str) -> Result<u64> {
        db.table("auth_refresh_tokens")?
            .where_eq("family_id", family)?
            .where_null("revoked_at")?
            .count()
            .await
    }

    async fn family_deadline(
        db: &Db,
        user_id: i64,
        family: &str,
        scope: &CredentialGuardBinding,
    ) -> Result<DateTime<Utc>> {
        let service = RefreshTokens::new(db.clone());
        let family = family.to_owned();
        let mut expected = context(user_id);
        expected.scope = Some(scope.clone());
        db.transaction_for_update(move |tx| {
            Box::pin(async move {
                service
                    .validate_family_in_transaction(tx, user_id, &family, &expected)
                    .await
            })
        })
        .await
    }

    #[tokio::test]
    async fn foreign_refresh_and_logout_cannot_revoke_or_rotate_another_scoped_family() -> Result<()>
    {
        owned(false, |db, user_id| {
            Box::pin(async move {
                let service = RefreshTokens::new(db.clone());
                let owner = api_scope();
                let issued = issue(db, user_id, Some(&owner)).await?;
                let original = issued.token.expose();
                let mut leaf = rotate(db, original, &owner)
                    .await?
                    .ok_or(Error::Unauthorized)?;
                let family = &issued.family_id;

                for foreign in [
                    CredentialGuardBinding {
                        guard: "staff".into(),
                        ..owner.clone()
                    },
                    CredentialGuardBinding {
                        provider: "staff".into(),
                        ..owner.clone()
                    },
                    CredentialGuardBinding {
                        issuer: "https://other.example".into(),
                        ..owner.clone()
                    },
                    CredentialGuardBinding {
                        audience: "desktop".into(),
                        ..owner.clone()
                    },
                ] {
                    for raw in [original, leaf.token.expose()] {
                        // Err 必须穿过真实事务边界；不能在事务内捕获后把副作用提交。
                        assert!(matches!(
                            rotate(db, raw, &foreign).await,
                            Err(Error::Unauthorized)
                        ));
                        assert!(matches!(
                            service.revoke_scoped(raw, &foreign).await,
                            Err(Error::Unauthorized)
                        ));
                    }
                    leaf = rotate(db, leaf.token.expose(), &owner)
                        .await?
                        .ok_or(Error::Unauthorized)?;
                    assert!(leaf.family_id.eq(family));
                    assert_eq!(
                        family_deadline(db, user_id, family, &owner).await?,
                        leaf.expires_at
                    );
                }

                for raw in [original, leaf.token.expose()] {
                    assert!(matches!(
                        single_rotation(db, raw).await,
                        Err(Error::Unauthorized)
                    ));
                }
                leaf = rotate(db, leaf.token.expose(), &owner)
                    .await?
                    .ok_or(Error::Unauthorized)?;
                assert!(leaf.family_id.eq(family));
                assert_eq!(unrevoked_rows(db, family).await?, 7);

                // 同域重放仍返回 Ok(None)，整族撤销必须已提交，独立查询和后继操作均能观察到。
                assert!(rotate(db, original, &owner).await?.is_none());
                assert_eq!(unrevoked_rows(db, family).await?, 0);
                assert!(matches!(
                    family_deadline(db, user_id, family, &owner).await,
                    Err(Error::Unauthorized)
                ));
                assert!(rotate(db, leaf.token.expose(), &owner).await?.is_none());
                Ok(())
            })
        })
        .await
    }

    #[tokio::test]
    async fn scoped_refresh_and_logout_cannot_mutate_unscoped_family() -> Result<()> {
        owned(false, |db, user_id| {
            Box::pin(async move {
                let service = RefreshTokens::new(db.clone());
                let typed = api_scope();
                let issued = issue(db, user_id, None).await?;
                let original = issued.token.expose();
                let leaf = single_rotation(db, original)
                    .await?
                    .ok_or(Error::Unauthorized)?;
                for raw in [original, leaf.token.expose()] {
                    assert!(matches!(
                        rotate(db, raw, &typed).await,
                        Err(Error::Unauthorized)
                    ));
                    assert!(matches!(
                        service.revoke_scoped(raw, &typed).await,
                        Err(Error::Unauthorized)
                    ));
                }
                let successor = single_rotation(db, leaf.token.expose())
                    .await?
                    .ok_or(Error::Unauthorized)?;
                assert!(successor.family_id.eq(&issued.family_id));
                assert_eq!(unrevoked_rows(db, &issued.family_id).await?, 3);
                assert!(single_rotation(db, original).await?.is_none());
                assert_eq!(unrevoked_rows(db, &issued.family_id).await?, 0);
                Ok(())
            })
        })
        .await
    }

    #[tokio::test]
    async fn corrupt_consumed_context_does_not_turn_a_scoped_family_into_an_unscoped_one()
    -> Result<()> {
        owned(false, |db, user_id| {
            Box::pin(async move {
                let owner = api_scope();
                let issued = issue(db, user_id, Some(&owner)).await?;
                let original = issued.token.expose();
                let leaf = rotate(db, original, &owner)
                    .await?
                    .ok_or(Error::Unauthorized)?;
                let changed = db
                    .table("auth_refresh_tokens")?
                    .where_eq("token_hash", sha256_hex(original))?
                    .update(vec![(
                        "factor_context",
                        "{corrupt-after-consumption".into(),
                    )])
                    .await?;
                assert_eq!(changed, 1);
                assert!(matches!(
                    single_rotation(db, original).await,
                    Err(Error::Unauthorized)
                ));
                let foreign = CredentialGuardBinding {
                    audience: "desktop".into(),
                    ..owner
                };
                let service = RefreshTokens::new(db.clone());
                assert!(matches!(
                    service.revoke_scoped(original, &foreign).await,
                    Err(Error::Unauthorized)
                ));
                assert!(matches!(
                    rotate(db, original, &foreign).await,
                    Err(Error::Unauthorized)
                ));
                assert_eq!(unrevoked_rows(db, &leaf.family_id).await?, 2);
                Ok(())
            })
        })
        .await
    }
}
