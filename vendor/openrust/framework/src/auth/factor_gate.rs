//! 未选择因子证明策略的凭据入口在同事务内检查真实现代因子资产。

#[cfg(all(test, any(feature = "db-mysql", feature = "db-postgres")))]
#[path = "factor_gate/real_server_tests.rs"]
mod real_server_tests;

use crate::db::{
    Db,
    sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, Statement},
};
use crate::{Error, Result};

// 外层Option区分真实缺列与存在但NULL；元数据不可见不能证明未安装。
async fn factor_column(
    db: &Db,
    transaction: &DatabaseTransaction,
    user_id: i64,
    column: &str,
) -> Result<Option<Option<String>>> {
    use crate::db::sea_orm::TransactionTrait;
    let backend = transaction.get_database_backend();
    let table = db.quoted_table_name("users")?;
    if backend == DbBackend::Sqlite {
        // table_xinfo自SQLite3.26已提供，并包含table_info遗漏的可SELECT generated/hidden列。
        // 标识符忽略ASCII大小写，元数据比较必须与随后SELECT的实际解析一致。
        let columns = transaction
            .query_all_raw(Statement::from_string(
                backend,
                format!("PRAGMA table_xinfo({table})"),
            ))
            .await
            .map_err(Error::internal)?;
        if columns.is_empty() {
            return Err(Error::custom(
                "auth_factor_schema_unavailable",
                "无法读取真实账户表字段，不能按未安装因子放行",
            ));
        }
        let names = columns
            .iter()
            .map(|row| row.try_get::<String>("", "name").map_err(Error::internal))
            .collect::<Result<Vec<_>>>()?;
        if !names.iter().any(|name| name.eq_ignore_ascii_case(column)) {
            return Ok(None);
        }
    }
    let (cast, placeholder, lock) = match backend {
        DbBackend::Postgres => ("TEXT", "$1", " FOR UPDATE"),
        DbBackend::MySql => ("CHAR", "?", " FOR UPDATE"),
        DbBackend::Sqlite => ("TEXT", "?", ""),
        _ => {
            return Err(Error::custom(
                "auth_factor_backend_unsupported",
                "因子存储后端不支持",
            ));
        }
    };
    // 列名仅来自上方固定清单。SAVEPOINT避免PG缺列错误中止外层事务。
    let probe = transaction.begin().await.map_err(Error::internal)?;
    let result = probe.query_one_raw(Statement::from_sql_and_values(
        backend,
        format!("SELECT CAST({column} AS {cast}) AS factor_value FROM {table} WHERE id={placeholder}{lock}"),
        [user_id.into()],
    )).await;
    probe.rollback().await.map_err(Error::internal)?;
    match result {
        Ok(Some(row)) => row
            .try_get("", "factor_value")
            .map(Some)
            .map_err(Error::internal),
        Ok(None) => Err(Error::Unauthorized),
        Err(error) if missing_factor_column(&error, backend) => Ok(None),
        Err(error) => Err(Error::internal(error)),
    }
}

fn missing_factor_column(error: &crate::db::sea_orm::DbErr, backend: DbBackend) -> bool {
    #[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
    {
        use crate::db::sea_orm::{DbErr, RuntimeErr};
        if let DbErr::Query(RuntimeErr::SqlxError(error))
        | DbErr::Exec(RuntimeErr::SqlxError(error)) = error
        {
            let Some(error) = error.as_database_error() else {
                return false;
            };
            return match backend {
                #[cfg(feature = "db-postgres")]
                DbBackend::Postgres => error.code().is_some_and(|code| code == "42703"),
                #[cfg(feature = "db-mysql")]
                DbBackend::MySql => error
                    .try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
                    .is_some_and(|error| error.number() == 1054),
                _ => false,
            };
        }
        false
    }
    #[cfg(not(any(feature = "db-mysql", feature = "db-postgres")))]
    {
        let _ = (error, backend);
        false
    }
}

pub(super) async fn require_unbound_fortify_account(
    db: &Db,
    transaction: &DatabaseTransaction,
    user_id: i64,
) -> Result<()> {
    require_gate_user(db, transaction, user_id).await?;
    let mut primary_columns = 0;
    let mut protected = false;
    for column in [
        "two_factor_secret",
        "two_factor_recovery_codes",
        "two_factor_confirmed_at",
    ] {
        if let Some(value) = factor_column(db, transaction, user_id, column).await? {
            primary_columns += 1;
            protected |= value.is_some();
        }
    }
    if primary_columns != 0 && primary_columns != 3 {
        return Err(Error::custom(
            "auth_factor_schema_incomplete",
            "现代因子状态字段不完整，不能从未选择证明策略的入口签发凭据",
        ));
    }
    // 只有三个现代字段真实全缺失才是未安装；非NULL（包括空密文）一律拒绝。
    if protected {
        return Err(Error::custom_with_status(
            http::StatusCode::FORBIDDEN,
            "auth_fortify_factor_proof_required",
            "此凭据入口尚不能消费当前双因素证明，请使用双因素登录流程",
        ));
    }
    Ok(())
}

async fn require_gate_user(db: &Db, transaction: &DatabaseTransaction, user_id: i64) -> Result<()> {
    use crate::db::sea_orm::TransactionTrait;
    if user_id <= 0 {
        return Err(Error::Unauthorized);
    }
    let backend = transaction.get_database_backend();
    let (marker, lock) = match backend {
        DbBackend::Postgres => ("$1", " FOR UPDATE"),
        DbBackend::MySql => ("?", " FOR UPDATE"),
        DbBackend::Sqlite => ("?", ""),
        _ => {
            return Err(Error::custom(
                "auth_factor_backend_unsupported",
                "因子存储后端不支持",
            ));
        }
    };
    // 先证明实际users行存在，不能把SQLite的“无表/无列”元数据空结果当未安装。
    // 成功释放SAVEPOINT保留同owner行锁，错误回滚SAVEPOINT保护PG外层事务。
    let table = db.quoted_table_name("users")?;
    let probe = transaction.begin().await.map_err(Error::internal)?;
    let result = probe
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            format!("SELECT id FROM {table} WHERE id={marker}{lock}"),
            [user_id.into()],
        ))
        .await;
    match result {
        Ok(Some(_)) => probe.commit().await.map_err(Error::internal),
        Ok(None) => {
            probe.rollback().await.map_err(Error::internal)?;
            Err(Error::Unauthorized)
        }
        Err(error) => {
            probe.rollback().await.map_err(Error::internal)?;
            Err(Error::internal(error))
        }
    }
}

#[cfg(all(
    test,
    any(feature = "db-sqlite", feature = "db-mysql", feature = "db-postgres")
))]
mod tests {
    use super::*;
    use crate::{
        core::BoxFuture,
        db::sea_orm_migration::{
            MigrationTrait, MigratorTrait, SchemaManager,
            prelude::{Alias, ColumnDef, Table},
        },
        testing::TestDatabase,
    };
    pub(super) struct NoMigrations;
    impl MigratorTrait for NoMigrations {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            Vec::new()
        }
    }
    pub(super) async fn owned<F>(database: TestDatabase, callback: F) -> Result<()>
    where
        F: FnOnce(Db) -> BoxFuture<'static, Result<()>>,
    {
        let db = Db::default();
        let outcome: Result<()> = async {
            db.initialize(database.connection()?)?;
            callback(db.clone()).await
        }
        .await;
        drop(db);
        let closed = database.close().await;
        outcome?;
        closed
    }
    pub(super) async fn create_user(db: &Db) -> Result<()> {
        db.connection()?
            .execute_unprepared(
                "CREATE TABLE users (id BIGINT PRIMARY KEY, password_hash VARCHAR(128) NOT NULL)",
            )
            .await
            .map_err(Error::internal)?;
        db.table("users")?
            .insert(vec![
                ("id", 42_i64.into()),
                ("password_hash", "retained".into()),
            ])
            .await?;
        Ok(())
    }
    pub(super) async fn add_column(db: &Db, column: &str) -> Result<()> {
        let connection = db.connection()?;
        SchemaManager::new(&connection)
            .alter_table(
                Table::alter()
                    .table(Alias::new(db.table_name("users")?))
                    .add_column(ColumnDef::new(Alias::new(column)).text().null())
                    .to_owned(),
            )
            .await
            .map_err(Error::internal)
    }
    pub(super) async fn assert_gate(db: &Db, expected: Option<&'static str>) -> Result<()> {
        let owner = db.clone();
        db.transaction_for_update(move |transaction| {
            Box::pin(async move {
                let result = require_unbound_fortify_account(&owner, transaction, 42).await;
                match expected {
                    None => result?,
                    Some(code) => assert!(result.err().is_some_and(|error| error.code() == code)),
                }
                // 每种探测结果后实际写入，证明PG外层事务未因缺列或拒绝而失效。
                assert_eq!(
                    owner
                        .table("users")?
                        .using_transaction(transaction)
                        .where_eq("id", 42_i64)?
                        .update(vec![("password_hash", "probe-completed".into())])
                        .await?,
                    1
                );
                Ok(())
            })
        })
        .await
    }
    pub(super) async fn schema_contract(db: Db) -> Result<()> {
        create_user(&db).await?;
        assert_gate(&db, None).await?;
        for (index, column) in [
            "two_factor_secret",
            "two_factor_recovery_codes",
            "two_factor_confirmed_at",
        ]
        .into_iter()
        .enumerate()
        {
            add_column(&db, column).await?;
            assert_gate(
                &db,
                if index == 2 {
                    None
                } else {
                    Some("auth_factor_schema_incomplete")
                },
            )
            .await?;
        }
        for column in [
            "two_factor_secret",
            "two_factor_recovery_codes",
            "two_factor_confirmed_at",
        ] {
            for value in ["", "0", "opaque-factor-state"] {
                db.table("users")?
                    .where_eq("id", 42_i64)?
                    .update(vec![(column, value.into())])
                    .await?;
                assert_gate(&db, Some("auth_fortify_factor_proof_required")).await?;
                let row = db
                    .table("users")?
                    .first()
                    .await?
                    .ok_or(Error::Unauthorized)?;
                assert_eq!(
                    row.try_get::<Option<String>>("", column)
                        .map_err(Error::internal)?
                        .as_deref(),
                    Some(value)
                );
            }
            db.table("users")?
                .where_eq("id", 42_i64)?
                .update(vec![(column, sea_orm::Value::String(None))])
                .await?;
            assert_gate(&db, None).await?;
        }
        let owner = db.clone();
        assert!(matches!(
            db.transaction_for_update(move |tx| Box::pin(async move {
                require_unbound_fortify_account(&owner, tx, 99).await
            }))
            .await,
            Err(Error::Unauthorized)
        ));
        Ok(())
    }
    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    async fn sqlite_modern_schema_presence_assets_and_parent_transaction_are_checked() -> Result<()>
    {
        owned(TestDatabase::sqlite_file::<NoMigrations>().await?, |db| {
            Box::pin(schema_contract(db))
        })
        .await
    }
    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    async fn sqlite_case_folded_and_generated_columns_cannot_hide_modern_assets() -> Result<()> {
        owned(TestDatabase::sqlite_file::<NoMigrations>().await?,|db|Box::pin(async move {
            create_user(&db).await?;
            db.connection()?.execute_unprepared("ALTER TABLE users ADD COLUMN source_secret TEXT").await.map_err(Error::internal)?;
            db.connection()?.execute_unprepared("ALTER TABLE users ADD COLUMN source_recovery TEXT").await.map_err(Error::internal)?;
            db.connection()?.execute_unprepared("ALTER TABLE users ADD COLUMN source_confirmed TEXT").await.map_err(Error::internal)?;
            for (index,(column,source)) in [("TWO_FACTOR_SECRET","source_secret"),("TwO_FaCtOr_ReCoVeRy_CoDeS","source_recovery"),("two_factor_CONFIRMED_at","source_confirmed")].into_iter().enumerate() {
                db.connection()?.execute_unprepared(&format!("ALTER TABLE users ADD COLUMN {column} TEXT GENERATED ALWAYS AS ({source}) VIRTUAL")).await.map_err(Error::internal)?;
                assert_gate(&db,if index==2 {None}else{Some("auth_factor_schema_incomplete")}).await?;
            }
            for source in ["source_secret","source_recovery","source_confirmed"] {
                db.table("users")?.where_eq("id",42_i64)?.update(vec![(source,"retained-generated".into())]).await?;
                assert_gate(&db,Some("auth_fortify_factor_proof_required")).await?;
                let row=db.table("users")?.first().await?.ok_or(Error::Unauthorized)?;
                assert_eq!(row.try_get::<String>("",source).map_err(Error::internal)?,"retained-generated");
                db.table("users")?.where_eq("id",42_i64)?.update(vec![(source,sea_orm::Value::String(None))]).await?;
            }
            assert_gate(&db,None).await
        })).await
    }
    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    async fn sqlite_plain_case_folded_and_stored_generated_modern_fields_are_protected()
    -> Result<()> {
        for stored in [false, true] {
            owned(TestDatabase::sqlite_file::<NoMigrations>().await?,move |db|Box::pin(async move {
                let definition=if stored {"id BIGINT PRIMARY KEY, password_hash TEXT NOT NULL, source TEXT, TWO_FACTOR_SECRET TEXT GENERATED ALWAYS AS (source) STORED, TwO_FaCtOr_ReCoVeRy_CoDeS TEXT GENERATED ALWAYS AS (NULL) STORED, two_factor_CONFIRMED_at TEXT GENERATED ALWAYS AS (NULL) STORED"}else{"id BIGINT PRIMARY KEY,password_hash TEXT NOT NULL,TWO_FACTOR_SECRET TEXT,TwO_FaCtOr_ReCoVeRy_CoDeS TEXT,two_factor_CONFIRMED_at TEXT"};
                db.connection()?.execute_unprepared(&format!("CREATE TABLE users ({definition})")).await.map_err(Error::internal)?;
                db.table("users")?.insert(vec![("id",42_i64.into()),("password_hash","retained".into())]).await?;
                assert_gate(&db,None).await?;
                let field=if stored {"source"}else{"TWO_FACTOR_SECRET"};
                db.table("users")?.where_eq("id",42_i64)?.update(vec![(field,"protected".into())]).await?;
                assert_gate(&db,Some("auth_fortify_factor_proof_required")).await
            })).await?;
        }
        Ok(())
    }
    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    async fn sqlite_missing_user_table_is_not_an_uninstalled_factor_schema() -> Result<()> {
        owned(TestDatabase::sqlite_file::<NoMigrations>().await?, |db| {
            Box::pin(async move {
                let owner = db.clone();
                db.transaction_for_update(move |tx| {
                    Box::pin(async move {
                        assert!(
                            require_unbound_fortify_account(&owner, tx, 42)
                                .await
                                .is_err()
                        );
                        tx.execute_unprepared("CREATE TABLE parent_still_alive(id INTEGER)")
                            .await
                            .map_err(Error::internal)?;
                        Ok(())
                    })
                })
                .await
            })
        })
        .await
    }
    #[test]
    fn message_lookalikes_never_become_missing_column_evidence() {
        for backend in [DbBackend::MySql, DbBackend::Postgres, DbBackend::Sqlite] {
            for message in [
                "Unknown column; 1054; 42703",
                "permission denied; deadlock; table not found",
            ] {
                assert!(!missing_factor_column(
                    &crate::db::sea_orm::DbErr::Custom(message.into()),
                    backend
                ));
            }
        }
    }
}
