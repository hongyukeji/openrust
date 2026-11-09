//! 示例记录分页通过标准 SQLite 连接应用 JSON 标量筛选。

use super::{Fixture, PASSWORD, bounded, ensure, mismatch};
use openrust::database::sea_orm::{ConnectionTrait, DbBackend, Statement, Value as DbValue};
use openrust::http::axum::http::StatusCode;
use openrust::{Db, Result};
use serde_json::{Value, json};

const RECORDS: &str = "/api/example/records";

async fn verify_engine(db: &Db) -> Result<()> {
    let expected = std::env::var("OPENRUST_EXPECT_SQLITE_VERSION_PREFIX")
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            openrust::Error::custom("sqlite_version_required", "须指定实际 SQLite 版本")
        })?;
    let row = db
        .primary_connection()?
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT sqlite_version() AS version",
        ))
        .await
        .map_err(openrust::Error::internal)?
        .ok_or_else(mismatch)?;
    let actual: String = row
        .try_get("", "version")
        .map_err(openrust::Error::internal)?;
    assert!(
        actual.starts_with(&expected),
        "SQLite {actual}，预期前缀 {expected}"
    );
    Ok(())
}

async fn account(fixture: &Fixture, email: &str) -> Result<(i64, String)> {
    let flows = fixture.flows().await?;
    let source = "sqlite-record-json-contains";
    let code = bounded(flows.register(email, PASSWORD, source))
        .await?
        .ok_or_else(mismatch)?;
    ensure(bounded(flows.verify_email(email, code.code(), source)).await?)?;
    let login = bounded(flows.login(email, PASSWORD, source)).await?;
    Ok((login.user_id(), login.access_token().to_owned()))
}

async fn insert_record(db: &Db, id: i64, owner: i64, metadata: Option<Value>) -> Result<()> {
    db.table("example_records")?
        .insert(vec![
            ("id", id.into()),
            ("owner_id", owner.into()),
            ("name", "SQLite 元数据筛选".into()),
            ("description", "分页与账户隔离夹具".into()),
            ("priority", 0_i32.into()),
            ("view_count", 0_i64.into()),
            ("enabled", true.into()),
            ("metadata", DbValue::Json(metadata.map(Box::new))),
        ])
        .await?;
    Ok(())
}

fn metadata_uri(needle: &str, page: u64) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("page", &page.to_string())
        .append_pair("per_page", "1")
        .append_pair("metadata_contains", needle)
        .finish();
    format!("{RECORDS}?{query}")
}

async fn exercise_json_contains(fixture: &Fixture) -> Result<()> {
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    verify_engine(&db).await?;
    let (owner, token) = account(fixture, "sqlite-record-owner@example.test").await?;
    let (other, other_token) = account(fixture, "sqlite-record-other@example.test").await?;
    ensure(owner != other)?;
    // HTTP 写入的可选字段会把 JSON null 折叠为 None，直接经 Db 区分两类存储值。
    for (id, account, metadata) in [
        (101, owner, Some(json!([true, "共同", "-0"]))),
        (102, owner, Some(json!([1, "共同"]))),
        (103, owner, Some(json!(["1", "1.5"]))),
        (104, owner, Some(json!([1.5]))),
        (105, owner, Some(Value::Null)),
        (106, owner, None),
        (107, owner, Some(json!([]))),
        (201, other, Some(json!([true, "共同", null]))),
    ] {
        insert_record(&db, id, account, metadata).await?;
    }

    for (token, needle, page, total, id) in [
        (&token, r#""共同""#, 1, 2, Some(101)),
        (&token, r#""共同""#, 2, 2, Some(102)),
        (&token, r#""共同""#, 3, 2, None),
        (&other_token, r#""共同""#, 1, 1, Some(201)),
        (&token, "true", 1, 2, Some(101)),
        (&token, "1", 2, 2, Some(102)),
        (&token, r#""1""#, 1, 1, Some(103)),
        (&token, "1.5", 1, 1, Some(103)),
        (&token, "-0.0", 1, 1, Some(101)),
        (&token, "null", 1, 1, Some(105)),
        (&other_token, "null", 1, 1, Some(201)),
    ] {
        let response = fixture
            .request(
                "GET",
                &metadata_uri(needle, page),
                &Value::Null,
                Some(token),
                None,
                None,
            )
            .await?;
        response.assert_status(StatusCode::OK)?;
        let body = response.json::<Value>()?;
        ensure(body["meta"]["total"].as_u64() == Some(total))?;
        let records = body["data"].as_array().ok_or_else(mismatch)?;
        match id {
            Some(id) => {
                ensure(matches!(records.as_slice(), [record] if record["id"].as_i64() == Some(id)))?
            }
            None => ensure(records.is_empty())?,
        }
    }

    for needle in ["[]", r#"["共同"]"#, r#"{"language":"共同"}"#] {
        let response = fixture
            .request(
                "GET",
                &metadata_uri(needle, 1),
                &Value::Null,
                Some(&token),
                None,
                None,
            )
            .await?;
        response.assert_status(StatusCode::UNPROCESSABLE_ENTITY)?;
        ensure(
            response.json::<Value>()?["errors"]["metadata_contains"]
                == json!(["SQLite 元数据包含条件必须是 JSON 标量"]),
        )?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "发布前 SQLite JSON 包含实库矩阵"]
async fn sqlite_record_json_contains_preserves_owner_pagination_and_scalar_types() -> Result<()> {
    let fixture = Fixture::create(false).await?;
    let outcome = exercise_json_contains(&fixture).await;
    let cleanup = fixture.close().await;
    outcome?;
    cleanup
}
