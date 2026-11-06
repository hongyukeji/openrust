//! 示例类型表的真实 CRUD、所有者隔离与路由命名空间回归。
#![cfg(any(feature = "db-postgres", feature = "db-mysql"))]

use figment::{Figment, providers::Serialized};
use futures_util::FutureExt as _;
use openrust::auth::AuthFlows;
use openrust::database::sea_orm::ConnectionTrait;
use openrust::database::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use openrust::testing::{TestApp, TestDatabase};
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct Migrator;

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = super::selected_test_auth::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}

fn request(
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<serde_json::Value>,
) -> TestResult<Request<Body>> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "example.test")
        .header("origin", "http://example.test");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    Ok(builder.body(Body::from(
        body.map_or_else(String::new, |value| value.to_string()),
    ))?)
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "同一真实 CRUD 流程的授权、字段类型、数据库状态与回收断言保持在一个生命周期内"
)]
async fn example_record_crud_uses_namespaced_routes_and_portable_field_types() -> TestResult {
    let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping example record DB flow: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = TestDatabase::create::<Migrator>(&admin).await?;
    let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
    let source = Figment::from(Serialized::defaults(openrust::config::FrameworkConfig::default()))
        .merge(Serialized::defaults(serde_json::json!({
            "app":{"env":"test"},
            "database":{"url":database.url()},
            "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
            "auth":{"jwt":{"issuer":"http://example.test","audience":"example-record-tests","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}}
        })));
    let mut app = TestApp::<crate::bootstrap::Server>::builder(
        crate::modules::example::test_sources(&(source))?,
    )
    .build()
    .await?;
    for (method, path) in [
        (Method::POST, "/api/auth/login"),
        (Method::GET, "/api/me"),
        (Method::POST, "/api/jobs/example-heartbeat"),
        (Method::GET, "/pages/about"),
    ] {
        let response = app.request(request(method, path, None, None)?).await?;
        response.assert_status(StatusCode::NOT_FOUND)?;
    }
    let anonymous = app
        .request(request(Method::GET, "/api/example/records", None, None)?)
        .await?;
    anonymous.assert_status(StatusCode::UNAUTHORIZED)?;

    let db = app.app()?.state().resolve::<openrust::Db>().await?;
    let connection = db.primary_connection()?;
    connection
        .execute_unprepared("INSERT INTO example_record_categories (name) VALUES ('field-types')")
        .await?;
    let category_id = connection
        .query_one_raw(openrust::database::sea_orm::Statement::from_string(
            db.backend()?,
            "SELECT id FROM example_record_categories WHERE name = 'field-types'".to_owned(),
        ))
        .await?
        .ok_or("example category insert was not visible")?
        .try_get::<i64>("", "id")?;
    drop(connection);

    let auth = app.app()?.state().resolve::<AuthFlows>().await?;
    let owner_email = "example-record-owner@example.test";
    let password = "example record test password";
    let code = auth
        .register(owner_email, password, "example-record-flow")
        .await?
        .ok_or("verification code missing")?;
    assert!(
        auth.verify_email(owner_email, code.code(), "example-record-flow")
            .await?
    );
    let token = auth
        .login(owner_email, password, "example-record-flow")
        .await?;
    let token = token.access_token().to_owned();
    let payload = serde_json::json!({
        "name":"示例记录", "description":"字段类型完整演示", "priority":7, "view_count":42,
        "enabled":true, "published_at":"2026-10-29T12:00:00Z",
        "metadata":{"source":"modules/example","tags":["typed","crud"]},
        "category_id":category_id
    });
    let created = app
        .request(request(
            Method::POST,
            "/api/example/records",
            Some(&token),
            Some(payload.clone()),
        )?)
        .await?;
    created.assert_status(StatusCode::CREATED)?;
    let data: serde_json::Value = serde_json::from_slice(created.body())?;
    let record = &data["data"];
    let id = record["id"].as_i64().ok_or("example record ID missing")?;
    assert_eq!(record["name"], "示例记录");
    assert_eq!(record["description"], "字段类型完整演示");
    assert_eq!(record["priority"], 7);
    assert_eq!(record["view_count"], 42);
    assert_eq!(record["enabled"], true);
    assert_eq!(record["published_at"], "2026-10-29T12:00:00Z");
    assert_eq!(record["metadata"], payload["metadata"]);
    assert_eq!(record["category_id"], category_id);

    let detail = app
        .request(request(
            Method::GET,
            &format!("/api/example/records/{id}"),
            Some(&token),
            None,
        )?)
        .await?;
    detail.assert_status(StatusCode::OK)?;
    let page = app
        .request(request(
            Method::GET,
            "/api/example/records?page=1&per_page=10",
            Some(&token),
            None,
        )?)
        .await?;
    page.assert_status(StatusCode::OK)?;
    let page_json: serde_json::Value = serde_json::from_slice(page.body())?;
    assert_eq!(page_json["meta"]["total"], 1);

    let other_email = "example-record-other@example.test";
    let other_code = auth
        .register(other_email, password, "example-record-flow")
        .await?
        .ok_or("second verification code missing")?;
    assert!(
        auth.verify_email(other_email, other_code.code(), "example-record-flow")
            .await?
    );
    let other_token = auth
        .login(other_email, password, "example-record-flow")
        .await?
        .access_token()
        .to_owned();
    let foreign_page = app
        .request(request(
            Method::GET,
            "/api/example/records",
            Some(&other_token),
            None,
        )?)
        .await?;
    let foreign_json: serde_json::Value = serde_json::from_slice(foreign_page.body())?;
    assert_eq!(foreign_json["meta"]["total"], 0);
    let foreign_detail = app
        .request(request(
            Method::GET,
            &format!("/api/example/records/{id}"),
            Some(&other_token),
            None,
        )?)
        .await?;
    foreign_detail.assert_status(StatusCode::NOT_FOUND)?;
    attribute_preview_contract(&app, &token, &other_token, id, &payload).await?;
    // 新数值断言失败也先完成原CRUD及宿主/测试库回收，再传播原失败。
    let numeric_outcome =
        std::panic::AssertUnwindSafe(numeric_preview_contract(&app, &token, &payload))
            .catch_unwind()
            .await;

    let update = serde_json::json!({
        "name":"更新记录", "description":"已更新", "priority":-3, "view_count":0,
        "enabled":false, "published_at":null, "metadata":null, "category_id":null
    });
    let updated = app
        .request(request(
            Method::PUT,
            &format!("/api/example/records/{id}"),
            Some(&token),
            Some(update),
        )?)
        .await?;
    updated.assert_status(StatusCode::OK)?;
    let updated_json: serde_json::Value = serde_json::from_slice(updated.body())?;
    assert_eq!(updated_json["data"]["priority"], -3);
    assert!(updated_json["data"]["metadata"].is_null());

    let deleted = app
        .request(request(
            Method::DELETE,
            &format!("/api/example/records/{id}"),
            Some(&token),
            None,
        )?)
        .await?;
    deleted.assert_status(StatusCode::OK)?;
    let missing = app
        .request(request(
            Method::GET,
            &format!("/api/example/records/{id}"),
            Some(&token),
            None,
        )?)
        .await?;
    missing.assert_status(StatusCode::NOT_FOUND)?;

    drop(auth);
    drop(db);
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(10))
        .await?;
    database.close().await?;
    match numeric_outcome {
        Ok(result) => result,
        Err(panic_payload) => std::panic::resume_unwind(panic_payload),
    }
}

// 检出预览误写数据库、脱离owner授权或把相同JSON编码差别当作dirty。
async fn attribute_preview_contract(
    app: &TestApp<crate::bootstrap::Server>,
    owner_token: &str,
    other_token: &str,
    id: i64,
    payload: &serde_json::Value,
) -> TestResult {
    use crate::modules::example::models::example_record;
    use openrust::database::sea_orm::EntityTrait;
    let db = app.app()?.state().resolve::<openrust::Db>().await?;
    let connection = db.primary_connection()?;
    let before = example_record::Entity::find_by_id(id)
        .one(&connection)
        .await?
        .ok_or("preview source row missing")?;
    let path = format!("/api/example/records/{id}/preview");
    preview_rejections(app, &path, owner_token, other_token, payload).await?;
    let mut unchanged = payload.clone();
    // JSON对象键顺序属于严格cast比较；使用数据库实际返回的顺序作为无变化输入。
    unchanged["metadata"] = before.metadata.clone().unwrap_or(serde_json::Value::Null);
    let clean = app
        .request(request(
            Method::POST,
            &path,
            Some(owner_token),
            Some(unchanged.clone()),
        )?)
        .await?;
    clean.assert_status(StatusCode::OK)?;
    let clean: serde_json::Value = serde_json::from_slice(clean.body())?;
    assert_eq!(clean["data"]["dirty"], serde_json::json!({}));
    assert_eq!(clean["data"]["changes"], serde_json::json!({}));
    assert_eq!(clean["data"]["previous"], serde_json::json!({}));
    assert_eq!(clean["data"]["has_changes"], false);
    assert_eq!(clean["data"]["persisted"], false);
    let mut proposal = unchanged;
    proposal["name"] = serde_json::json!("仅预览");
    proposal["priority"] = serde_json::json!(8);
    proposal["metadata"]["tags"] = serde_json::json!(["changed"]);
    proposal["published_at"] = serde_json::Value::Null;
    proposal["category_id"] = serde_json::Value::Null;
    let changed = app
        .request(request(
            Method::POST,
            &path,
            Some(owner_token),
            Some(proposal),
        )?)
        .await?;
    changed.assert_status(StatusCode::OK)?;
    let changed: serde_json::Value = serde_json::from_slice(changed.body())?;
    let changed = &changed["data"];
    assert_eq!(changed["original"]["name"], payload["name"]);
    assert_eq!(changed["proposed"]["name"], "仅预览");
    assert_eq!(changed["dirty"]["priority"], 8);
    assert!(changed["dirty"].get("metadata").is_some());
    assert_eq!(changed["changes"], changed["dirty"]);
    assert!(before.published_at.is_some());
    assert!(before.category_id.is_some());
    for field in ["published_at", "category_id"] {
        assert_eq!(changed["dirty"].get(field), Some(&serde_json::Value::Null));
        assert_eq!(
            changed["changes"].get(field),
            Some(&serde_json::Value::Null)
        );
    }
    assert_eq!(
        changed["previous"]["published_at"],
        serde_json::to_value(before.published_at)?
    );
    assert_eq!(
        changed["previous"]["category_id"],
        serde_json::to_value(before.category_id)?
    );
    assert_eq!(changed["previous"]["priority"], payload["priority"]);
    assert_eq!(changed["persisted"], false);
    assert_eq!(changed["has_changes"], true);
    assert!(changed["original"].get("owner_id").is_none());
    let after = example_record::Entity::find_by_id(id)
        .one(&connection)
        .await?
        .ok_or("preview removed source row")?;
    assert_eq!(after, before);
    Ok(())
}

// priority可成功反序列化为i32；仅依赖业务range规则拒绝，移除Validated会破坏断言。
async fn preview_rejections(
    app: &TestApp<crate::bootstrap::Server>,
    path: &str,
    owner: &str,
    other: &str,
    payload: &serde_json::Value,
) -> TestResult {
    for (token, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(other), StatusCode::NOT_FOUND),
    ] {
        app.request(request(Method::POST, path, token, Some(payload.clone()))?)
            .await?
            .assert_status(expected)?;
    }
    let mut invalid = payload.clone();
    invalid["priority"] = serde_json::json!(100_001);
    let rejected = app
        .request(request(Method::POST, path, Some(owner), Some(invalid))?)
        .await?;
    rejected.assert_status(StatusCode::UNPROCESSABLE_ENTITY)?;
    let error: serde_json::Value = serde_json::from_slice(rejected.body())?;
    assert!(
        error["errors"]["priority"]
            .as_array()
            .is_some_and(|messages| !messages.is_empty())
    );
    Ok(())
}

// 必须通过正常API真正落库，再用同一JWT/owner路径预览；不借内存fixture代替DB原值。
async fn numeric_preview_contract(
    app: &TestApp<crate::bootstrap::Server>,
    token: &str,
    payload: &serde_json::Value,
) -> TestResult {
    let before_count = owned_record_count(app, token).await?;
    let mut fixture = payload.clone();
    fixture["name"] = serde_json::json!("数值类型预览");
    fixture["metadata"] = serde_json::json!({"n":1.0});
    let created = app
        .request(request(
            Method::POST,
            "/api/example/records",
            Some(token),
            Some(fixture.clone()),
        )?)
        .await?;
    created.assert_status(StatusCode::CREATED)?;
    let created: serde_json::Value = serde_json::from_slice(created.body())?;
    let id = created["data"]["id"]
        .as_i64()
        .ok_or("numeric preview fixture ID missing")?;
    numeric_preview_cases(app, token, id, &fixture).await?;
    app.request(request(
        Method::DELETE,
        &format!("/api/example/records/{id}"),
        Some(token),
        None,
    )?)
    .await?
    .assert_status(StatusCode::OK)?;
    assert_eq!(owned_record_count(app, token).await?, before_count);
    Ok(())
}

async fn owned_record_count(
    app: &TestApp<crate::bootstrap::Server>,
    token: &str,
) -> TestResult<u64> {
    let page = app
        .request(request(
            Method::GET,
            "/api/example/records?page=1&per_page=10",
            Some(token),
            None,
        )?)
        .await?;
    page.assert_status(StatusCode::OK)?;
    let page: serde_json::Value = serde_json::from_slice(page.body())?;
    page["meta"]["total"]
        .as_u64()
        .ok_or_else(|| "numeric preview row count missing".into())
}

async fn numeric_preview_cases(
    app: &TestApp<crate::bootstrap::Server>,
    token: &str,
    id: i64,
    fixture: &serde_json::Value,
) -> TestResult {
    use crate::modules::example::models::example_record;
    use openrust::database::sea_orm::EntityTrait;
    let db = app.app()?.state().resolve::<openrust::Db>().await?;
    let connection = db.primary_connection()?;
    let record_path = format!("/api/example/records/{id}");
    for (seed, proposed, seed_float, proposed_float) in [
        (
            serde_json::json!({"n":1.0}),
            serde_json::json!({"n":1.0}),
            true,
            true,
        ),
        (
            serde_json::json!({"n":1}),
            serde_json::json!({"n":1.0}),
            false,
            true,
        ),
        (
            serde_json::json!({"n":1.0}),
            serde_json::json!({"n":1}),
            true,
            false,
        ),
    ] {
        reset_numeric_preview_seed(app, token, id, fixture, &connection).await?;
        let mut body = fixture.clone();
        body["metadata"] = seed;
        app.request(request(
            Method::PUT,
            &record_path,
            Some(token),
            Some(body.clone()),
        )?)
        .await?
        .assert_status(StatusCode::OK)?;
        let before = example_record::Entity::find_by_id(id)
            .one(&connection)
            .await?
            .ok_or("numeric preview DB row missing after update")?;
        let stored = before
            .metadata
            .as_ref()
            .ok_or("numeric preview DB metadata missing")?;
        assert_numeric_type(stored, seed_float, "actual database seed")?;
        body["metadata"] = proposed.clone();
        let preview = app
            .request(request(
                Method::POST,
                &format!("{record_path}/preview"),
                Some(token),
                Some(body),
            )?)
            .await?;
        preview.assert_status(StatusCode::OK)?;
        let preview: serde_json::Value = serde_json::from_slice(preview.body())?;
        assert_numeric_report(
            &preview["data"],
            stored,
            &proposed,
            seed_float,
            proposed_float,
        )?;
        let after = example_record::Entity::find_by_id(id)
            .one(&connection)
            .await?
            .ok_or("numeric preview removed DB row")?;
        assert_numeric_type(
            after
                .metadata
                .as_ref()
                .ok_or("numeric preview DB metadata removed")?,
            seed_float,
            "database after preview",
        )?;
        assert_eq!(
            serde_json::to_string(&after.metadata)?,
            serde_json::to_string(&before.metadata)?
        );
        assert_eq!(after, before);
    }
    Ok(())
}

fn assert_numeric_type(value: &serde_json::Value, floating: bool, stage: &str) -> TestResult {
    let number = value
        .get("n")
        .and_then(serde_json::Value::as_number)
        .ok_or("numeric preview n is not a number")?;
    assert_eq!(
        number.is_f64(),
        floating,
        "{stage}: database/driver JSON category differs"
    );
    assert_eq!(
        number.as_f64().map(f64::to_bits),
        Some(1.0_f64.to_bits()),
        "{stage}"
    );
    Ok(())
}

fn assert_numeric_report(
    report: &serde_json::Value,
    stored: &serde_json::Value,
    proposed: &serde_json::Value,
    stored_float: bool,
    proposed_float: bool,
) -> TestResult {
    let original_text = report["original"]["metadata"]
        .as_str()
        .ok_or("numeric preview original text missing")?;
    let proposed_text = report["proposed"]["metadata"]
        .as_str()
        .ok_or("numeric preview proposed text missing")?;
    assert_eq!(original_text, serde_json::to_string(stored)?);
    assert_eq!(proposed_text, serde_json::to_string(proposed)?);
    assert_numeric_type(
        &serde_json::from_str(original_text)?,
        stored_float,
        "report original",
    )?;
    assert_numeric_type(
        &serde_json::from_str(proposed_text)?,
        proposed_float,
        "report proposed",
    )?;
    let dirty = stored_float != proposed_float;
    assert_eq!(report["has_changes"], dirty);
    assert_eq!(report["persisted"], false);
    assert_eq!(report["changes"], report["dirty"]);
    if dirty {
        assert_eq!(
            report["dirty"].as_object().map(serde_json::Map::len),
            Some(1)
        );
        assert_eq!(report["dirty"]["metadata"].as_str(), Some(proposed_text));
        assert_eq!(report["previous"]["metadata"].as_str(), Some(original_text));
    } else {
        assert_eq!(report["dirty"], serde_json::json!({}));
        assert_eq!(report["previous"], serde_json::json!({}));
    }
    Ok(())
}

// MySQL数值等值JSON更新可保留旧类型；先真实改为不同值，再建立本轮seed。
async fn reset_numeric_preview_seed(
    app: &TestApp<crate::bootstrap::Server>,
    token: &str,
    id: i64,
    fixture: &serde_json::Value,
    connection: &openrust::database::DatabaseConnection,
) -> TestResult {
    use crate::modules::example::models::example_record;
    use openrust::database::sea_orm::EntityTrait;
    let mut body = fixture.clone();
    body["metadata"] = serde_json::json!({"n":2});
    app.request(request(
        Method::PUT,
        &format!("/api/example/records/{id}"),
        Some(token),
        Some(body),
    )?)
    .await?
    .assert_status(StatusCode::OK)?;
    let reset = example_record::Entity::find_by_id(id)
        .one(connection)
        .await?
        .ok_or("numeric preview DB row missing after reset")?;
    let number = reset
        .metadata
        .as_ref()
        .and_then(|value| value.get("n"))
        .and_then(serde_json::Value::as_number)
        .ok_or("numeric preview reset n missing")?;
    assert!(number.is_i64());
    assert_eq!(number.as_i64(), Some(2));
    Ok(())
}
