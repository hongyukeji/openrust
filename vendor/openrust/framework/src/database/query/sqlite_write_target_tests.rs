//! 发现写入丢join/limit、投影绑定泄漏、rowid假定及事务状态丢失的真实SQLite回归。

use super::*;
use crate::database::{
    DatabaseTableNames, Db,
    query::{JoinKind, RawSql, SortDirection},
};
use sea_orm::{ConnectOptions, ConnectionTrait, Database, TransactionTrait, Value};
use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

async fn database() -> Result<Db> {
    let mut options = ConnectOptions::new("sqlite::memory:");
    options.max_connections(1);
    let connection = Database::connect(options)
        .await
        .map_err(crate::Error::internal)?;
    let version = connection
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DbBackend::Sqlite,
            "SELECT sqlite_version() AS version".to_owned(),
        ))
        .await
        .map_err(crate::Error::internal)?
        .ok_or_else(|| crate::Error::custom("test_sqlite_version_missing", "缺少实际SQLite版本"))?;
    let version: String = version
        .try_get("", "version")
        .map_err(crate::Error::internal)?;
    eprintln!("SQLite write-target engine: {version}");
    if let Ok(expected) = std::env::var("OPENRUST_EXPECT_SQLITE_VERSION_PREFIX") {
        assert!(
            version.starts_with(&expected),
            "actual={version}, expected prefix={expected}"
        );
    }
    let db = Db::default();
    db.initialize(connection)?;
    Ok(db)
}

async fn exec(db: &Db, sql: &str) -> Result<()> {
    db.primary_connection()?
        .execute_unprepared(sql)
        .await
        .map_err(crate::Error::internal)?;
    Ok(())
}

fn native_failure(result: Result<u64>, fragments: &[&str]) -> Result<()> {
    match result {
        Err(crate::Error::Internal(source)) => {
            let message = source.to_string();
            for fragment in fragments {
                assert!(
                    message.contains(fragment),
                    "SQLite error did not contain {fragment}: {message}"
                );
            }
            Ok(())
        }
        Err(error) => Err(error),
        Ok(affected) => Err(crate::Error::custom(
            "test_sqlite_native_failure_missing",
            format!("应原生失败却写入{affected}行"),
        )),
    }
}

async fn seeded() -> Result<Db> {
    let db = database().await?;
    exec(
        &db,
        "CREATE TABLE items(id INTEGER PRIMARY KEY, priority INTEGER, mark INTEGER)",
    )
    .await?;
    exec(
        &db,
        "INSERT INTO items VALUES(1,10,0),(2,20,0),(3,30,0),(4,40,0)",
    )
    .await?;
    exec(&db, "CREATE TABLE permit(item_id INTEGER, visible INTEGER)").await?;
    exec(
        &db,
        "INSERT INTO permit VALUES(1,1),(2,0),(3,1),(3,1),(4,1)",
    )
    .await?;
    Ok(db)
}

#[tokio::test]
#[ignore = "发布前SQLite当前及最低版本矩阵；日常仅真实MySQL8.4"]
async fn compilation_replaces_only_projection_and_orders_set_then_where_then_limit_bindings()
-> Result<()> {
    let db = seeded().await?;
    let query = db
        .table("items")?
        .select_raw(RawSql::new("? AS ignored_projection"), vec![999i32.into()])?
        .join("permit", "id", "item_id", JoinKind::Inner)?
        .where_eq("visible", 1i32)?
        .group_by("id")?
        .having_eq("visible", 1i32)?
        .order_by("priority", SortDirection::Desc)?
        .limit(1);
    let original_sql = query.to_sql()?;
    let original_bindings = query.get_bindings()?;
    let state = query.snapshot()?;
    let values = vec![(Alias::new("mark"), Expr::val(7i32))];
    let (sql, bindings) = state.sqlite_compile_update(&values, true)?;
    assert!(
        sql.starts_with("UPDATE \"items\" SET \"mark\" = ?"),
        "{sql}"
    );
    assert!(
        sql.contains("\"rowid\" IN (SELECT \"items\".\"rowid\""),
        "{sql}"
    );
    assert!(sql.contains("INNER JOIN \"permit\""), "{sql}");
    assert!(sql.contains("ORDER BY \"priority\" DESC"), "{sql}");
    assert!(sql.contains("GROUP BY \"id\""), "{sql}");
    assert!(sql.contains("HAVING"), "{sql}");
    assert_eq!(
        bindings.0,
        vec![
            Value::from(7i32),
            Value::from(1i32),
            Value::from(1i32),
            Value::from(1u64)
        ]
    );
    let (_, bindings) = state.sqlite_compile_delete(true)?;
    assert_eq!(
        bindings.0,
        vec![Value::from(1i32), Value::from(1i32), Value::from(1u64)]
    );
    assert_eq!(query.to_sql()?, original_sql);
    assert_eq!(query.get_bindings()?, original_bindings);
    Ok(())
}

#[tokio::test]
#[ignore = "发布前SQLite当前及最低版本矩阵；日常仅真实MySQL8.4"]
async fn joined_ordered_limited_offset_writes_use_same_target_set_without_projection_leak()
-> Result<()> {
    let db = seeded().await?;
    let distinct = db
        .table("items")?
        .join("permit", "id", "item_id", JoinKind::Inner)?
        .where_eq("visible", 1i32)?
        .distinct()
        .order_by("priority", SortDirection::Desc)?
        .limit(1)
        .offset(2);
    assert_eq!(distinct.update(vec![("mark", 6i32.into())]).await?, 1);
    assert_eq!(
        db.table("items")?
            .where_eq("mark", 6i32)?
            .pluck::<i64>("id")
            .await?,
        vec![1]
    );
    let query = db
        .table("items")?
        .join("permit", "id", "item_id", JoinKind::Inner)?
        .where_eq("visible", 1i32)?
        .order_by("priority", SortDirection::Desc)?
        .limit(1)
        .offset(1);
    assert_eq!(query.update(vec![("mark", 7i32.into())]).await?, 1);
    assert_eq!(
        db.table("items")?
            .where_eq("mark", 7i32)?
            .pluck::<i64>("id")
            .await?,
        vec![3]
    );
    assert_eq!(query.delete().await?, 1);
    assert_eq!(db.table("items")?.count().await?, 3);
    assert_eq!(db.table("items")?.where_eq("id", 2i32)?.count().await?, 1);
    Ok(())
}

#[tokio::test]
#[ignore = "发布前SQLite当前及最低版本矩阵；日常仅真实MySQL8.4"]
async fn join_without_limit_also_targets_join_members_not_all_where_rows() -> Result<()> {
    let db = seeded().await?;
    let query = db
        .table("items")?
        .join("permit", "id", "item_id", JoinKind::Inner)?
        .where_eq("visible", 1i32)?;
    assert_eq!(query.update(vec![("mark", 5i32.into())]).await?, 3);
    assert_eq!(query.delete().await?, 3);
    assert_eq!(db.table("items")?.pluck::<i64>("id").await?, vec![2]);
    Ok(())
}

#[tokio::test]
#[ignore = "发布前SQLite当前及最低版本矩阵；日常仅真实MySQL8.4"]
async fn limit_zero_is_present_but_order_or_offset_alone_follows_official_direct_write_path()
-> Result<()> {
    let db = seeded().await?;
    let none = db.table("items")?.limit(0);
    assert_eq!(none.update(vec![("mark", 9i32.into())]).await?, 0);
    assert_eq!(none.delete().await?, 0);
    assert_eq!(
        db.table("items")?
            .order_by("id", SortDirection::Desc)?
            .update(vec![("mark", 1i32.into())])
            .await?,
        4
    );
    assert_eq!(db.table("items")?.offset(2).delete().await?, 4);
    Ok(())
}

#[tokio::test]
#[ignore = "发布前SQLite当前及最低版本矩阵；日常仅真实MySQL8.4"]
async fn joined_offset_without_limit_propagates_sqlite_error_without_writing() -> Result<()> {
    let db = seeded().await?;
    let query = db
        .table("items")?
        .join("permit", "id", "item_id", JoinKind::Inner)?
        .offset(1);
    let state = query.snapshot()?;
    let (sql, _) = state.sqlite_compile_delete(true)?;
    assert!(
        sql.contains(" OFFSET ") && !sql.contains(" LIMIT "),
        "{sql}"
    );
    assert!(query.update(vec![("mark", 9i32.into())]).await.is_err());
    assert!(query.delete().await.is_err());
    assert_eq!(db.table("items")?.count().await?, 4);
    assert_eq!(db.table("items")?.where_eq("mark", 9i32)?.count().await?, 0);
    Ok(())
}

#[tokio::test]
#[ignore = "发布前SQLite当前及最低版本矩阵；日常仅真实MySQL8.4"]
async fn declared_casefolded_rowid_is_not_silently_replaced_by_hidden_unique_rowid() -> Result<()> {
    let db = database().await?;
    exec(
        &db,
        "CREATE TABLE shadow(id INTEGER PRIMARY KEY, ROWID INTEGER, mark INTEGER)",
    )
    .await?;
    exec(&db, "INSERT INTO shadow VALUES(1,7,0),(2,7,0),(3,8,0)").await?;
    let query = db
        .table("shadow")?
        .order_by("id", SortDirection::Asc)?
        .limit(1);
    // 固定Laravel字面rowid解析为声明列；选中7会命中两个同值行，并非物理行唯一性保证。
    assert_eq!(query.update(vec![("mark", 1i32.into())]).await?, 2);
    assert_eq!(
        db.table("shadow")?.where_eq("mark", 1i32)?.count().await?,
        2
    );
    assert_eq!(query.delete().await?, 2);
    assert_eq!(db.table("shadow")?.pluck::<i64>("id").await?, vec![3]);
    Ok(())
}

#[tokio::test]
#[ignore = "发布前SQLite当前及最低版本矩阵；日常仅真实MySQL8.4"]
async fn without_rowid_missing_name_fails_natively_without_broad_write_or_primary_key_fallback()
-> Result<()> {
    let db = database().await?;
    exec(
        &db,
        "CREATE TABLE no_rowid(key TEXT PRIMARY KEY, mark INTEGER) WITHOUT ROWID",
    )
    .await?;
    exec(&db, "INSERT INTO no_rowid VALUES('first',0),('second',0)").await?;
    assert_eq!(
        db.table("no_rowid")?
            .update(vec![("mark", 1i32.into())])
            .await?,
        2
    );
    native_failure(
        db.table("no_rowid")?
            .limit(1)
            .update(vec![("mark", 9i32.into())])
            .await,
        &["no such column", "rowid"],
    )?;
    assert_eq!(
        db.table("no_rowid")?
            .where_eq("mark", 1i32)?
            .count()
            .await?,
        2
    );
    native_failure(
        db.table("no_rowid")?.limit(1).delete().await,
        &["no such column", "rowid"],
    )?;
    assert_eq!(db.table("no_rowid")?.count().await?, 2);
    exec(
        &db,
        "CREATE TABLE named_rowid(rowid INTEGER PRIMARY KEY, mark INTEGER) WITHOUT ROWID",
    )
    .await?;
    exec(&db, "INSERT INTO named_rowid VALUES(7,0),(8,0)").await?;
    let query = db
        .table("named_rowid")?
        .order_by("rowid", SortDirection::Asc)?
        .limit(1);
    assert_eq!(query.update(vec![("mark", 1i32.into())]).await?, 1);
    assert_eq!(query.delete().await?, 1);
    Ok(())
}

#[tokio::test]
#[ignore = "发布前SQLite当前及最低版本矩阵；日常仅真实MySQL8.4"]
async fn clone_and_transaction_transfer_keep_join_flag_and_rollback_actual_writes() -> Result<()> {
    let db = seeded().await?;
    let transaction = db
        .primary_connection()?
        .begin()
        .await
        .map_err(crate::Error::internal)?;
    {
        let query = db
            .table("items")?
            .join("permit", "id", "item_id", JoinKind::Inner)?
            .where_eq("visible", 1i32)?
            .using_transaction(&transaction);
        let cloned = query.clone();
        assert_eq!(cloned.update(vec![("mark", 3i32.into())]).await?, 3);
        assert_eq!(query.delete().await?, 3);
    }
    transaction
        .rollback()
        .await
        .map_err(crate::Error::internal)?;
    assert_eq!(db.table("items")?.count().await?, 4);
    assert_eq!(db.table("items")?.where_eq("mark", 0i32)?.count().await?, 4);
    Ok(())
}

#[tokio::test]
#[ignore = "发布前SQLite当前及最低版本矩阵；日常仅真实MySQL8.4"]
async fn before_callback_is_consumed_once_and_frozen_child_binding_growth_blocks_both_writes()
-> Result<()> {
    let db = seeded().await?;
    let called = Arc::new(AtomicUsize::new(0));
    let trace = Arc::clone(&called);
    let query = db.table("items")?.before_query(move |query| {
        trace.fetch_add(1, Ordering::SeqCst);
        *query = query
            .clone()
            .join("permit", "id", "item_id", JoinKind::Inner)?
            .where_eq("visible", 1i32)?;
        Ok(())
    });
    assert_eq!(query.update(vec![("mark", 2i32.into())]).await?, 3);
    assert_eq!(query.delete().await?, 3);
    assert_eq!(called.load(Ordering::SeqCst), 1);
    let child = db.table("permit")?.select(&["item_id"])?;
    let parent = db.table("items")?.where_exists(&child)?.limit(1);
    let child = child.where_eq("visible", 1i32)?;
    assert!(
        matches!(parent.update(vec![("mark", 99i32.into())]).await, Err(e) if e.code() == "query_subquery_bindings_mismatch")
    );
    assert!(
        matches!(parent.delete().await, Err(e) if e.code() == "query_subquery_bindings_mismatch")
    );
    assert_eq!(
        db.table("items")?.where_eq("mark", 99i32)?.count().await?,
        0
    );
    assert_eq!(db.table("items")?.count().await?, 1);
    let _ = child;
    Ok(())
}

#[tokio::test]
#[ignore = "发布前SQLite当前及最低版本矩阵；日常仅真实MySQL8.4"]
async fn union_branches_are_preserved_and_projection_arity_errors_are_not_swallowed() -> Result<()>
{
    let db = seeded().await?;
    let child = db
        .query()
        .select_raw(RawSql::new("? AS selected_rowid"), vec![4i32.into()])?;
    let query = db
        .table("items")?
        .select_raw(
            RawSql::new("? AS removed_base_projection"),
            vec![999i32.into()],
        )?
        .where_eq("id", 1i32)?
        .limit(1)
        .union(&child, false)?;
    let original_bindings = query.get_bindings()?;
    let (_, compiled_bindings) = query
        .snapshot()?
        .sqlite_compile_update(&[(Alias::new("mark"), Expr::val(8i32))], true)?;
    assert_eq!(
        compiled_bindings.0,
        vec![
            Value::from(8i32),
            Value::from(1i32),
            Value::from(1u64),
            Value::from(4i32)
        ]
    );
    assert_eq!(query.update(vec![("mark", 8i32.into())]).await?, 2);
    assert_eq!(query.get_bindings()?, original_bindings);
    assert_eq!(db.table("items")?.where_eq("mark", 8i32)?.count().await?, 2);
    assert_eq!(query.delete().await?, 2);
    let invalid_child = db.table("items")?.select(&["id", "mark"])?;
    let invalid = db.table("items")?.limit(1).union(&invalid_child, false)?;
    native_failure(
        invalid.update(vec![("mark", 9i32.into())]).await,
        &["same number", "columns"],
    )?;
    native_failure(invalid.delete().await, &["same number", "columns"])?;
    assert_eq!(db.table("items")?.count().await?, 2);
    assert_eq!(db.table("items")?.where_eq("mark", 9i32)?.count().await?, 0);
    Ok(())
}

#[tokio::test]
#[ignore = "发布前SQLite当前及最低版本矩阵；日常仅真实MySQL8.4"]
async fn physical_table_prefix_is_used_on_both_outer_table_and_target_rowid_projection()
-> Result<()> {
    let plain = database().await?;
    let db = Db::with_table_names(DatabaseTableNames::new("p_", BTreeMap::new())?);
    db.initialize(plain.primary_connection()?)?;
    exec(
        &db,
        "CREATE TABLE p_items(id INTEGER PRIMARY KEY, mark INTEGER)",
    )
    .await?;
    exec(&db, "INSERT INTO p_items VALUES(1,0),(2,0)").await?;
    assert_eq!(
        db.table("items")?
            .order_by("id", SortDirection::Asc)?
            .limit(1)
            .update(vec![("mark", 1i32.into())])
            .await?,
        1
    );
    assert_eq!(db.table("items")?.where_eq("mark", 1i32)?.count().await?, 1);
    assert_eq!(
        db.table("items")?
            .order_by("id", SortDirection::Asc)?
            .limit(1)
            .delete()
            .await?,
        1
    );
    assert_eq!(db.table("items")?.count().await?, 1);
    Ok(())
}

#[tokio::test]
#[ignore = "发布前SQLite当前及最低版本矩阵；日常仅真实MySQL8.4"]
async fn union_branch_limit_zero_does_not_limit_entire_update_or_delete() -> Result<()> {
    let db = seeded().await?;
    let child = db
        .table("items")?
        .select(&["id"])?
        .where_eq("id", 4i32)?
        .limit(0);
    let query = db
        .table("items")?
        .where_eq("id", 1i32)?
        .limit(1)
        .union(&child, true)?;
    assert_eq!(query.update(vec![("mark", 41i32.into())]).await?, 1);
    assert_eq!(
        db.table("items")?
            .where_eq("id", 1i32)?
            .where_eq("mark", 41i32)?
            .count()
            .await?,
        1
    );
    assert_eq!(query.delete().await?, 1);
    assert_eq!(db.table("items")?.where_eq("id", 4i32)?.count().await?, 1);
    assert_eq!(db.table("items")?.count().await?, 3);
    Ok(())
}

#[tokio::test]
#[ignore = "发布前SQLite当前及最低版本矩阵；日常仅真实MySQL8.4"]
async fn nested_union_branches_keep_local_limits_and_frozen_binding_order() -> Result<()> {
    let db = seeded().await?;
    let leaf = db
        .table("items")?
        .select(&["id"])?
        .where_eq("id", 4i32)?
        .limit(0);
    let child = db
        .table("items")?
        .select(&["id"])?
        .where_eq("id", 3i32)?
        .limit(1)
        .union(&leaf, true)?;
    let query = db
        .table("items")?
        .where_eq("id", 1i32)?
        .limit(1)
        .union(&child, true)?;
    let (_, values) = query
        .snapshot()?
        .sqlite_compile_update(&[(Alias::new("mark"), Expr::val(42i32))], true)?;
    assert_eq!(
        values.0,
        vec![
            42i32.into(),
            1i32.into(),
            1u64.into(),
            3i32.into(),
            1u64.into(),
            4i32.into(),
            0u64.into()
        ]
    );
    assert_eq!(query.update(vec![("mark", 42i32.into())]).await?, 2);
    assert_eq!(
        db.table("items")?
            .where_eq("id", 4i32)?
            .where_eq("mark", 0i32)?
            .count()
            .await?,
        1
    );
    assert_eq!(query.delete().await?, 2);
    assert_eq!(db.table("items")?.count().await?, 2);
    Ok(())
}

#[tokio::test]
#[ignore = "发布前SQLite当前及最低版本矩阵；日常仅真实MySQL8.4"]
async fn direct_write_rejects_unused_union_and_having_business_bindings_before_sqlx_can_ignore_them()
-> Result<()> {
    let db = seeded().await?;
    let child = db.query().select_raw(RawSql::new("?"), vec![4i32.into()])?;
    let query = db.table("items")?.union(&child, true)?.limit(1);
    // 只设union_limit不会触发官方rowid改写；多余业务绑定必须报错，不能变成全表写。
    assert!(
        matches!(query.update(vec![("mark",43i32.into())]).await, Err(error) if error.code()=="query_sqlite_unused_write_bindings")
    );
    assert!(
        matches!(query.delete().await, Err(error) if error.code()=="query_sqlite_unused_write_bindings")
    );
    let having = db.table("items")?.having_eq("id", 1i32)?;
    assert!(
        matches!(having.update(vec![("mark",43i32.into())]).await, Err(error) if error.code()=="query_sqlite_unused_write_bindings")
    );
    assert!(
        matches!(having.delete().await, Err(error) if error.code()=="query_sqlite_unused_write_bindings")
    );
    assert_eq!(db.table("items")?.count().await?, 4);
    assert_eq!(db.table("items")?.where_eq("mark", 0i32)?.count().await?, 4);
    Ok(())
}

#[tokio::test]
#[ignore = "发布前SQLite当前及最低版本矩阵；日常仅真实MySQL8.4"]
async fn direct_write_does_not_treat_pagination_or_bound_where_as_unused_business_bindings()
-> Result<()> {
    let db = seeded().await?;
    let unused_callback_count = Arc::new(AtomicUsize::new(0));
    let callback_count = Arc::clone(&unused_callback_count);
    let child = db
        .query()
        .select_raw(RawSql::new("4"), vec![])?
        .limit(1)
        .offset(0)
        .before_query(move |_| {
            callback_count.fetch_add(1, Ordering::SeqCst);
            Err(crate::Error::custom(
                "unused_union_callback_must_not_run",
                "直接写不编译UNION",
            ))
        });
    let exists_leaf = db
        .query()
        .select_raw(RawSql::new("?"), vec![2i32.into()])?
        .limit(1);
    let exists = db
        .table("items")?
        .select(&["id"])?
        .where_eq("id", 2i32)?
        .limit(1)
        .union(&exists_leaf, true)?;
    let query = db
        .table("items")?
        .where_eq("id", 1i32)?
        .where_exists(&exists)?
        .union(&child, true)?
        .limit(1);
    assert_eq!(query.update(vec![("mark", 44i32.into())]).await?, 1);
    assert_eq!(query.delete().await?, 1);
    assert_eq!(db.table("items")?.count().await?, 3);
    assert_eq!(unused_callback_count.load(Ordering::SeqCst), 0);
    Ok(())
}
