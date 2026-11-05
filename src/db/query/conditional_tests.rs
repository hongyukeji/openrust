//! SQLite 真实查询覆盖存在性分支、惰性回调与错误传播。

use super::*;
use crate::core::Error;
use crate::db::Db;
use sea_orm::{ConnectionTrait, Database};
use std::cell::Cell;

async fn database() -> Result<Db> {
    let connection = Database::connect("sqlite::memory:")
        .await
        .map_err(Error::internal)?;
    connection
        .execute_unprepared("CREATE TABLE items (id INTEGER)")
        .await
        .map_err(Error::internal)?;
    connection
        .execute_unprepared("INSERT INTO items (id) VALUES (1)")
        .await
        .map_err(Error::internal)?;
    let db = Db::default();
    db.initialize(connection)?;
    Ok(db)
}

#[tokio::test]
async fn exists_or_existing_rows_do_not_even_create_callback_future() -> Result<()> {
    let db = database().await?;
    let called = Cell::new(false);
    let result = db
        .table("items")?
        .exists_or(|| {
            called.set(true);
            async { Err::<String, _>(Error::custom("unexpected_callback", "不应执行回调")) }
        })
        .await?;
    assert_eq!(result, ExistsOrResult::True);
    assert!(!called.get());
    Ok(())
}

#[tokio::test]
async fn exists_or_absent_rows_consume_capture_once_and_keep_arbitrary_value() -> Result<()> {
    let db = database().await?;
    let value = vec![String::from("回调结果"), String::from("保留类型")];
    let expected = value.clone();
    let called = Cell::new(0);
    let result = db
        .table("items")?
        .where_eq("id", 2)?
        .exists_or(|| {
            called.set(called.get() + 1);
            async move { Ok(value) }
        })
        .await?;
    assert_eq!(result, ExistsOrResult::Callback(expected));
    assert_eq!(called.get(), 1);
    Ok(())
}

#[tokio::test]
async fn doesnt_exist_or_absent_rows_do_not_create_callback_future() -> Result<()> {
    let db = database().await?;
    let called = Cell::new(false);
    let result = db
        .table("items")?
        .where_eq("id", 2)?
        .doesnt_exist_or(|| {
            called.set(true);
            async { Err::<u64, _>(Error::custom("unexpected_callback", "不应执行回调")) }
        })
        .await?;
    assert_eq!(result, ExistsOrResult::True);
    assert!(!called.get());
    Ok(())
}

#[tokio::test]
async fn doesnt_exist_or_existing_rows_run_callback_once_and_keep_false() -> Result<()> {
    let db = database().await?;
    let called = Cell::new(0);
    let result = db
        .table("items")?
        .doesnt_exist_or(|| {
            called.set(called.get() + 1);
            async { Ok(false) }
        })
        .await?;
    assert_eq!(result, ExistsOrResult::Callback(false));
    assert_eq!(called.get(), 1);
    Ok(())
}

#[tokio::test]
async fn callback_errors_keep_their_exact_codes_for_both_operations() -> Result<()> {
    let db = database().await?;
    let missing = db
        .table("items")?
        .where_eq("id", 2)?
        .exists_or(|| async {
            Err::<(), _>(Error::custom("exists_callback_failed", "存在性回调失败"))
        })
        .await
        .err()
        .ok_or_else(|| Error::custom("test_expected_error", "预期错误未返回"))?;
    assert_eq!(missing.code(), "exists_callback_failed");
    let existing = db
        .table("items")?
        .doesnt_exist_or(|| async {
            Err::<(), _>(Error::custom("absent_callback_failed", "不存在性回调失败"))
        })
        .await
        .err()
        .ok_or_else(|| Error::custom("test_expected_error", "预期错误未返回"))?;
    assert_eq!(existing.code(), "absent_callback_failed");
    Ok(())
}

#[tokio::test]
async fn missing_table_errors_skip_callbacks_for_both_operations() -> Result<()> {
    let db = database().await?;
    let called = Cell::new(false);
    let missing = db.table("missing_table")?;
    let expected = missing
        .exists()
        .await
        .err()
        .ok_or_else(|| Error::custom("test_expected_error", "预期错误未返回"))?;
    let error = missing
        .exists_or(|| {
            called.set(true);
            async { Ok(()) }
        })
        .await
        .err()
        .ok_or_else(|| Error::custom("test_expected_error", "预期错误未返回"))?;
    assert_eq!(error.code(), expected.code());
    assert!(!called.get());
    let error = missing
        .doesnt_exist_or(|| {
            called.set(true);
            async { Ok(()) }
        })
        .await
        .err()
        .ok_or_else(|| Error::custom("test_expected_error", "预期错误未返回"))?;
    assert_eq!(error.code(), expected.code());
    assert!(!called.get());
    Ok(())
}

#[tokio::test]
async fn same_query_remains_reusable_after_short_circuit_and_callback() -> Result<()> {
    let db = database().await?;
    let query = db.table("items")?.where_eq("id", 1)?;
    assert_eq!(
        query.exists_or(|| async { Ok(10) }).await?,
        ExistsOrResult::True
    );
    assert_eq!(
        query.doesnt_exist_or(|| async { Ok(20) }).await?,
        ExistsOrResult::Callback(20)
    );
    assert_eq!(query.count().await?, 1);
    db.table("items")?.where_eq("id", 1)?.delete().await?;
    assert_eq!(
        query.exists_or(|| async { Ok(30) }).await?,
        ExistsOrResult::Callback(30)
    );
    assert_eq!(
        query.doesnt_exist_or(|| async { Ok(40) }).await?,
        ExistsOrResult::True
    );
    assert_eq!(query.count().await?, 0);
    Ok(())
}

#[tokio::test]
async fn immutable_query_can_be_borrowed_by_its_own_read_only_fallback() -> Result<()> {
    let db = database().await?;
    let query = db.table("items")?.where_eq("id", 2)?;
    assert_eq!(
        query.exists_or(|| query.count()).await?,
        ExistsOrResult::Callback(0)
    );
    let existing = db.table("items")?.where_eq("id", 1)?;
    assert_eq!(
        existing.doesnt_exist_or(|| existing.count()).await?,
        ExistsOrResult::Callback(1)
    );
    Ok(())
}
