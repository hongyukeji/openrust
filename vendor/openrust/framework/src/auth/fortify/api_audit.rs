//! 显式 API 的双因素登录与管理事务审计。
use crate::Error;
use crate::Result;
use crate::audit::Audit;
use crate::audit::AuditEvent;
use crate::audit::AuditOutcome;
use crate::audit::AuditSubject;
use crate::audit::DatabaseAuditSink;
use crate::auth::flows::ApiLoginAuditSink;
use crate::database::DatabaseTransaction;
use crate::database::Db;
use crate::support::BoxFuture;

/// 在受控 API 因子管理动作的原数据库事务内记录成功事实。
pub trait ApiFactorManagementAuditSink: Send + Sync {
    /// 核验账户库与审计目标能参与同一事务。
    fn require_management_store(&self, db: &Db) -> Result<()>;

    /// 必需审计失败时向调用方返回错误，由账户事务统一回滚。
    fn record_management_in_transaction<'a>(
        &'a self,
        transaction: &'a DatabaseTransaction,
        user_id: i64,
        mutation: super::FactorAuthorityMutation,
    ) -> BoxFuture<'a, Result<()>>;
}

/// 复用通用审计脱敏策略，并在凭据消费的同一数据库事务中记录成功事实。
///
/// 应用可选择登录 [`ApiLoginAuditSink`]、管理 [`ApiFactorManagementAuditSink`] 或两者，并安装共同的审计表。
#[derive(Clone)]
pub struct DatabaseApiLoginAudit {
    db: Db,
    audit: Audit,
    sink: DatabaseAuditSink,
}
impl std::fmt::Debug for DatabaseApiLoginAudit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DatabaseApiLoginAudit")
    }
}
impl DatabaseApiLoginAudit {
    /// 使用同数据库目标和应用已经配置的通用审计脱敏策略。
    #[must_use]
    pub fn new(db: Db, audit: Audit) -> Self {
        Self {
            sink: DatabaseAuditSink::new(db.clone()),
            db,
            audit,
        }
    }

    async fn record_action(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
        action: &str,
    ) -> Result<()> {
        if user_id <= 0 {
            return Err(Error::Unauthorized);
        }
        let subject = AuditSubject::new("user", user_id.to_string())?;
        let event =
            AuditEvent::new(action, subject.clone(), AuditOutcome::Succeeded)?.actor(subject);
        self.sink
            .record_in_transaction(transaction, &self.audit.prepare(event)?)
            .await
    }
}
impl ApiFactorManagementAuditSink for DatabaseApiLoginAudit {
    fn require_management_store(&self, db: &Db) -> Result<()> {
        self.require_store(db)
    }

    fn record_management_in_transaction<'a>(
        &'a self,
        transaction: &'a DatabaseTransaction,
        user_id: i64,
        mutation: super::FactorAuthorityMutation,
    ) -> BoxFuture<'a, Result<()>> {
        use super::FactorAuthorityMutation;
        let action = match mutation {
            FactorAuthorityMutation::Enable => "auth.two_factor.enable",
            FactorAuthorityMutation::Confirm => "auth.two_factor.confirm",
            FactorAuthorityMutation::Regenerate => "auth.two_factor.regenerate",
            FactorAuthorityMutation::Disable => "auth.two_factor.disable",
        };
        Box::pin(self.record_action(transaction, user_id, action))
    }
}
impl ApiLoginAuditSink for DatabaseApiLoginAudit {
    fn require_store(&self, db: &Db) -> Result<()> {
        if !self.db.same_handle(db)
            || self.db.table_name("audit_records")? != db.table_name("audit_records")?
        {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }
    fn record_in_transaction<'a>(
        &'a self,
        transaction: &'a DatabaseTransaction,
        user_id: i64,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(self.record_action(transaction, user_id, "auth.two_factor.login"))
    }
}

#[cfg(all(test, feature = "db-sqlite"))]
mod tests {
    use super::*;
    use crate::{
        audit::{AuditPolicy, DatabaseAuditMigration},
        database::sea_orm::ConnectionTrait as _,
        database::sea_orm_migration::{MigrationTrait, MigratorTrait},
    };
    struct Migrator;
    impl MigratorTrait for Migrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            vec![Box::new(DatabaseAuditMigration)]
        }
    }
    async fn commit_candidate(db: &Db, audit: DatabaseApiLoginAudit) -> Result<()> {
        let candidate = db.clone();
        db.transaction_for_update(move |transaction| {
            Box::pin(async move {
                candidate
                    .table("audit_candidate")?
                    .using_transaction(transaction)
                    .insert(vec![("id", 1_i64.into())])
                    .await?;
                audit.record_in_transaction(transaction, 42).await
            })
        })
        .await
    }
    #[tokio::test]
    async fn explicit_database_audit_failure_rolls_back_same_transaction_candidate() -> Result<()> {
        let database = crate::testing::TestDatabase::sqlite_file::<Migrator>().await?;
        let db = Db::default();
        let outcome: Result<()> = async {
            db.initialize(database.connection()?)?;
            db.connection()?.execute_unprepared("CREATE TABLE audit_candidate (id INTEGER PRIMARY KEY)").await.map_err(Error::internal)?;
            let audit = DatabaseApiLoginAudit::new(db.clone(), Audit::new(DatabaseAuditSink::new(db.clone()), AuditPolicy::default()));
            audit.require_store(&db)?;
            assert!(audit.require_store(&Db::default()).is_err());
            let table = db.quoted_table_name("audit_records")?;
            db.connection()?.execute_unprepared(&format!("CREATE TRIGGER reject_two_factor_audit BEFORE INSERT ON {table} WHEN NEW.action='auth.two_factor.login' BEGIN SELECT RAISE(ABORT,'controlled audit failure'); END")).await.map_err(Error::internal)?;
            assert!(commit_candidate(&db, audit.clone()).await.is_err());
            assert_eq!(db.table("audit_candidate")?.count().await?, 0);
            assert_eq!(db.table("audit_records")?.count().await?, 0);
            db.connection()?.execute_unprepared("DROP TRIGGER reject_two_factor_audit").await.map_err(Error::internal)?;
            commit_candidate(&db, audit).await?;
            assert_eq!(db.table("audit_candidate")?.count().await?, 1);
            let row = db.table("audit_records")?.first().await?.ok_or(Error::Unauthorized)?;
            assert_eq!(row.try_get::<String>("", "action").map_err(Error::internal)?, "auth.two_factor.login");
            assert_eq!(row.try_get::<String>("", "details").map_err(Error::internal)?, "{}");
            Ok(())
        }.await;
        drop(db);
        let closed = database.close().await;
        outcome?;
        closed
    }
}
