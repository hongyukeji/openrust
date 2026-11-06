//! 验证生成应用通过公开 Query Builder 获取本次插入 ID，并保持事务与回调语义。

use openrust::{
    Error, Result,
    database::{
        Db,
        sea_orm::{ConnectionTrait, DbBackend, TransactionTrait, Value},
        sea_orm_migration::{MigrationTrait, MigratorTrait},
    },
    testing::TestDatabase,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct EmptyMigrator;
impl MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}

fn check(condition: bool) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::custom(
            "generated_insert_id_contract",
            "插入 ID 公开契约不符",
        ))
    }
}

#[tokio::test]
async fn public_insert_ids_preserve_execution_and_backend_contracts()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping public insert ID test: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = TestDatabase::create::<EmptyMigrator>(&url).await?;
    let outcome: Result<()> = async {
        let db = Db::default();
        db.initialize(database.connection()?)?;
        let connection = db.connection()?;
        let backend = db.backend()?;
        let ddl = match backend {
            DbBackend::Sqlite => "CREATE TABLE insert_items (id INTEGER PRIMARY KEY, label TEXT DEFAULT 'default')",
            DbBackend::MySql => "CREATE TABLE insert_items (id BIGINT AUTO_INCREMENT PRIMARY KEY, label VARCHAR(255) DEFAULT 'default')",
            DbBackend::Postgres => "CREATE TABLE insert_items (id BIGSERIAL PRIMARY KEY, label TEXT DEFAULT 'default')",
            _ => return Err(Error::custom("generated_insert_id_backend", "测试需要受支持的数据库")),
        };
        connection.execute_unprepared(ddl).await.map_err(Error::internal)?;
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let query = db.table("insert_items")?.before_query(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        check(query.insert_get_id(Vec::new(), None).await? == Value::BigInt(Some(1)))?;
        let label = "bound ' value ? $1";
        check(query.insert_get_id(vec![("label", label.into())], Some("")).await? == Value::BigInt(Some(2)))?;
        check(calls.load(Ordering::SeqCst) == 1)?;
        check(query.count().await? == 2)?;
        check(db.table("insert_items")?.where_eq("id", 2_i64)?.value::<String>("label").await?.as_deref() == Some(label))?;
        check(db.table("insert_items")?.where_eq("id", 1_i64)?.value::<String>("label").await?.as_deref() == Some("default"))?;
        check(query.insert_get_id(Vec::new(), Some("0")).await? == Value::BigInt(Some(3)))?;
        check(calls.load(Ordering::SeqCst) == 1)?;
        rollback_insert(&db).await?;
        check(query.count().await? == 3)?;
        backend_ids(&db).await
    }.await;
    database.close().await?;
    outcome?;
    Ok(())
}

async fn rollback_insert(db: &Db) -> Result<()> {
    let transaction = db.connection()?.begin().await.map_err(Error::internal)?;
    let outcome: Result<()> = async {
        let query = db.table("insert_items")?.using_transaction(&transaction);
        check(
            query
                .insert_get_id(vec![("label", "rolled-back".into())], None)
                .await?
                == Value::BigInt(Some(4)),
        )?;
        check(query.count().await? == 4)
    }
    .await;
    transaction.rollback().await.map_err(Error::internal)?;
    outcome
}

async fn backend_ids(db: &Db) -> Result<()> {
    let connection = db.connection()?;
    match db.backend()? {
        DbBackend::Sqlite => {
            connection
                .execute_unprepared("CREATE TABLE signed_insert_items (id INTEGER PRIMARY KEY)")
                .await
                .map_err(Error::internal)?;
            let query = db.table("signed_insert_items")?;
            check(
                query
                    .insert_get_id(vec![("id", (-10_i64).into())], None)
                    .await?
                    == Value::BigInt(Some(-10)),
            )?;
            check(
                query
                    .insert_get_id(Vec::new(), Some("ignored_sequence"))
                    .await?
                    == Value::BigInt(Some(-9)),
            )
        }
        DbBackend::Postgres => {
            connection
                .execute_unprepared("CREATE TABLE text_insert_items (custom_key TEXT PRIMARY KEY)")
                .await
                .map_err(Error::internal)?;
            let query = db.table("text_insert_items")?;
            check(
                query
                    .insert_get_id(vec![("custom_key", "000123".into())], Some("custom_key"))
                    .await?
                    == Value::BigInt(Some(123)),
            )?;
            check(
                query
                    .insert_get_id(vec![("custom_key", "key:abc".into())], Some("custom_key"))
                    .await?
                    == Value::from("key:abc"),
            )
        }
        DbBackend::MySql => check(
            db.table("insert_items")?
                .insert_get_id(Vec::new(), Some("ignored_sequence"))
                .await?
                == Value::BigInt(Some(5)),
        ),
        _ => Err(Error::custom(
            "generated_insert_id_backend",
            "测试需要受支持的数据库",
        )),
    }
}
