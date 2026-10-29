//! 示例类型表的真实 CRUD、所有者隔离与路由命名空间回归。
#![cfg(any(feature = "db-postgres", feature = "db-mysql"))]

use figment::{Figment, providers::Serialized};
use openrust::auth::AuthFlows;
use openrust::db::sea_orm::ConnectionTrait;
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use openrust::testing::{TestApp, TestDatabase};
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct Migrator;

#[openrust::db::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = openrust::db::FrameworkMigrator::migrations();
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
            "cache":{"default_store":"default","stores":{"default":{"driver":"memory"}}},
            "auth":{"jwt":{"issuer":"http://example.test","audience":"example-record-tests","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}}
        })));
    let mut app = TestApp::<crate::bootstrap::Server>::builder(source)
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
        .query_one_raw(openrust::db::sea_orm::Statement::from_string(
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
    Ok(())
}
