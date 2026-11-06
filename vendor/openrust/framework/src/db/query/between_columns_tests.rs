//! SQLite 真实行过滤、绑定顺序及 SQL 布尔优先级回归。

use super::*;
use crate::{Error, db::Db};
use sea_orm::{
    ConnectionTrait, Database, DbBackend, Statement, TransactionTrait,
    sea_query::{MysqlQueryBuilder, PostgresQueryBuilder, SqliteQueryBuilder, Values},
};

async fn database() -> Result<Db> {
    let connection = Database::connect("sqlite::memory:")
        .await
        .map_err(Error::internal)?;
    connection.execute_unprepared(
        "CREATE TABLE ranges (id INTEGER, value INTEGER, lower_bound INTEGER, upper_bound INTEGER, flag INTEGER);\
         INSERT INTO ranges VALUES (1,10,10,20,0),(2,20,10,20,1),(3,9,10,20,0),(4,21,10,20,1),\
         (5,NULL,10,20,0),(6,15,NULL,20,0),(7,15,10,NULL,0),(8,15,20,10,0)"
    ).await.map_err(Error::internal)?;
    let db = Db::default();
    db.initialize(connection)?;
    Ok(db)
}

async fn ids(query: QueryBuilder<'_, '_>) -> Result<Vec<i64>> {
    query
        .order_by("id", crate::db::query::SortDirection::Asc)?
        .get()
        .await?
        .into_iter()
        .map(|row| row.try_get("", "id").map_err(Error::internal))
        .collect()
}

#[tokio::test]
async fn inclusive_not_null_and_reversed_bounds() -> Result<()> {
    let db = database().await?;
    assert_eq!(
        ids(db.table("ranges")?.where_between_columns(
            "main.ranges.value",
            ["main.ranges.lower_bound", "main.ranges.upper_bound"]
        )?)
        .await?,
        [1, 2]
    );
    assert_eq!(
        ids(db
            .table("ranges")?
            .where_between_columns("value", ["lower_bound", "upper_bound"])?)
        .await?,
        [1, 2]
    );
    assert_eq!(
        ids(db
            .table("ranges")?
            .where_not_between_columns("value", ["lower_bound", "upper_bound"])?)
        .await?,
        [3, 4, 8]
    );
    assert_eq!(
        ids(db
            .table("ranges")?
            .where_between_columns("ranges.value", ["ranges.lower_bound", "ranges.upper_bound"])?)
        .await?,
        [1, 2]
    );
    Ok(())
}

#[tokio::test]
async fn or_respects_and_precedence_and_explicit_groups() -> Result<()> {
    let db = database().await?;
    assert_eq!(
        ids(db
            .table("ranges")?
            .where_eq("id", 3)?
            .or_where_between_columns("value", ["lower_bound", "upper_bound"])?
            .where_eq("flag", 1)?)
        .await?,
        [2, 3]
    );
    assert_eq!(
        ids(db
            .table("ranges")?
            .where_eq("id", 1)?
            .or_where_not_between_columns("value", ["lower_bound", "upper_bound"])?
            .where_eq("flag", 1)?)
        .await?,
        [1, 4]
    );
    assert_eq!(
        ids(db
            .table("ranges")?
            .or_where_between_columns("value", ["lower_bound", "upper_bound"])?)
        .await?,
        [1, 2]
    );
    assert_eq!(
        ids(db
            .table("ranges")?
            .or_where_not_between_columns("value", ["lower_bound", "upper_bound"])?)
        .await?,
        [3, 4, 8]
    );
    assert_eq!(
        ids(db
            .table("ranges")?
            .where_nested(|group| group
                .where_eq("id", 3)?
                .or_where_between_columns("value", ["lower_bound", "upper_bound"]))?
            .where_eq("flag", 1)?)
        .await?,
        [2]
    );
    assert_eq!(
        ids(db.table("ranges")?.where_nested(|group| group
            .where_not_between_columns("value", ["lower_bound", "upper_bound"])?
            .or_where_not_between_columns("id", [RawSql::new("1"), RawSql::new("7")]))?)
        .await?,
        [3, 4, 8]
    );
    assert_eq!(
        ids(db.table("ranges")?.where_nested(
            |group| group.where_between_columns("value", ["lower_bound", "upper_bound"])
        )?)
        .await?,
        [1, 2]
    );
    Ok(())
}

#[tokio::test]
async fn expressions_and_subqueries_preserve_binding_order() -> Result<()> {
    let db = database().await?;
    let query = db
        .table("ranges")?
        .where_eq("flag", 1)?
        .where_between_columns("value", ["lower_bound", "upper_bound"])?;
    let statement = query.statement(&query.scoped_select()?)?;
    assert_eq!(statement.values, Some(Values(vec![1.into()])));
    let scalar = db
        .table("ranges")?
        .select(&["value"])?
        .where_in("id", [1, 3])?
        .order_by("value", crate::db::query::SortDirection::Desc)?
        .limit(1);
    let query = db
        .table("ranges")?
        .where_eq("id", 2)?
        .where_between_columns(
            &scalar,
            [
                ColumnOperand::from(RawSql::new("10")),
                ColumnOperand::from(RawSql::new("20")),
            ],
        )?;
    let mut expected = vec![2.into()];
    expected.extend(
        scalar
            .statement(&scalar.scoped_select()?)?
            .values
            .ok_or_else(|| Error::custom("test_bindings_missing", "subquery bindings missing"))?
            .0,
    );
    assert_eq!(
        query.statement(&query.scoped_select()?)?.values,
        Some(Values(expected))
    );
    assert_eq!(ids(query).await?, [2]);
    assert_eq!(
        ids(db
            .table("ranges")?
            .where_between_columns(RawSql::new("10"), [RawSql::new("10"), RawSql::new("20")])?)
        .await?,
        [1, 2, 3, 4, 5, 6, 7, 8]
    );
    // 各方言都必须引用列名，不能将边界变成占位符或字符串值。
    for backend in [DbBackend::Sqlite, DbBackend::MySql, DbBackend::Postgres] {
        let query = db
            .table("ranges")?
            .where_between_columns("value", ["ranges.lower_bound", "ranges.upper_bound"])?;
        let select = query.scoped_select()?;
        let (sql, values) = match backend {
            DbBackend::Sqlite => select.build(SqliteQueryBuilder),
            DbBackend::MySql => select.build(MysqlQueryBuilder),
            _ => select.build(PostgresQueryBuilder),
        };
        assert_eq!(values.0, [] as [sea_orm::Value; 0]);
        assert!(sql.to_lowercase().contains("between"));
        let quote = if backend == DbBackend::MySql {
            '`'
        } else {
            '"'
        };
        assert!(sql.contains(&format!("{quote}ranges{quote}.{quote}lower_bound{quote}")));
        let outer = db
            .table("ranges")?
            .where_eq("id", 2)?
            .where_between_columns(&scalar, [RawSql::new("10"), RawSql::new("20")])?;
        let build = |query: &QueryBuilder<'_, '_>| -> Result<(String, Values)> {
            Ok(match backend {
                DbBackend::Sqlite => query.scoped_select()?.build(SqliteQueryBuilder),
                DbBackend::MySql => query.scoped_select()?.build(MysqlQueryBuilder),
                _ => query.scoped_select()?.build(PostgresQueryBuilder),
            })
        };
        let (sql, values) = build(&outer)?;
        let mut expected = vec![2.into()];
        expected.extend(build(&scalar)?.1.0);
        assert_eq!(values.0, expected);
        assert!(sql.to_lowercase().contains("limit"));
        assert!(sql.to_lowercase().contains("desc"));
    }
    Ok(())
}

#[tokio::test]
async fn malformed_identifiers_fail_before_execution() -> Result<()> {
    let db = database().await?;
    for column in [
        "",
        ".value",
        "ranges..value",
        "value; DROP TABLE ranges",
        "a.b.c.d",
        "value --",
    ] {
        let result = db
            .table("ranges")?
            .where_between_columns(column, ["lower_bound", "upper_bound"]);
        assert!(matches!(result, Err(error) if error.code() == "query_identifier_invalid"));
        let result = db
            .table("ranges")?
            .where_not_between_columns("value", ["lower_bound", column]);
        assert!(matches!(result, Err(error) if error.code() == "query_identifier_invalid"));
    }
    assert_eq!(
        db.primary_connection()?
            .query_all_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT * FROM ranges"
            ))
            .await
            .map_err(Error::internal)?
            .len(),
        8
    );
    Ok(())
}

#[tokio::test]
async fn mutations_use_the_same_or_precedence() -> Result<()> {
    let db = database().await?;
    let query = db
        .table("ranges")?
        .where_eq("id", 3)?
        .or_where_between_columns("value", ["lower_bound", "upper_bound"])?
        .where_eq("flag", 1)?;
    assert_eq!(query.update(vec![("flag", 7.into())]).await?, 2);
    assert_eq!(ids(db.table("ranges")?.where_eq("flag", 7)?).await?, [2, 3]);
    assert_eq!(
        db.table("ranges")?
            .where_eq("id", 1)?
            .or_where_eq("id", 4)?
            .where_eq("flag", 1)?
            .delete()
            .await?,
        2
    );
    assert_eq!(ids(db.table("ranges")?).await?, [2, 3, 5, 6, 7, 8]);
    Ok(())
}

#[tokio::test]
async fn empty_chain_and_empty_groups_do_not_filter_crud() -> Result<()> {
    let db = database().await?;
    assert_eq!(db.table("ranges")?.count().await?, 8);
    assert_eq!(db.table("ranges")?.where_nested(Ok)?.count().await?, 8);
    assert_eq!(
        db.table("ranges")?
            .where_eq("id", 1)?
            .where_nested(Ok)?
            .count()
            .await?,
        1
    );
    assert_eq!(
        db.table("ranges")?
            .where_nested(Ok)?
            .or_where_eq("id", 1)?
            .count()
            .await?,
        1
    );
    assert_eq!(
        db.table("ranges")?
            .where_nested(Ok)?
            .update(vec![("flag", 5.into())])
            .await?,
        8
    );
    assert_eq!(db.table("ranges")?.where_eq("flag", 5)?.count().await?, 8);
    assert_eq!(db.table("ranges")?.where_nested(Ok)?.delete().await?, 8);
    db.table("ranges")?.insert(vec![("id", 9.into())]).await?;
    assert_eq!(
        db.table("ranges")?.update(vec![("flag", 6.into())]).await?,
        1
    );
    assert_eq!(db.table("ranges")?.delete().await?, 1);
    Ok(())
}

async fn file_database(value: i64) -> Result<(tempfile::TempDir, Db)> {
    let directory = tempfile::tempdir().map_err(Error::internal)?;
    let path = directory.path().join("query.sqlite");
    let connection = Database::connect(format!("sqlite://{}?mode=rwc", path.display()))
        .await
        .map_err(Error::internal)?;
    connection.execute_unprepared("CREATE TABLE ranges (id INTEGER, value INTEGER, lower_bound INTEGER, upper_bound INTEGER)").await.map_err(Error::internal)?;
    let db = Db::default();
    db.initialize(connection)?;
    db.table("ranges")?
        .insert(vec![
            ("id", 1.into()),
            ("value", value.into()),
            ("lower_bound", 10.into()),
            ("upper_bound", 20.into()),
        ])
        .await?;
    Ok((directory, db))
}

#[tokio::test]
async fn subquery_database_and_transaction_provenance_is_checked() -> Result<()> {
    let (_left_directory, left) = file_database(15).await?;
    let (_right_directory, right) = file_database(99).await?;
    let foreign = right.table("ranges")?.select(&["value"])?;
    let result = left
        .table("ranges")?
        .where_between_columns(&foreign, ["lower_bound", "upper_bound"]);
    assert!(matches!(&result, Err(error) if error.code() == "query_subquery_context_mismatch"));
    drop(result);
    let result = left.table("ranges")?.where_nested(|group| {
        group.where_between_columns(&foreign, ["lower_bound", "upper_bound"])
    });
    assert!(matches!(&result, Err(error) if error.code() == "query_subquery_context_mismatch"));
    drop(result);
    let same_handle = left.clone();
    let local = same_handle.table("ranges")?.select(&["value"])?;
    assert_eq!(
        ids(left
            .table("ranges")?
            .where_between_columns(&local, ["lower_bound", "upper_bound"])?)
        .await?,
        [1]
    );
    let transaction = left
        .primary_connection()?
        .begin()
        .await
        .map_err(Error::internal)?;
    let other_transaction = right
        .primary_connection()?
        .begin()
        .await
        .map_err(Error::internal)?;
    let transactional = left
        .table("ranges")?
        .using_transaction(&transaction)
        .select(&["value"])?;
    let result = left
        .table("ranges")?
        .where_between_columns(&transactional, ["lower_bound", "upper_bound"]);
    assert!(matches!(&result, Err(error) if error.code() == "query_subquery_context_mismatch"));
    drop(result);
    let result = left
        .table("ranges")?
        .using_transaction(&transaction)
        .where_between_columns(&local, ["lower_bound", "upper_bound"]);
    assert!(matches!(&result, Err(error) if error.code() == "query_subquery_context_mismatch"));
    drop(result);
    let other_context = left
        .table("ranges")?
        .using_transaction(&other_transaction)
        .select(&["value"])?;
    let result = left
        .table("ranges")?
        .using_transaction(&transaction)
        .where_between_columns(&other_context, ["lower_bound", "upper_bound"]);
    assert!(matches!(&result, Err(error) if error.code() == "query_subquery_context_mismatch"));
    drop(result);
    assert_eq!(
        ids(left
            .table("ranges")?
            .using_transaction(&transaction)
            .where_nested(
                |group| group.where_between_columns(&transactional, ["lower_bound", "upper_bound"])
            )?)
        .await?,
        [1]
    );
    drop(transactional);
    drop(other_context);
    transaction.rollback().await.map_err(Error::internal)?;
    other_transaction
        .rollback()
        .await
        .map_err(Error::internal)?;
    Ok(())
}

#[tokio::test]
async fn logical_qualifiers_reuse_prefix_and_mapping() -> Result<()> {
    use crate::db::DatabaseTableNames;
    use std::collections::BTreeMap;
    for (names, physical) in [
        (
            DatabaseTableNames::new("prefix_", BTreeMap::new())?,
            "prefix_ranges",
        ),
        (
            DatabaseTableNames::new(
                "prefix_",
                BTreeMap::from([("ranges".to_owned(), "mapped_ranges".to_owned())]),
            )?,
            "mapped_ranges",
        ),
    ] {
        let connection = Database::connect("sqlite::memory:")
            .await
            .map_err(Error::internal)?;
        connection.execute_unprepared(&format!("CREATE TABLE {physical} (id INTEGER, value INTEGER, lower_bound INTEGER, upper_bound INTEGER); INSERT INTO {physical} VALUES (1,15,10,20),(2,25,10,20)")).await.map_err(Error::internal)?;
        let db = Db::with_table_names(names);
        db.initialize(connection)?;
        assert_eq!(
            ids(db.table("ranges")?.where_between_columns(
                "ranges.value",
                ["ranges.lower_bound", "ranges.upper_bound"]
            )?)
            .await?,
            [1]
        );
        assert_eq!(
            ids(db
                .table("ranges")?
                .where_nested(|group| group.where_not_between_columns(
                    "ranges.value",
                    ["ranges.lower_bound", "ranges.upper_bound"]
                ))?)
            .await?,
            [2]
        );
        let schema_query = db.table("ranges")?.where_between_columns(
            "main.ranges.value",
            ["main.ranges.lower_bound", "main.ranges.upper_bound"],
        )?;
        let (sql, _) = schema_query.scoped_select()?.build(SqliteQueryBuilder);
        assert!(sql.contains("\"prefix_main\".\"ranges\".\"value\""));
        assert_eq!(
            ids(db.table("ranges")?.where_between_columns(
                format!("{physical}.value"),
                [
                    format!("{physical}.lower_bound"),
                    format!("{physical}.upper_bound")
                ]
            )?)
            .await?,
            [1]
        );
    }
    Ok(())
}

async fn assert_rebound_terminals_rejected(rebound: QueryBuilder<'_, '_>) {
    assert!(
        matches!(rebound.get().await, Err(error) if error.code() == "query_subquery_context_mismatch")
    );
    assert!(
        matches!(rebound.first().await, Err(error) if error.code() == "query_subquery_context_mismatch")
    );
    assert!(
        matches!(rebound.count().await, Err(error) if error.code() == "query_subquery_context_mismatch")
    );
    assert!(
        matches!(rebound.update(vec![("value", 0.into())]).await, Err(error) if error.code() == "query_subquery_context_mismatch")
    );
    assert!(
        matches!(rebound.delete().await, Err(error) if error.code() == "query_subquery_context_mismatch")
    );
}

async fn assert_transaction_values_unchanged(
    left: &Db,
    right: &Db,
    left_tx: &sea_orm::DatabaseTransaction,
    right_tx: &sea_orm::DatabaseTransaction,
) -> Result<()> {
    assert_eq!(
        left.table("ranges")?
            .using_transaction(left_tx)
            .value::<i64>("value")
            .await?,
        Some(15)
    );
    assert_eq!(
        right
            .table("ranges")?
            .using_transaction(right_tx)
            .value::<i64>("value")
            .await?,
        Some(99)
    );
    Ok(())
}

async fn assert_valid_file_range_query(db: &Db) -> Result<()> {
    let scalar = db.table("ranges")?.select(&["value"])?;
    let attached = db
        .table("ranges")?
        .where_between_columns(&scalar, ["lower_bound", "upper_bound"])?;
    assert_eq!(attached.count().await?, 1);
    Ok(())
}

#[tokio::test]
async fn attached_subquery_cannot_be_rebound_or_laundered() -> Result<()> {
    let (_left_directory, left) = file_database(15).await?;
    let (_right_directory, right) = file_database(99).await?;
    // 普通池查询须先于占用唯一物理连接的事务。
    assert_valid_file_range_query(&left).await?;
    let left_tx = left
        .primary_connection()?
        .begin()
        .await
        .map_err(Error::internal)?;
    let right_tx = right
        .primary_connection()?
        .begin()
        .await
        .map_err(Error::internal)?;

    {
        let scalar = left.table("ranges")?.select(&["value"])?;
        let attached = left
            .table("ranges")?
            .where_between_columns(&scalar, ["lower_bound", "upper_bound"])?;
        // 已经附着的非事务标量不能随后改解释为任一事务中的同名表。
        let rebound = attached.clone().using_transaction(&right_tx);
        assert_rebound_terminals_rejected(rebound.clone()).await;
        let same_db_rebind = attached.clone().using_transaction(&left_tx);
        assert!(
            matches!(same_db_rebind.get().await, Err(error) if error.code() == "query_subquery_context_mismatch")
        );

        // invalid 状态必须穿过 scalar/EXISTS/UNION 包装边界，不能被 scoped_select 擦掉。
        let wrapped = left
            .table("ranges")?
            .using_transaction(&right_tx)
            .where_between_columns(&rebound, ["lower_bound", "upper_bound"]);
        assert!(matches!(wrapped, Err(error) if error.code() == "query_subquery_context_mismatch"));
        assert!(
            matches!(left.table("ranges")?.where_exists(&rebound), Err(error) if error.code() == "query_subquery_context_mismatch")
        );
        assert!(
            matches!(left.table("ranges")?.union(&rebound, true), Err(error) if error.code() == "query_subquery_context_mismatch")
        );
        assert!(
            matches!(left.table("ranges")?.using_transaction(&right_tx).where_exists(&attached), Err(error) if error.code() == "query_subquery_context_mismatch")
        );
        assert!(
            matches!(right.table("ranges")?.union(&attached, true), Err(error) if error.code() == "query_subquery_context_mismatch")
        );

        // 包装成功后再改事务，以及同一有效事务先附着再换另一事务，都应保持拒绝。
        let wrapped = left
            .table("ranges")?
            .where_exists(&attached)?
            .using_transaction(&right_tx);
        assert!(
            matches!(wrapped.get().await, Err(error) if error.code() == "query_subquery_context_mismatch")
        );
        assert!(
            matches!(wrapped.first().await, Err(error) if error.code() == "query_subquery_context_mismatch")
        );
        let united = left
            .table("ranges")?
            .union(&attached, true)?
            .using_transaction(&right_tx);
        assert!(
            matches!(united.get().await, Err(error) if error.code() == "query_subquery_context_mismatch")
        );
        let nested = left
            .table("ranges")?
            .where_nested(|group| {
                group.where_between_columns(&scalar, ["lower_bound", "upper_bound"])
            })?
            .using_transaction(&right_tx);
        assert!(
            matches!(nested.get().await, Err(error) if error.code() == "query_subquery_context_mismatch")
        );
        let scalar_tx = left
            .table("ranges")?
            .using_transaction(&left_tx)
            .select(&["value"])?;
        let attached_tx = left
            .table("ranges")?
            .using_transaction(&left_tx)
            .where_between_columns(&scalar_tx, ["lower_bound", "upper_bound"])?;
        assert_eq!(
            attached_tx
                .clone()
                .using_transaction(&left_tx)
                .count()
                .await?,
            1
        );
        let rebound_tx = attached_tx.using_transaction(&right_tx);
        assert!(
            matches!(rebound_tx.get().await, Err(error) if error.code() == "query_subquery_context_mismatch")
        );
        // 再绑回原事务不能洗掉已经发生的非法重绑定。
        assert!(
            matches!(rebound_tx.using_transaction(&left_tx).get().await, Err(error) if error.code() == "query_subquery_context_mismatch")
        );
        assert_transaction_values_unchanged(&left, &right, &left_tx, &right_tx).await?;
    }
    left_tx.rollback().await.map_err(Error::internal)?;
    right_tx.rollback().await.map_err(Error::internal)?;
    Ok(())
}

fn capture_group(
    db: &Db,
    scalar: &QueryBuilder<'static, 'static>,
) -> Result<WhereGroup<'static, 'static>> {
    let mut saved = None;
    let result = db.table("ranges")?.where_nested(|group| {
        saved = Some(group.where_between_columns(scalar, ["lower_bound", "upper_bound"])?);
        Err(Error::custom(
            "test_capture_group",
            "retain nested group without attaching",
        ))
    });
    assert!(matches!(result, Err(error) if error.code() == "test_capture_group"));
    saved.ok_or_else(|| Error::custom("test_group_missing", "nested group was not captured"))
}

#[tokio::test]
async fn callback_cannot_return_a_group_captured_from_another_context() -> Result<()> {
    let (_left_directory, left) = file_database(15).await?;
    let (_right_directory, right) = file_database(99).await?;
    let scalar = left.table("ranges")?.select(&["value"])?;
    let saved = capture_group(&left, &scalar)?;
    let result = right.table("ranges")?.where_nested(|_| Ok(saved));
    assert!(matches!(result, Err(error) if error.code() == "query_nested_context_mismatch"));
    let saved = capture_group(&left, &scalar)?;
    let result = left.table("another_table")?.where_nested(|_| Ok(saved));
    assert!(matches!(result, Err(error) if error.code() == "query_nested_context_mismatch"));
    // 同一 handle、表和事务上下文的组仍可正常被接纳。
    let saved = capture_group(&left, &scalar)?;
    assert_eq!(
        ids(left.table("ranges")?.where_nested(|_| Ok(saved))?).await?,
        [1]
    );
    assert_eq!(left.table("ranges")?.value::<i64>("value").await?, Some(15));
    assert_eq!(
        right.table("ranges")?.value::<i64>("value").await?,
        Some(99)
    );
    Ok(())
}
