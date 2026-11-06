//! 受控 JWT 管理事件及审计必须与密钥、代际、管理绑定和验证码消费共同提交。
use super::*;
#[cfg(feature = "db-sqlite")]
use crate::auth::fortify::{
    RecoveryCodesGenerated, TwoFactorAuthenticationConfirmed, TwoFactorAuthenticationDisabled,
};
use crate::{
    audit::{Audit, AuditPolicy, DatabaseAuditSink},
    auth::fortify::{
        DatabaseApiLoginAudit, FactorAuthoritySnapshot, TwoFactorAuthenticationEnabled,
    },
    db::sea_orm::ConnectionTrait,
};
#[cfg(feature = "db-sqlite")]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use tokio::sync::Notify;

#[cfg(feature = "db-sqlite")]
async fn events(provider: EventsServiceProvider) -> Result<Events> {
    let mut builder = ProviderBuilder::new();
    builder.provider(provider);
    let runtime = builder.start(figment::Figment::new()).await?;
    Ok((*runtime.container().resolve::<Events>().await?).clone())
}

fn audit(db: &Db) -> Arc<DatabaseApiLoginAudit> {
    Arc::new(DatabaseApiLoginAudit::new(
        db.clone(),
        Audit::new(DatabaseAuditSink::new(db.clone()), AuditPolicy::default()),
    ))
}

async fn snapshot(fixture: &Fixture) -> Result<(FactorAuthoritySnapshot, Option<String>)> {
    let user = fixture.manager.user(42).await?;
    let authority = user
        .as_two_factor()
        .and_then(TwoFactorAuthenticatable::two_factor_authority)
        .ok_or(Error::Unauthorized)?;
    let snapshot = authority.load().await?;
    let row = fixture
        .db
        .table("users")?
        .where_eq("id", 42_i64)?
        .first()
        .await?
        .ok_or(Error::Unauthorized)?;
    let consumed = row
        .try_get("", "two_factor_consumed_codes")
        .map_err(Error::internal)?;
    Ok((snapshot, consumed))
}

async fn grant(fixture: &Fixture) -> Result<String> {
    fixture
        .manager
        .prepare(&password_login(fixture, [192, 0, 2, 1]).await?, PASSWORD)
        .await
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn cancelled_enable_listener_rolls_back_pending_state_and_continuation() -> Result<()> {
    owned(true, |mut fixture| {
        Box::pin(async move {
            let entered = Arc::new(Notify::new());
            let pending = entered.clone();
            let mut listeners = EventsServiceProvider::new();
            listeners.listen::<TwoFactorAuthenticationEnabled, _>(move |_, context| {
                let pending = pending.clone();
                Box::pin(async move {
                    if context.transaction().is_none() {
                        return Err(Error::Unauthorized);
                    }
                    pending.notify_one();
                    std::future::pending::<Result<()>>().await
                })
            });
            fixture.manager.actions.events = events(listeners).await?;
            fixture.manager.actions = fixture
                .manager
                .actions
                .clone()
                .with_api_management_audit(audit(&fixture.db));
            let original = grant(&fixture).await?;
            let before = snapshot(&fixture).await?;
            let (user, actions) = fixture.manager.actions(&original).await?;
            {
                let attempt = actions.enable(&user, false);
                tokio::pin!(attempt);
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    tokio::select! {
                        () = entered.notified() => Ok(()),
                        result = &mut attempt => result.and(Err(Error::Unauthorized)),
                    }
                })
                .await
                .map_err(Error::internal)??;
            }
            assert_eq!(snapshot(&fixture).await?, before);
            assert_eq!(fixture.db.table("audit_records")?.count().await?, 0);
            assert!(fixture.manager.actions(&original).await.is_ok());
            Ok(())
        })
    })
    .await
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn failed_listener_rolls_back_api_enable_while_web_event_observes_committed_state()
-> Result<()> {
    owned(true, |mut fixture| {
        Box::pin(async move {
            let transaction_calls = Arc::new(AtomicUsize::new(0));
            let committed_calls = Arc::new(AtomicUsize::new(0));
            let on_transaction = transaction_calls.clone();
            let after_commit = committed_calls.clone();
            let db = fixture.db.clone();
            let mut listeners = EventsServiceProvider::new();
            listeners.listen::<TwoFactorAuthenticationEnabled, _>(move |event, ctx| {
                let db = db.clone();
                let on_transaction = on_transaction.clone();
                let after_commit = after_commit.clone();
                Box::pin(async move {
                    let q = db.table("users")?.where_eq(
                        "id",
                        event
                            .user
                            .auth_identifier()?
                            .as_i64()
                            .ok_or(Error::Unauthorized)?,
                    )?;
                    let row = if let Some(tx) = ctx.transaction() {
                        on_transaction.fetch_add(1, Ordering::SeqCst);
                        q.using_transaction(tx).first().await?
                    } else {
                        after_commit.fetch_add(1, Ordering::SeqCst);
                        q.first().await?
                    }
                    .ok_or(Error::Unauthorized)?;
                    let secret: Option<String> = row
                        .try_get("", "two_factor_secret")
                        .map_err(Error::internal)?;
                    assert!(secret.is_some());
                    Err(Error::custom(
                        "test_factor_listener_rejected",
                        "测试监听器拒绝变更",
                    ))
                })
            });
            fixture.manager.actions.events = events(listeners).await?;
            fixture.manager.actions = fixture
                .manager
                .actions
                .clone()
                .with_api_management_audit(audit(&fixture.db));
            let original = grant(&fixture).await?;
            let before = snapshot(&fixture).await?;
            let (user, api_actions) = fixture.manager.actions(&original).await?;
            let failed = api_actions.enable(&user, false).await;
            assert!(failed.is_err_and(|e| e.code() == "test_factor_listener_rejected"));
            assert_eq!(snapshot(&fixture).await?, before);
            assert_eq!(transaction_calls.load(Ordering::SeqCst), 1);
            assert_eq!(committed_calls.load(Ordering::SeqCst), 0);

            // Web 的普通同一动作在事务提交后才调用监听器。
            let failed = fixture.manager.actions.enable(&user, false).await;
            assert!(failed.is_err_and(|e| e.code() == "test_factor_listener_rejected"));
            assert!(snapshot(&fixture).await?.0.state.secret.is_some());
            assert_eq!(transaction_calls.load(Ordering::SeqCst), 1);
            assert_eq!(committed_calls.load(Ordering::SeqCst), 1);
            assert_eq!(fixture.db.table("audit_records")?.count().await?, 0);
            Ok(())
        })
    })
    .await
}

#[cfg(feature = "db-sqlite")]
fn count_event<E: Send + Sync + 'static>(
    provider: &mut EventsServiceProvider,
    db: &Db,
    name: &'static str,
) {
    let db = db.clone();
    provider.listen::<E, _>(move |_, ctx| {
        let db = db.clone();
        Box::pin(async move {
            let tx = ctx.transaction().ok_or(Error::Unauthorized)?;
            db.table("factor_test_events")?
                .using_transaction(tx)
                .insert(vec![("name", name.into())])
                .await?;
            Ok(())
        })
    });
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn database_audit_failure_rolls_back_event_nonce_and_mutation_and_retry_records_once()
-> Result<()> {
    owned(true, |fixture| Box::pin(exercise_management_audit(fixture))).await
}

#[cfg(feature = "db-sqlite")]
async fn exercise_management_audit(mut fixture: Fixture) -> Result<()> {
    let db = &fixture.db;
    let probe = db.quoted_table_name("factor_test_events")?;
    db.connection()?
        .execute_unprepared(&format!("CREATE TABLE {probe} (name TEXT NOT NULL)"))
        .await
        .map_err(Error::internal)?;
    let mut listeners = EventsServiceProvider::new();
    count_event::<TwoFactorAuthenticationEnabled>(&mut listeners, db, "enable");
    count_event::<TwoFactorAuthenticationConfirmed>(&mut listeners, db, "confirm");
    count_event::<RecoveryCodesGenerated>(&mut listeners, db, "regenerate");
    count_event::<TwoFactorAuthenticationDisabled>(&mut listeners, db, "disable");
    fixture.manager.actions.events = events(listeners).await?;
    fixture.manager.actions = fixture
        .manager
        .actions
        .clone()
        .with_api_management_audit(audit(db));
    let original = grant(&fixture).await?;
    let (user, actions) = fixture.manager.actions(&original).await?;
    actions.enable(&user, false).await?;
    actions.generate_new_recovery_codes(&user).await?;
    actions.disable(&user).await?;

    let pending = grant(&fixture).await?;
    let (user, actions) = fixture.manager.actions(&pending).await?;
    actions.enable(&user, false).await?;
    let secret = actions.secret_key(&user).await?;
    let code = crate::auth::totp::totp_code(
        &crate::auth::fortify::engine::decode_secret(&secret)?,
        100,
        6,
    )?;
    let before = snapshot(&fixture).await?;
    assert!(before.1.is_none());
    let audit_table = db.quoted_table_name("audit_records")?;
    db.connection()?.execute_unprepared(&format!("CREATE TRIGGER reject_confirm_audit BEFORE INSERT ON {audit_table} WHEN NEW.action='auth.two_factor.confirm' BEGIN SELECT RAISE(ABORT,'confirm audit rejected'); END")).await.map_err(Error::internal)?;
    assert!(actions.confirm(&user, &code).await.is_err());
    assert_eq!(snapshot(&fixture).await?, before);
    assert_eq!(
        db.table("factor_test_events")?
            .where_eq("name", "confirm")?
            .count()
            .await?,
        0
    );
    assert_eq!(
        db.table("audit_records")?
            .where_eq("action", "auth.two_factor.confirm")?
            .count()
            .await?,
        0
    );
    db.connection()?
        .execute_unprepared("DROP TRIGGER reject_confirm_audit")
        .await
        .map_err(Error::internal)?;

    let (user, actions) = fixture.manager.actions(&pending).await?;
    actions.confirm(&user, &code).await?;
    assert!(snapshot(&fixture).await?.1.is_some());
    assert!(fixture.manager.actions(&pending).await.is_err());
    for (name, count) in [
        ("enable", 2),
        ("confirm", 1),
        ("regenerate", 1),
        ("disable", 1),
    ] {
        assert_eq!(
            db.table("factor_test_events")?
                .where_eq("name", name)?
                .count()
                .await?,
            count
        );
        assert_eq!(
            db.table("audit_records")?
                .where_eq("action", format!("auth.two_factor.{name}"))?
                .count()
                .await?,
            count
        );
    }
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn password_change_by_transaction_listener_invalidates_grant_before_commit() -> Result<()> {
    owned(true, |mut fixture| {
        Box::pin(async move {
            let db = fixture.db.clone();
            let mut listeners = EventsServiceProvider::new();
            listeners.listen::<TwoFactorAuthenticationEnabled, _>(move |_, ctx| {
                let db = db.clone();
                Box::pin(async move {
                    let tx = ctx.transaction().ok_or(Error::Unauthorized)?;
                    let updated = db
                        .table("users")?
                        .using_transaction(tx)
                        .where_eq("id", 42_i64)?
                        .update(vec![("password_hash", "changed-during-listener".into())])
                        .await?;
                    assert_eq!(updated, 1);
                    Ok(())
                })
            });
            fixture.manager.actions.events = events(listeners).await?;
            fixture.manager.actions = fixture
                .manager
                .actions
                .clone()
                .with_api_management_audit(audit(&fixture.db));
            let original = grant(&fixture).await?;
            let before = snapshot(&fixture).await?;
            let (user, actions) = fixture.manager.actions(&original).await?;
            assert!(matches!(
                actions.enable(&user, false).await,
                Err(Error::Unauthorized)
            ));
            assert_eq!(snapshot(&fixture).await?, before);
            assert_eq!(fixture.db.table("audit_records")?.count().await?, 0);
            assert!(fixture.manager.actions(&original).await.is_ok());
            Ok(())
        })
    })
    .await
}

// 每个监听器只观察本测试私有的库。修改密码发生在业务事件和审计均已成功之后。
#[derive(Clone, Default)]
struct CommitGate {
    armed: Arc<std::sync::atomic::AtomicBool>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

impl CommitGate {
    fn listeners(&self, durable_table: &'static str) -> EventsServiceProvider {
        let mut listeners = EventsServiceProvider::new();
        let gate = self.clone();
        listeners.listen::<crate::db::TransactionBeforeCommit, _>(move |_, ctx| {
            let gate = gate.clone();
            Box::pin(async move {
                if !gate.armed.load(Ordering::SeqCst) {
                    return Ok(());
                }
                let tx = ctx.transaction().ok_or(Error::Unauthorized)?;
                let statement = |sql: String| {
                    crate::db::sea_orm::Statement::from_string(tx.get_database_backend(), sql)
                };
                let audit = statement(
                    "SELECT 1 FROM audit_records WHERE action='auth.two_factor.enable'".into(),
                );
                if tx
                    .query_one_raw(audit)
                    .await
                    .map_err(Error::internal)?
                    .is_none()
                {
                    return Ok(());
                }
                let rows = tx
                    .query_all_raw(statement(format!("SELECT 1 FROM {durable_table} LIMIT 2")))
                    .await
                    .map_err(Error::internal)?;
                assert_eq!(rows.len(), 1);
                gate.armed.store(false, Ordering::SeqCst);
                gate.entered.notify_one();
                gate.release.notified().await;
                let changed = tx
                    .execute_unprepared(
                        "UPDATE users SET password_hash='changed-by-global-listener' WHERE id=42",
                    )
                    .await
                    .map_err(Error::internal)?;
                assert_eq!(changed.rows_affected(), 1);
                Ok(())
            })
        });
        listeners
    }

    async fn verify_rollback(
        self,
        mut fixture: Fixture,
        durable_table: &'static str,
    ) -> Result<()> {
        fixture.manager.actions = fixture
            .manager
            .actions
            .clone()
            .with_api_management_audit(audit(&fixture.db));
        let raw = grant(&fixture).await?;
        let before = snapshot(&fixture).await?;
        let (user, actions) = fixture.manager.actions(&raw).await?;
        self.armed.store(true, Ordering::SeqCst);
        let release = async {
            self.entered.notified().await;
            self.release.notify_one();
        };
        let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::join!(actions.enable(&user, false), release)
        })
        .await
        .map_err(Error::internal)?;
        assert!(matches!(result, Err(Error::Unauthorized)));
        // 新事务之前独立读取持久表，确认观察的是回滚后的结果。
        assert_eq!(fixture.db.table("audit_records")?.count().await?, 0);
        assert_eq!(fixture.db.table(durable_table)?.count().await?, 0);
        assert_eq!(snapshot(&fixture).await?, before);
        assert!(fixture.manager.actions(&raw).await.is_ok());
        Ok(())
    }
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_global_listener_after_enable_rolls_back_factor_continuation_audit_and_event()
-> Result<()> {
    let gate = CommitGate::default();
    let mut listeners = gate.listeners("factor_commit_events");
    listeners.listen::<TwoFactorAuthenticationEnabled, _>(|_, ctx| {
        Box::pin(async move {
            ctx.transaction()
                .ok_or(Error::Unauthorized)?
                .execute_unprepared("INSERT INTO factor_commit_events (mark) VALUES (1)")
                .await
                .map_err(Error::internal)?;
            Ok(())
        })
    });
    owned_with_events(true, listeners, move |fixture| {
        Box::pin(async move {
            fixture
                .db
                .connection()?
                .execute_unprepared("CREATE TABLE factor_commit_events (mark INTEGER NOT NULL)")
                .await
                .map_err(Error::internal)?;
            Box::pin(gate.verify_rollback(fixture, "factor_commit_events")).await
        })
    })
    .await
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_global_listener_change_during_management_read_rolls_back_and_rejects() -> Result<()>
{
    let change = Arc::new(AtomicUsize::new(0));
    let selected = change.clone();
    let mut listeners = EventsServiceProvider::new();
    listeners.listen::<crate::db::TransactionBeforeCommit, _>(move |_, ctx| {
        let selected = selected.clone();
        Box::pin(async move {
            let sql = match selected.swap(0, Ordering::SeqCst) {
                1 => "UPDATE users SET password_hash='changed-before-read-commit' WHERE id=42",
                2 => "UPDATE users SET two_factor_secret=NULL, two_factor_recovery_codes=NULL WHERE id=42",
                _ => return Ok(()),
            };
            let changed = ctx.transaction().ok_or(Error::Unauthorized)?
                .execute_unprepared(sql).await.map_err(Error::internal)?;
            assert_eq!(changed.rows_affected(), 1);
            Ok(())
        })
    });
    owned_with_events(true, listeners, move |fixture| {
        Box::pin(async move {
            let raw = grant(&fixture).await?;
            let (user, actions) = fixture.manager.actions(&raw).await?;
            actions.enable(&user, false).await?;

            let account = user
                .as_two_factor()
                .and_then(TwoFactorAuthenticatable::two_factor_authority)
                .ok_or(Error::Unauthorized)?;
            let before = account.load().await?;
            let context = fixture.manager.decode(&raw)?.management_context()?;
            change.store(1, Ordering::SeqCst);
            assert!(matches!(
                account.authorize_management(&before, &context, true).await,
                Err(Error::Unauthorized)
            ));
            change.store(2, Ordering::SeqCst);
            assert!(matches!(
                account.authorize_management(&before, &context, true).await,
                Err(Error::Conflict(_))
            ));
            assert_eq!(change.load(Ordering::SeqCst), 0);
            assert_eq!(account.load().await?, before);
            assert!(fixture.manager.actions(&raw).await.is_ok());
            Ok(())
        })
    })
    .await
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_global_listener_cannot_release_management_material_after_grant_expiry() -> Result<()>
{
    let expires_at = Arc::new(std::sync::atomic::AtomicI64::new(0));
    let delayed = expires_at.clone();
    let mut listeners = EventsServiceProvider::new();
    listeners.listen::<crate::db::TransactionBeforeCommit, _>(move |_, _| {
        let delayed = delayed.clone();
        Box::pin(async move {
            let cutoff = delayed.swap(0, Ordering::SeqCst);
            if cutoff > 0 {
                let cutoff = chrono::DateTime::<Utc>::from_timestamp(cutoff, 0)
                    .ok_or(Error::Unauthorized)?;
                let remaining = (cutoff - Utc::now()).to_std().unwrap_or_default();
                tokio::time::sleep(remaining + std::time::Duration::from_millis(10)).await;
            }
            Ok(())
        })
    });
    owned_with_events(true, listeners, move |fixture| {
        Box::pin(async move {
            let raw = grant(&fixture).await?;
            let (user, actions) = fixture.manager.actions(&raw).await?;
            actions.enable(&user, false).await?;
            let account = user
                .as_two_factor()
                .and_then(TwoFactorAuthenticatable::two_factor_authority)
                .ok_or(Error::Unauthorized)?;
            let before = account.load().await?;
            let mut short = fixture.manager.decode(&raw)?;
            short.expires_at = Utc::now().timestamp() + 2;
            let context = fixture
                .manager
                .decode(&encode(&fixture.manager, &short)?)?
                .management_context()?;
            expires_at.store(short.expires_at, Ordering::SeqCst);
            assert!(matches!(
                account.authorize_management(&before, &context, true).await,
                Err(Error::Unauthorized)
            ));
            assert_eq!(expires_at.load(Ordering::SeqCst), 0);
            assert!(fixture.manager.actions(&raw).await.is_ok());
            Ok(())
        })
    })
    .await
}

#[cfg(all(feature = "queue", any(feature = "db-mysql", feature = "db-postgres")))]
mod queued {
    use super::*;
    use crate::{
        db::DatabaseBackend,
        queue::{Job, JobContext, QueueMigration, QueueServiceProvider},
    };

    struct QueueMigrator;
    impl MigratorTrait for QueueMigrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            let mut migrations = Migrator::migrations();
            migrations.push(Box::new(QueueMigration));
            migrations
        }
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    struct EnrollmentNotice {
        user_id: i64,
    }
    impl Job for EnrollmentNotice {
        const NAME: &'static str = "test.verified-enrollment";
        fn handle(&self, _: JobContext) -> BoxFuture<'_, Result<()>> {
            Box::pin(async { Ok(()) })
        }
    }

    async fn queued_rollback(expected: DatabaseBackend) -> Result<()> {
        let url = std::env::var("OPENRUST_TEST_DATABASE_URL").map_err(|_| {
            Error::custom(
                "test_database_url_required",
                "需提供目标后端的独占测试库连接",
            )
        })?;
        if DatabaseBackend::from_url(&url)? != expected {
            return Err(Error::custom(
                "test_database_backend_mismatch",
                "测试连接与目标后端不符",
            ));
        }
        let database = TestDatabase::create::<QueueMigrator>(&url).await?;
        let gate = CommitGate::default();
        let mut listeners = gate.listeners("jobs");
        listeners.listen_queued::<TwoFactorAuthenticationEnabled, EnrollmentNotice, _>(
            "default",
            |event| {
                Ok(EnrollmentNotice {
                    user_id: event
                        .user
                        .auth_identifier()?
                        .as_i64()
                        .ok_or(Error::Unauthorized)?,
                })
            },
        );
        let mut builder = ProviderBuilder::new();
        let queues = QueueServiceProvider::new();
        queues.contributions().register_job::<EnrollmentNotice>()?;
        builder.provider(queues);
        owned_database(database, true, listeners, builder, move |fixture| {
            Box::pin(gate.verify_rollback(fixture, "jobs"))
        })
        .await
    }

    #[cfg(feature = "db-mysql")]
    #[tokio::test]
    #[ignore = "需要真实 MySQL 独占临时库与原生数据库队列"]
    async fn mysql_global_listener_rejection_rolls_back_queued_enrollment() -> Result<()> {
        queued_rollback(DatabaseBackend::MySql).await
    }

    #[cfg(feature = "db-postgres")]
    #[tokio::test]
    #[ignore = "需要真实 PostgreSQL 独占临时库与原生数据库队列"]
    async fn postgres_global_listener_rejection_rolls_back_queued_enrollment() -> Result<()> {
        queued_rollback(DatabaseBackend::PostgreSql).await
    }
}
