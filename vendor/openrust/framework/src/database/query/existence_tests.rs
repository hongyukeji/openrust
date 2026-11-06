//! 存在性查询保留原查询形状、读连接与显式事务的回归。

use super::*;
use crate::database::{Db, SortDirection};
use sea_orm::{Database, DatabaseConnection, TransactionTrait};

async fn connection() -> Result<DatabaseConnection> {
    let connection = Database::connect("sqlite::memory:")
        .await
        .map_err(Error::internal)?;
    connection
        .execute_unprepared("CREATE TABLE items (id INTEGER, label TEXT NULL)")
        .await
        .map_err(Error::internal)?;
    Ok(connection)
}

async fn database() -> Result<Db> {
    let db = Db::default();
    db.initialize(connection().await?)?;
    Ok(db)
}

#[tokio::test]
async fn empty_filtered_null_and_existing_queries_return_boolean() -> Result<()> {
    let db = database().await?;
    assert!(!db.table("items")?.exists().await?);
    assert!(db.table("items")?.doesnt_exist().await?);
    db.table("items")?
        .insert(vec![
            ("id", 1.into()),
            ("label", Option::<String>::None.into()),
        ])
        .await?;
    let reusable = db.table("items")?.where_eq("id", 1)?;
    assert!(reusable.exists().await?);
    assert!(!reusable.doesnt_exist().await?);
    assert_eq!(reusable.count().await?, 1);
    assert!(reusable.exists().await?);
    assert!(db.table("items")?.exists().await?);
    assert!(!db.table("items")?.doesnt_exist().await?);
    assert!(
        db.table("items")?
            .select(&["label"])?
            .where_null("label")?
            .exists()
            .await?
    );
    assert!(!db.table("items")?.where_eq("id", 2)?.exists().await?);
    assert!(db.table("items")?.where_eq("id", 2)?.doesnt_exist().await?);
    assert!(!db.table("items")?.where_not_null("label")?.exists().await?);
    Ok(())
}

#[tokio::test]
async fn limits_offsets_and_unions_are_not_replaced_by_count() -> Result<()> {
    let db = database().await?;
    db.table("items")?.insert(vec![("id", 1.into())]).await?;
    assert!(!db.table("items")?.limit(0).exists().await?);
    assert!(db.table("items")?.limit(1).exists().await?);
    assert!(!db.table("items")?.limit(1).offset(1).exists().await?);
    assert!(db.table("items")?.limit(0).doesnt_exist().await?);
    let matching = db.table("items")?.where_eq("id", 1)?;
    let absent = db.table("items")?.where_eq("id", 2)?;
    assert!(absent.clone().union(&matching, true)?.exists().await?);
    assert!(!absent.clone().union(&absent, false)?.exists().await?);
    assert!(!absent.union(&matching, false)?.doesnt_exist().await?);
    Ok(())
}

#[tokio::test]
async fn read_replica_and_primary_scope_follow_existing_db_routing() -> Result<()> {
    let primary = connection().await?;
    primary
        .execute_unprepared("INSERT INTO items (id) VALUES (1)")
        .await
        .map_err(Error::internal)?;
    let replica = connection().await?;
    let db = Db::default();
    db.initialize_with_replicas(primary, vec![replica])?;
    assert!(!db.table("items")?.exists().await?);
    assert!(db.table("items")?.doesnt_exist().await?);
    assert!(db.with_primary(db.table("items")?.exists()).await??);
    assert!(!db.with_primary(db.table("items")?.doesnt_exist()).await??);
    assert!(!db.table("items")?.exists().await?);
    Ok(())
}

async fn file_database() -> Result<(tempfile::TempDir, Db, Db)> {
    let directory = tempfile::tempdir().map_err(Error::internal)?;
    let url = format!(
        "sqlite://{}?mode=rwc",
        directory.path().join("existence.sqlite").display()
    );
    let writer = Database::connect(&url).await.map_err(Error::internal)?;
    writer
        .execute_unprepared("CREATE TABLE items (id INTEGER, label TEXT NULL)")
        .await
        .map_err(Error::internal)?;
    let reader = Database::connect(&url).await.map_err(Error::internal)?;
    let db = Db::default();
    db.initialize(writer)?;
    let observer = Db::default();
    observer.initialize(reader)?;
    Ok((directory, db, observer))
}

#[tokio::test]
async fn explicit_transaction_observes_uncommitted_rows() -> Result<()> {
    let (_directory, db, observer) = file_database().await?;
    let transaction = db
        .primary_connection()?
        .begin()
        .await
        .map_err(Error::internal)?;
    db.table("items")?
        .using_transaction(&transaction)
        .insert(vec![("id", 1.into())])
        .await?;
    assert!(
        db.table("items")?
            .using_transaction(&transaction)
            .exists()
            .await?
    );
    assert!(
        !db.table("items")?
            .using_transaction(&transaction)
            .doesnt_exist()
            .await?
    );
    assert!(!observer.table("items")?.exists().await?);
    transaction.commit().await.map_err(Error::internal)?;
    assert!(observer.table("items")?.exists().await?);
    Ok(())
}

#[tokio::test]
async fn invalid_scalar_rebinding_and_driver_errors_are_not_false_results() -> Result<()> {
    let (_left_directory, left, _) = file_database().await?;
    let (_right_directory, right, _) = file_database().await?;
    left.table("items")?.insert(vec![("id", 1.into())]).await?;
    let scalar = left.table("items")?.select(&["id"])?;
    let attached = left.table("items")?.where_between_columns(
        &scalar,
        [
            super::super::RawSql::new("0"),
            super::super::RawSql::new("2"),
        ],
    )?;
    assert!(attached.exists().await?);
    let transaction = right
        .primary_connection()?
        .begin()
        .await
        .map_err(Error::internal)?;
    let rebound = attached.using_transaction(&transaction);
    assert!(
        matches!(rebound.exists().await, Err(error) if error.code() == "query_subquery_context_mismatch")
    );
    assert!(
        matches!(rebound.doesnt_exist().await, Err(error) if error.code() == "query_subquery_context_mismatch")
    );
    drop(rebound);
    transaction.rollback().await.map_err(Error::internal)?;
    assert!(left.table("missing_table")?.exists().await.is_err());
    assert!(left.table("missing_table")?.doesnt_exist().await.is_err());
    Ok(())
}

#[tokio::test]
async fn all_dialects_preserve_the_original_query_sql_and_bindings() -> Result<()> {
    let db = database().await?;
    let query = db
        .table("items")?
        .where_eq("label", "bound value")?
        .order_by("id", SortDirection::Desc)?
        .limit(2)
        .offset(1);
    for backend in [DbBackend::Sqlite, DbBackend::MySql, DbBackend::Postgres] {
        let build = |select: &SelectStatement| match backend {
            DbBackend::Sqlite => select.build(super::super::SqliteQueryBuilder),
            DbBackend::MySql => select.build(super::super::MysqlQueryBuilder),
            _ => select.build(super::super::PostgresQueryBuilder),
        };
        let (inner_sql, inner_values) = build(&query.scoped_select()?);
        let (sql, values) = build(&query.existence_select()?);
        let quote = if backend == DbBackend::MySql {
            '`'
        } else {
            '"'
        };
        assert_eq!(
            sql,
            format!("SELECT EXISTS({inner_sql}) AS {quote}exists{quote}")
        );
        assert_eq!(values.0, inner_values.0);
        assert_eq!(values.0.first(), Some(&"bound value".into()));
    }
    Ok(())
}
