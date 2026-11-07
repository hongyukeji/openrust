//! 发布矩阵源码：双query、成熟物理lease、读角色、第二步错误及CTX来源。
use super::{Db, Error, Result};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
type Log = Arc<Mutex<Vec<(&'static str, String, bool)>>>;
fn log_values(log: &Log) -> Vec<(&'static str, String, bool)> {
    log.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}
fn clear(log: &Log) {
    log.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}
async fn fixture(
    label: &'static str,
    log: &Log,
) -> Result<(sqlx::SqlitePool, DatabaseConnection, Arc<AtomicUsize>)> {
    let acquisitions = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&acquisitions);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .before_acquire(move |_, _| {
            let counter = Arc::clone(&counter);
            Box::pin(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(true)
            })
        })
        .connect("sqlite::memory:")
        .await
        .map_err(Error::internal)?;
    let mut connection: DatabaseConnection = pool.clone().into();
    let observed = Arc::clone(log);
    connection.set_metric_callback(move |info| {
        observed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((label, info.statement.sql.clone(), info.failed));
    });
    Ok((pool, connection, acquisitions))
}
#[tokio::test]
#[ignore = "发布前SQLite专跑：物理lease/temp/ATTACH/副本/NUL矩阵"]
async fn writer_pin_retains_temp_generated_names_empty_and_view_second_query() -> Result<()> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (pool, connection, acquisitions) = fixture("p", &log).await?;
    connection.execute_unprepared("CREATE TEMP TABLE probe (z INTEGER,a TEXT,b TEXT GENERATED ALWAYS AS (a||a) VIRTUAL); CREATE VIEW vprobe AS SELECT 1 AS z, 2 AS a").await.map_err(Error::internal)?;
    let db = Db::default();
    db.initialize(connection)?;
    acquisitions.store(0, Ordering::SeqCst);
    assert_eq!(
        db.schema().get_column_listing("temp.probe").await?,
        ["z", "a", "b"]
    );
    assert_eq!(acquisitions.load(Ordering::SeqCst), 1);
    let calls = log_values(&log);
    assert_eq!(calls.len(), 2);
    assert!(calls[0].1.contains("pragma_table_xinfo"));
    assert!(calls[1].1.contains("\"temp\".sqlite_master"));
    assert!(!calls.iter().any(|entry| entry.2));
    clear(&log);
    assert_eq!(
        db.schema().get_column_listing("missing").await?,
        [] as [String; 0]
    );
    assert_eq!(log_values(&log).len(), 2);
    clear(&log);
    assert_eq!(db.schema().get_column_listing("vprobe").await?, ["z", "a"]);
    assert_eq!(log_values(&log).len(), 2);
    drop(db);
    pool.close().await;
    Ok(())
}
#[tokio::test]
#[ignore = "发布前SQLite专跑：物理lease/temp/ATTACH/副本/NUL矩阵"]
async fn replica_failure_is_not_primary_fallback_and_failed_first_does_not_route() -> Result<()> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (writer, primary, _) = fixture("p", &log).await?;
    let (reader, replica, _) = fixture("r", &log).await?;
    primary
        .execute_unprepared(
            "ATTACH ':memory:' AS attached; CREATE TABLE attached.probe(z INTEGER,a TEXT)",
        )
        .await
        .map_err(Error::internal)?;
    let db = Db::default();
    db.initialize_with_replicas(primary, vec![replica])?;
    assert!(
        db.schema()
            .get_column_listing("unknown.probe")
            .await
            .is_err()
    );
    assert_eq!(log_values(&log).len(), 1);
    assert_eq!(db.next_replica.load(Ordering::SeqCst), 0);
    assert!(log_values(&log)[0].2);
    clear(&log);
    assert!(
        db.schema()
            .get_column_listing("attached.probe")
            .await
            .is_err()
    );
    let calls = log_values(&log);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "p");
    assert_eq!(calls[1].0, "r");
    assert!(calls[1].2);
    clear(&log);
    let columns = db
        .with_primary(db.schema().get_column_listing("attached.probe"))
        .await??;
    assert_eq!(columns, ["z", "a"]);
    assert!(log_values(&log).iter().all(|entry| entry.0 == "p"));
    clear(&log);
    let selected = db.next_replica.load(Ordering::SeqCst);
    writer.close().await;
    assert!(
        db.schema()
            .get_column_listing("attached.probe")
            .await
            .is_err()
    );
    assert_eq!(log_values(&log), [] as [(&str, String, bool); 0]); // acquire失败，不记fetch metric也不选第二读路由。
    assert_eq!(db.next_replica.load(Ordering::SeqCst), selected);
    drop(db);
    writer.close().await;
    reader.close().await;
    Ok(())
}
#[tokio::test]
#[ignore = "发布前SQLite专跑：物理lease/temp/ATTACH/副本/NUL矩阵"]
async fn replica_role_sharing_one_pool_releases_writer_before_acquire() -> Result<()> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (pool, connection, acquisitions) = fixture("same", &log).await?;
    connection
        .execute_unprepared("CREATE TABLE probe(z INTEGER,a TEXT)")
        .await
        .map_err(Error::internal)?;
    let db = Db::default();
    db.initialize_with_replicas(connection.clone(), vec![connection])?;
    acquisitions.store(0, Ordering::SeqCst);
    let columns = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        db.schema().get_column_listing("probe"),
    )
    .await
    .map_err(Error::internal)??;
    assert_eq!(columns, ["z", "a"]);
    assert_eq!(acquisitions.load(Ordering::SeqCst), 2);
    assert_eq!(log_values(&log).len(), 2);
    drop(db);
    pool.close().await;
    Ok(())
}
#[tokio::test]
#[ignore = "发布前SQLite专跑：物理lease/temp/ATTACH/副本/NUL矩阵"]
async fn context_reads_uncommitted_ddl_and_rejects_another_db_same_pool() -> Result<()> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (pool, connection, _) = fixture("tx", &log).await?;
    let db = Db::default();
    db.initialize(connection.clone())?;
    let other = Db::default();
    other.initialize(connection)?;
    let owner = db.clone();
    let observed = Arc::clone(&log);
    let columns = db
        .transaction_for_update_checked_context(
            move |context| {
                Box::pin(async move {
                    let before = log_values(&observed).len();
                    let error = other
                        .schema()
                        .using_transaction(&context)
                        .err()
                        .ok_or(Error::Unauthorized)?;
                    assert_eq!(error.code(), "schema_transaction_source_mismatch");
                    assert_eq!(error.status().as_u16(), 500);
                    assert_eq!(log_values(&observed).len(), before);
                    context
                        .transaction()
                        .execute_unprepared("CREATE TABLE txprobe(b INTEGER,a TEXT)")
                        .await
                        .map_err(Error::internal)?;
                    owner
                        .schema()
                        .using_transaction(&context)?
                        .get_column_listing("txprobe")
                        .await
                })
            },
            |_| Box::pin(async { Ok(()) }),
        )
        .await?;
    assert_eq!(columns, ["b", "a"]);
    drop(db);
    pool.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "发布前SQLite专跑：物理lease/temp/ATTACH/副本/NUL矩阵"]
async fn nul_bound_identifier_counterproof_and_schema_rejection_before_query() -> Result<()> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (pool, connection, _) = fixture("nul", &log).await?;
    connection
        .execute_unprepared("CREATE TEMP TABLE probe(z INTEGER,a TEXT)")
        .await
        .map_err(Error::internal)?;
    let db = Db::default();
    db.initialize(connection.clone())?;
    for (table, schema, reference) in [
        ("probe\0suffix", "temp", "temp.probe\0suffix"),
        ("probe", "temp\0suffix", "temp\0suffix.probe"),
    ] {
        // 固定本地SQLite pragmaVtabFilter用%s复制绑定参数；这是尚未执行的原生反例。
        let rows = connection
            .query_all_raw(sea_orm::Statement::from_sql_and_values(
                sea_orm::DbBackend::Sqlite,
                "select name from pragma_table_xinfo(?, ?) order by cid asc",
                [table.into(), schema.into()],
            ))
            .await
            .map_err(Error::internal)?;
        let names: Vec<String> = rows
            .into_iter()
            .map(|row| row.try_get("", "name").map_err(Error::internal))
            .collect::<Result<_>>()?;
        assert_eq!(names, ["z", "a"]);
        clear(&log);
        assert_eq!(
            db.schema()
                .get_column_listing(reference)
                .await
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("schema_identifier_invalid")
        );
        assert_eq!(log_values(&log), [] as [(&str, String, bool); 0]);
    }
    drop(db);
    drop(connection);
    pool.close().await;
    Ok(())
}
