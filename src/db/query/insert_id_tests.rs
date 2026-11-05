//! 本次执行 ID、写连接路由、事务及原查询回调租约回归。

use super::super::*;
use sea_orm::TransactionTrait;
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
fn unsigned(value: &Value) -> Result<u64> {
    match value {
        Value::BigInt(Some(value)) if *value >= 0 => u64::try_from(*value).map_err(Error::internal),
        _ => Err(Error::custom(
            "test_insert_id_type",
            "插入 ID 未按 PHP 64 位整数转换",
        )),
    }
}

#[cfg(feature = "db-sqlite")]
fn sqlite_id(value: &Value) -> Result<i64> {
    match value {
        Value::BigInt(Some(value)) => Ok(*value),
        _ => Err(Error::custom(
            "test_insert_id_type",
            "SQLite 插入 ID 未保留原生有符号执行结果",
        )),
    }
}

#[cfg(feature = "db-sqlite")]
async fn sqlite_connection() -> Result<sea_orm::DatabaseConnection> {
    sea_orm::Database::connect("sqlite::memory:")
        .await
        .map_err(Error::internal)
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_default_row_same_execution_large_id_and_ignored_sequence() -> Result<()> {
    let connection = sqlite_connection().await?;
    connection.execute_unprepared("CREATE TABLE generated_items (id INTEGER PRIMARY KEY AUTOINCREMENT, label TEXT DEFAULT 'default')").await.map_err(Error::internal)?;
    let db = Db::default();
    db.initialize(connection)?;
    let query = db.table("generated_items")?;
    assert_eq!(sqlite_id(&query.insert_get_id(Vec::new(), None).await?)?, 1);
    assert_eq!(
        query.where_eq("id", 1)?.value::<String>("label").await?,
        Some("default".into())
    );
    // ID 大于 JavaScript 精确整数范围；不得经过 JSON/f64 或二次 last-ID 查询。
    let large = 9_007_199_254_740_993i64;
    assert_eq!(
        sqlite_id(
            &db.table("generated_items")?
                .insert_get_id(
                    vec![("id", large.into())],
                    Some("ignored;not_an_identifier")
                )
                .await?
        )?,
        large
    );
    assert_eq!(
        sqlite_id(
            &db.table("generated_items")?
                .insert_get_id(Vec::new(), None)
                .await?
        )?,
        large + 1
    );
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_insert_id_routes_to_primary_and_preserves_transaction_rollback() -> Result<()> {
    let primary = sqlite_connection().await?;
    primary
        .execute_unprepared(
            "CREATE TABLE generated_items (id INTEGER PRIMARY KEY AUTOINCREMENT, label TEXT)",
        )
        .await
        .map_err(Error::internal)?;
    let replica = sqlite_connection().await?;
    // replica 故意没有此表；错走读连接会立即失败。
    let db = Db::default();
    db.initialize_with_replicas(primary.clone(), vec![replica])?;
    assert_eq!(
        sqlite_id(
            &db.table("generated_items")?
                .insert_get_id(vec![("label", "primary".into())], None)
                .await?
        )?,
        1
    );
    let tx = primary.begin().await.map_err(Error::internal)?;
    let query = db.table("generated_items")?.using_transaction(&tx);
    assert_eq!(
        sqlite_id(
            &query
                .insert_get_id(vec![("label", "rolled back".into())], None)
                .await?
        )?,
        2
    );
    assert_eq!(query.count().await?, 2);
    drop(query);
    tx.rollback().await.map_err(Error::internal)?;
    let row = primary
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS total FROM generated_items".to_owned(),
        ))
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| Error::custom("test_missing_row", "行数查询缺失"))?;
    assert_eq!(row.try_get::<i64>("", "total").map_err(Error::internal)?, 1);
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn insert_id_callbacks_apply_once_and_share_original_busy_lease() -> Result<()> {
    let connection = sqlite_connection().await?;
    connection
        .execute_unprepared("CREATE TABLE generated_items (id INTEGER PRIMARY KEY AUTOINCREMENT)")
        .await
        .map_err(Error::internal)?;
    let db = Db::default();
    db.initialize(connection)?;
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let query = db.table("generated_items")?.before_query(move |current| {
        observed.fetch_add(1, Ordering::SeqCst);
        assert!(matches!(current.to_sql(), Err(error) if error.code() == "query_subquery_busy"));
        Ok(())
    });
    assert_eq!(sqlite_id(&query.insert_get_id(Vec::new(), None).await?)?, 1);
    assert_eq!(sqlite_id(&query.insert_get_id(Vec::new(), None).await?)?, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        query
            .insert_get_id(vec![("unsafe;column", 1.into())], None)
            .await
            .is_err()
    );
    assert_eq!(query.count().await?, 2);
    Ok(())
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
struct Migrator;
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
impl crate::db::sea_orm_migration::MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn crate::db::sea_orm_migration::MigrationTrait>> {
        Vec::new()
    }
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
#[tokio::test]
async fn real_insert_id_preserves_primary_returning_types_defaults_and_transaction() -> Result<()> {
    use crate::testing::TestDatabase;
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("跳过真实 insert_get_id：未配置 OPENRUST_TEST_DATABASE_URL");
        return Ok(());
    };
    let database = TestDatabase::create::<Migrator>(&url).await?;
    let replica_database = match TestDatabase::create::<Migrator>(&url).await {
        Ok(replica) => replica,
        Err(error) => {
            database.close().await?;
            return Err(error);
        }
    };
    let outcome: Result<()> = async {
        let primary = database.connection()?;
        let backend = primary.get_database_backend();
        let ddl = match backend {
            DbBackend::Postgres => "CREATE TABLE generated_items (id BIGSERIAL PRIMARY KEY, label TEXT DEFAULT 'default')",
            DbBackend::MySql => "CREATE TABLE generated_items (id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY, label TEXT)",
            _ => return Err(unsupported_backend()),
        };
        primary.execute_unprepared(ddl).await.map_err(Error::internal)?;
        let db = Db::default();
        // 独立数据库 replica 没有业务表；任何 INSERT 错走读连接都会失败。
        db.initialize_with_replicas(primary.clone(), vec![replica_database.connection()?])?;
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let query = db.table("generated_items")?.before_query(move |_| { observed.fetch_add(1, Ordering::SeqCst); Ok(()) });
        let first = query.insert_get_id(Vec::new(), None).await?;
        match backend {
            DbBackend::Postgres => assert_eq!(first, Value::BigInt(Some(1))),
            DbBackend::MySql => assert_eq!(unsigned(&first)?, 1),
            _ => return Err(unsupported_backend()),
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let large = 9_007_199_254_740_993i64;
        let returned = query.insert_get_id(vec![("id", large.into())], None).await?;
        match backend {
            DbBackend::Postgres => assert_eq!(returned, Value::BigInt(Some(large))),
            DbBackend::MySql => assert_eq!(unsigned(&returned)?, u64::try_from(large).map_err(Error::internal)?),
            _ => return Err(unsupported_backend()),
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let tx = primary.begin().await.map_err(Error::internal)?;
        let bound = db.table("generated_items")?.using_transaction(&tx);
        bound.insert_get_id(vec![("label", "rollback".into())], None).await?;
        assert_eq!(bound.count().await?, 3);
        drop(bound);
        tx.rollback().await.map_err(Error::internal)?;
        assert_eq!(db.with_primary(query.count()).await??, 2);
        if backend == DbBackend::Postgres {
            primary.execute_unprepared("CREATE TABLE string_keys (key TEXT PRIMARY KEY DEFAULT 'key:default', extra INTEGER DEFAULT 7)").await.map_err(Error::internal)?;
            assert_eq!(db.table("string_keys")?.insert_get_id(Vec::new(), Some("key")).await?, Value::from("key:default"));
            assert_eq!(db.table("string_keys")?.insert_get_id(vec![("key", "000123".into())], Some("key")).await?, Value::BigInt(Some(123)));
            let compiled = db.table("string_keys")?.snapshot()?.compile_insert(vec![("key", "literal-bound".into())], Some("key"))?;
            // SeaQuery cast_as 将类型名作为 SQL 类型表达式渲染，不引用成列标识符。
            if !compiled.0.contains("CAST(\"key\" AS text) AS \"__openrust_insert_id_text\"") {
                return Err(Error::custom(
                    "test_insert_id_text_projection_missing",
                    format!("同一 RETURNING 缺少无损文本投影；实际 SQL：{}", compiled.0),
                ));
            }
            assert!(!compiled.0.contains("literal-bound"));
            assert_eq!(compiled.1.0.len(), 1);
            assert!(db.table("string_keys")?.insert_get_id(vec![("key", "bad".into())], Some("key;DROP")).await.is_err());
            assert_eq!(db.with_primary(db.table("string_keys")?.count()).await??, 2);
            primary.execute_unprepared("CREATE TABLE decimal_keys (id NUMERIC(28,0) PRIMARY KEY DEFAULT 1234567890123456789012345678)").await.map_err(Error::internal)?;
            assert_eq!(db.table("decimal_keys")?.insert_get_id(Vec::new(), None).await?, Value::BigInt(Some(i64::MAX)));
            primary.execute_unprepared("CREATE TABLE fractional_keys (id NUMERIC(40,30) PRIMARY KEY DEFAULT 1234567890.123456789012345678901234567890)").await.map_err(Error::internal)?;
            assert_eq!(db.table("fractional_keys")?.insert_get_id(Vec::new(), None).await?, Value::BigInt(Some(1_234_567_890)));
            assert_eq!(db.table("string_keys")?.insert_get_id(vec![("key", "123tail".into())], Some("key")).await?, Value::from("123tail"));
            assert_eq!(db.table("string_keys")?.insert_get_id(vec![("key", "123e2".into())], Some("key")).await?, Value::BigInt(Some(12_300)));
            for sequence in [Some(""), Some("0")] {
                assert!(matches!(query.insert_get_id(Vec::new(), sequence).await?, Value::BigInt(Some(id)) if id > 0));
            }
            primary.execute_unprepared("CREATE TABLE float_keys (id DOUBLE PRECISION PRIMARY KEY)").await.map_err(Error::internal)?;
            let conversion_error = db.table("float_keys")?.insert_get_id(vec![("id", (9_223_372_036_854_775_808.0f64).into())], None).await.err().ok_or_else(|| Error::custom("test_missing_cast_error", "越界浮点主键未被拒绝"))?;
            assert_eq!(conversion_error.code(), "query_insert_id_numeric_cast_failed");
            assert_eq!(conversion_error.status().as_u16(), 500);
            // 与 Laravel Processor 一样，转换失败时 INSERT 已发生，不隐式回滚。
            assert_eq!(db.with_primary(db.table("float_keys")?.count()).await??, 1);
            assert_eq!(db.table("float_keys")?.insert_get_id(vec![("id", (-1.75f64).into())], None).await?, Value::BigInt(Some(-1)));
        } else {
            assert!(unsigned(&query.insert_get_id(vec![("label", "ignored sequence".into())], Some("ignored;column")).await?)? > u64::try_from(large).map_err(Error::internal)?);
            assert_eq!(query.insert_get_id(vec![("id", u64::MAX.into())], None).await?, Value::BigInt(Some(i64::MAX)));
        }
        Ok(())
    }.await;
    let cleanup = database.close().await;
    let replica_cleanup = replica_database.close().await;
    outcome?;
    cleanup?;
    replica_cleanup
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn insert_id_cannot_bypass_attached_scalar_transaction_provenance() -> Result<()> {
    let connection = sqlite_connection().await?;
    connection
        .execute_unprepared("CREATE TABLE generated_items (id INTEGER PRIMARY KEY AUTOINCREMENT)")
        .await
        .map_err(Error::internal)?;
    let db = Db::default();
    db.initialize(connection.clone())?;
    let child = db.table("generated_items")?.select(&["id"])?;
    let attached = db
        .table("generated_items")?
        .where_between_columns(&child, ["id", "id"])?;
    let tx = connection.begin().await.map_err(Error::internal)?;
    let rebound = attached.using_transaction(&tx);
    assert!(
        matches!(rebound.insert_get_id(Vec::new(), None).await, Err(error) if error.code() == "query_subquery_context_mismatch")
    );
    drop(rebound);
    drop(child);
    tx.rollback().await.map_err(Error::internal)?;
    assert_eq!(db.table("generated_items")?.count().await?, 0);
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_negative_explicit_and_generated_rowids_remain_signed() -> Result<()> {
    let connection = sqlite_connection().await?;
    connection
        .execute_unprepared("CREATE TABLE signed_items (id INTEGER PRIMARY KEY)")
        .await
        .map_err(Error::internal)?;
    let db = Db::default();
    db.initialize(connection)?;
    assert_eq!(
        db.table("signed_items")?
            .insert_get_id(vec![("id", (-10i64).into())], None)
            .await?,
        Value::BigInt(Some(-10))
    );
    assert_eq!(
        db.table("signed_items")?
            .insert_get_id(Vec::new(), None)
            .await?,
        Value::BigInt(Some(-9))
    );
    assert_eq!(db.table("signed_items")?.count().await?, 2);
    Ok(())
}

#[test]
fn processor_numeric_strings_use_full_php_grammar_and_integer_cast_rules() -> Result<()> {
    use super::{float_id, processor_id, text_id};
    for (text, expected) in [
        ("000123", 123),
        ("  +1.9e2\t", 190),
        ("1234567890123456789012345678901234567890", i64::MAX),
        ("-1234567890123456789012345678901234567890", i64::MIN),
        ("1234567890.123456789012345678901234567890", 1_234_567_890),
    ] {
        assert_eq!(text_id(text.into()), Value::BigInt(Some(expected)));
    }
    for text in [
        "123tail",
        "key:123",
        "NaN",
        "Infinity",
        "",
        "123e",
        "123\u{00a0}",
    ] {
        assert_eq!(text_id(text.into()), Value::from(text));
    }
    assert_eq!(
        processor_id(Value::BigUnsigned(Some(u64::MAX)))?,
        Value::BigInt(Some(i64::MAX))
    );
    assert_eq!(
        processor_id(Value::Bool(Some(true)))?,
        Value::Bool(Some(true))
    );
    for (float, expected) in [
        (-1.75, -1),
        (9_223_372_036_854_774_784.0, 9_223_372_036_854_774_784),
    ] {
        assert_eq!(float_id(float)?, Value::BigInt(Some(expected)));
    }
    for float in [
        9_223_372_036_854_775_808.0,
        -9_223_372_036_854_777_856.0,
        18_446_744_073_709_551_616.0,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
    ] {
        assert!(
            matches!(float_id(float), Err(error) if error.code() == "query_insert_id_numeric_cast_failed" && error.status().as_u16() == 500)
        );
    }
    assert_eq!(processor_id(Value::String(None))?, Value::String(None));
    assert_eq!(
        float_id(-9_223_372_036_854_775_808.0)?,
        Value::BigInt(Some(i64::MIN))
    );
    Ok(())
}

#[test]
fn php85_processor_oracle_applies_laravel_default_warning_exceptions() -> Result<()> {
    use super::processor_id;
    #[derive(serde::Deserialize)]
    struct Oracle {
        php: String,
        cases: Vec<Case>,
    }
    #[derive(serde::Deserialize)]
    struct Case {
        kind: String,
        input: String,
        result: serde_json::Value,
        result_type: String,
        warnings: Vec<u32>,
    }
    let oracle: Oracle = serde_json::from_str(include_str!(
        "../../../tests/fixtures/query_insert_id_php85.json"
    ))
    .map_err(Error::internal)?;
    assert_eq!(oracle.php, "8.5.12");
    assert_eq!(oracle.cases.len(), 18);
    let invalid = || Error::custom("test_processor_oracle_invalid", "主键转换 oracle 格式无效");
    for case in oracle.cases {
        let input = match case.kind.as_str() {
            "string" => Value::from(case.input),
            "float" => Value::Double(Some(match case.input.as_str() {
                "2^63" => 9_223_372_036_854_775_808.0,
                "-1.75" => -1.75,
                "INF" => f64::INFINITY,
                "NAN" => f64::NAN,
                _ => return Err(invalid()),
            })),
            _ => return Err(invalid()),
        };
        let result = processor_id(input);
        if case.warnings.is_empty() {
            let expected = match case.result_type.as_str() {
                "int" => Value::BigInt(Some(case.result.as_i64().ok_or_else(invalid)?)),
                "string" => Value::from(case.result.as_str().ok_or_else(invalid)?),
                _ => return Err(invalid()),
            };
            assert_eq!(result?, expected);
        } else {
            assert_eq!(case.warnings, [2]);
            // fixture 记录 raw builtin 的返回值；Laravel 默认处理器会在 warning 处抛异常。
            assert!(
                matches!(result, Err(error) if error.code() == "query_insert_id_numeric_cast_failed" && error.status().as_u16() == 500)
            );
        }
    }
    Ok(())
}
