//! 验证生成应用的 SQLite 联表、分页与 JSON 包含查询的读写范围。

use openrust::{
    Error, Result,
    database::{
        Db, JoinKind, SortDirection,
        sea_orm::{ConnectionTrait, DbBackend, QueryResult, Statement, Value},
        sea_orm_migration::{MigrationTrait, MigratorTrait},
    },
    testing::TestDatabase,
};
use serde_json::json;

struct EmptyMigrator;

impl MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}

#[tokio::test]
#[ignore = "SQLite 实库留待发布前当前版与最低版本矩阵"]
async fn joined_and_limited_writes_select_only_the_intended_rows() -> Result<()> {
    let database = TestDatabase::sqlite_file::<EmptyMigrator>().await?;
    let connection = database.connection();
    let outcome = tokio::spawn(async move {
        let db = Db::default();
        db.initialize(connection?)?;
        let connection = db.primary_connection()?;
        for sql in [
            "CREATE TABLE tasks (id INTEGER PRIMARY KEY, completed INTEGER NOT NULL)",
            "CREATE TABLE permitted_tasks (task_id INTEGER NOT NULL)",
            "INSERT INTO tasks VALUES (1, 0), (2, 0), (3, 0)",
            "INSERT INTO permitted_tasks VALUES (2), (3)",
        ] {
            connection
                .execute_unprepared(sql)
                .await
                .map_err(Error::internal)?;
        }

        let tasks = db.table("tasks")?;
        assert_eq!(
            tasks
                .clone()
                .join("permitted_tasks", "id", "task_id", JoinKind::Inner)?
                .order_by("id", SortDirection::Desc)?
                .limit(1)
                .update(vec![("completed", 1_i64.into())])
                .await?,
            1
        );
        assert_eq!(
            tasks
                .clone()
                .where_eq("id", 3_i64)?
                .where_eq("completed", 1_i64)?
                .count()
                .await?,
            1
        );
        assert_eq!(
            tasks.clone().where_eq("completed", 1_i64)?.count().await?,
            1
        );
        assert_eq!(tasks.clone().limit(0).delete().await?, 0);
        assert_eq!(
            tasks
                .clone()
                .join("permitted_tasks", "id", "task_id", JoinKind::Inner)?
                .where_eq("id", 2_i64)?
                .delete()
                .await?,
            1
        );
        assert_eq!(tasks.clone().where_eq("id", 1_i64)?.count().await?, 1);
        assert_eq!(tasks.where_eq("id", 2_i64)?.count().await?, 0);
        Ok::<(), Error>(())
    })
    .await;
    close_after_test(database, outcome).await
}

#[tokio::test]
#[ignore = "SQLite JSON1 实库留待发布前当前版与最低版本矩阵"]
async fn generated_sqlite_json_contains_preserves_bindings_and_exact_read_write_targets()
-> Result<()> {
    let expected_version = std::env::var("OPENRUST_EXPECT_SQLITE_VERSION_PREFIX")
        .ok()
        .filter(|version| !version.trim().is_empty())
        .ok_or_else(|| {
            Error::custom(
                "sqlite_json_test_version_required",
                "请设置 OPENRUST_EXPECT_SQLITE_VERSION_PREFIX 以核对实际引擎版本",
            )
        })?;
    let database = TestDatabase::sqlite_file::<EmptyMigrator>().await?;
    let connection = database.connection();
    let outcome = tokio::spawn(async move {
        let db = Db::default();
        db.initialize(connection?)?;
        assert_sqlite_engine(&db, &expected_version).await?;

        let literal = "中文 ' OR 1=1 -- ? $1";
        let label = "visible";
        seed_json_records(&db, literal, label).await?;
        let table = db.table("json_records")?;
        let query = table
            .clone()
            .where_eq("owner_id", 7_i64)?
            .where_json_contains("payload", json!(literal))?
            .where_eq("label", label)?;
        assert!(!query.to_sql()?.contains(literal));
        assert_eq!(
            query.get_bindings()?,
            vec![7_i64.into(), literal.into(), label.into()]
        );
        assert_eq!(row_ids(&query.get().await?)?, [1]);
        assert_eq!(query.count().await?, 1);

        // SQLite JSON1 把布尔展开为整数。浮点和越界整数的参数遵循 PDO 的 TEXT 绑定。
        for (needle, binding, expected) in [
            (json!(true), 1_i64.into(), &[4, 5][..]),
            (json!(false), 0_i64.into(), &[8][..]),
            (json!(1), 1_i64.into(), &[4, 5][..]),
            (json!("1"), "1".into(), &[15][..]),
            (
                json!(1.234_567_890_123_456),
                "1.2345678901235".into(),
                &[7][..],
            ),
            (
                json!(9_223_372_036_854_775_808_u64),
                "9.2233720368548E+18".into(),
                &[12][..],
            ),
            (serde_json::Value::Null, Value::String(None), &[9, 10][..]),
        ] {
            let matches = table
                .clone()
                .where_eq("owner_id", 7_i64)?
                .where_json_contains("payload", needle)?
                .where_eq("label", label)?
                .order_by("id", SortDirection::Asc)?;
            assert_eq!(
                matches.get_bindings()?,
                vec![7_i64.into(), binding, label.into()]
            );
            assert_eq!(row_ids(&matches.get().await?)?.as_slice(), expected);
        }
        assert_eq!(
            row_ids(&table.clone().where_null("payload")?.get().await?)?,
            [11]
        );
        assert_sqlite_json_key_paths(&db, label).await?;

        assert_eq!(query.update(vec![("flag", 9_i64.into())]).await?, 1);
        assert_eq!(
            row_ids(&table.clone().where_eq("flag", 9_i64)?.get().await?)?,
            [1]
        );
        assert_eq!(query.delete().await?, 1);
        let remaining = table.order_by("id", SortDirection::Asc)?.get().await?;
        assert_eq!(row_ids(&remaining)?, (2..=15).collect::<Vec<_>>());
        Ok(())
    })
    .await;
    close_after_test(database, outcome).await
}

async fn assert_sqlite_engine(db: &Db, expected_version: &str) -> Result<()> {
    let engine = db
        .primary_connection()?
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT sqlite_version() AS version, sqlite_source_id() AS source_id",
        ))
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| {
            Error::custom("sqlite_json_test_version_missing", "连接未返回 SQLite 版本")
        })?;
    let version: String = engine.try_get("", "version").map_err(Error::internal)?;
    let source_id: String = engine.try_get("", "source_id").map_err(Error::internal)?;
    assert!(!source_id.is_empty());
    eprintln!("SQLite JSON1 实测引擎 {version}，source_id {source_id}");
    assert!(
        version.starts_with(expected_version),
        "预期 SQLite 版本前缀 {expected_version}，实际 {version}"
    );
    Ok(())
}

async fn seed_json_records(db: &Db, literal: &str, label: &str) -> Result<()> {
    db.primary_connection()?
        .execute_unprepared(
            "CREATE TABLE json_records (id INTEGER PRIMARY KEY, owner_id INTEGER NOT NULL, \
             payload TEXT, label TEXT NOT NULL, flag INTEGER NOT NULL)",
        )
        .await
        .map_err(Error::internal)?;
    for (id, owner, payload, visibility) in [
        (1_i64, 7_i64, Some(json!(literal)), label),
        (2, 8, Some(json!([literal])), label),
        (3, 7, Some(json!([literal])), "other"),
        (4, 7, Some(json!([true])), label),
        (5, 7, Some(json!(1)), label),
        (6, 7, Some(json!(1.234_567_890_123_456)), label),
        (7, 7, Some(json!(["1.2345678901235"])), label),
        (8, 7, Some(json!({"enabled": false})), label),
        (9, 7, Some(serde_json::Value::Null), label),
        (10, 7, Some(json!([null])), label),
        (11, 7, None, label),
        (12, 7, Some(json!("9.2233720368548E+18")), label),
        (13, 7, Some(json!(9_223_372_036_854_775_808_u64)), label),
        (14, 7, Some(json!([])), label),
        (15, 7, Some(json!("1")), label),
    ] {
        db.table("json_records")?
            .insert(vec![
                ("id", id.into()),
                ("owner_id", owner.into()),
                (
                    "payload",
                    Value::String(payload.map(|value| value.to_string())),
                ),
                ("label", visibility.into()),
                ("flag", 0_i64.into()),
            ])
            .await?;
    }
    Ok(())
}

async fn assert_sqlite_json_key_paths(db: &Db, label: &str) -> Result<()> {
    let owner = db.table("json_records")?.where_eq("owner_id", 7_i64)?.where_eq("label", label)?;
    let array = owner.clone().where_json_contains_key("payload->[0]")?.order_by("id", SortDirection::Asc)?;
    assert_eq!(array.get_bindings()?, vec![7_i64.into(), label.into()]);
    assert_eq!(row_ids(&array.get().await?)?, [4, 7, 10]);
    assert_eq!(row_ids(&owner.clone().where_json_contains_key("payload->enabled")?.get().await?)?, [8]);
    assert_eq!(row_ids(&owner.where_json_doesnt_contain_key("payload")?.get().await?)?, [11]);
    Ok(())
}

fn row_ids(rows: &[QueryResult]) -> Result<Vec<i64>> {
    rows.iter()
        .map(|row| row.try_get("", "id").map_err(Error::internal))
        .collect()
}

async fn close_after_test(
    database: TestDatabase,
    outcome: std::result::Result<Result<()>, tokio::task::JoinError>,
) -> Result<()> {
    let outcome = outcome
        .map_err(Error::internal)
        .and_then(std::convert::identity);
    let cleanup = database.close().await;
    match (outcome, cleanup) {
        (Ok(()), cleanup) => cleanup,
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup_error)) => {
            eprintln!("SQLite 生成测试清理失败：{}", cleanup_error.code());
            Err(error)
        }
    }
}
