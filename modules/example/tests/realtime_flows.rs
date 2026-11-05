//! 真实数据库上的文章授权 SSE 与用户定向发布业务回归。

use super::models::article;
use figment::{Figment, providers::Serialized};
use futures_util::StreamExt as _;
use openrust::Db;
use openrust::db::sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait, Statement};
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode},
};
use openrust::testing::{TestApp, TestDatabase};
use std::time::Duration;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

struct RealtimeMigrator;
#[openrust::db::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for RealtimeMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = super::selected_test_auth::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}

fn request(path: &str, token: Option<&str>, origin: Option<&str>) -> TestResultRequest {
    let mut builder = Request::get(path).header("host", "localhost");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(origin) = origin {
        builder = builder.header("origin", origin);
    }
    Ok(builder.body(Body::empty())?)
}
type TestResultRequest = std::result::Result<Request<Body>, Box<dyn std::error::Error>>;

fn announce_request(path: &str, token: &str) -> TestResultRequest {
    Ok(Request::post(path)
        .header("host", "localhost")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())?)
}

#[tokio::test]
async fn owner_sse_receives_only_authorized_article_announcement() -> TestResult {
    let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping realtime DB test: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = TestDatabase::create::<RealtimeMigrator>(&admin).await?;
    let mut framework = openrust::config::FrameworkConfig::default();
    framework.api.http_status = openrust::config::HttpStatusMode::Semantic;
    let source = Figment::from(Serialized::defaults(framework)).merge(Serialized::defaults(
        serde_json::json!({
            "app":{"env":"test"}, "database":{"url":database.url()},
            "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
            "auth":{"jwt":{"issuer":"http://example.test","audience":"realtime-tests","active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}},
            "realtime":{"allowed_origins":["https://example.test"]}
        }),
    ));
    let mut app = TestApp::<crate::bootstrap::Server>::builder(
        crate::modules::example::test_sources(&(source))?,
    )
    .build()
    .await?;
    let db = app.app()?.state().resolve::<Db>().await?;
    let connection = db.primary_connection()?;
    let users = db.quoted_table_name("users")?;
    for id in [1_i64, 2] {
        let sql = format!(
            "INSERT INTO {users} (id,email,password_hash,email_verified_at,created_at,updated_at) VALUES ({id},'realtime-{id}@example.test','not-used',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)"
        );
        connection
            .execute_raw(Statement::from_string(
                connection.get_database_backend(),
                sql,
            ))
            .await?;
    }
    let article = article::ActiveModel {
        owner_id: Set(1),
        title: Set("Realtime title".into()),
        body: Set("Body".into()),
        ..Default::default()
    }
    .insert(&connection)
    .await?;
    let owner = super::selected_test_auth::seeded_password_access(app.app()?, 1).await?;
    let other = super::selected_test_auth::seeded_password_access(app.app()?, 2).await?;
    let path = format!("/api/example/articles/{}/events", article.id);
    app.request(request(&path, None, Some("https://example.test"))?)
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    app.request(request(&path, Some(&other), Some("https://example.test"))?)
        .await?
        .assert_status(StatusCode::NOT_FOUND)?;
    app.request(request(&path, Some(&owner), Some("https://evil.test"))?)
        .await?
        .assert_status(StatusCode::FORBIDDEN)?;
    let response = app
        .raw_response(request(&path, Some(&owner), Some("https://example.test"))?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let mut stream = response.into_body().into_data_stream();
    let announce = format!("/api/example/articles/{}/announce", article.id);
    let denied = app.request(announce_request(&announce, &other)?).await?;
    denied.assert_status(StatusCode::NOT_FOUND)?;
    let delivered = app.request(announce_request(&announce, &owner)?).await?;
    delivered.assert_status(StatusCode::OK)?;
    let report: serde_json::Value = delivered.json()?;
    assert_eq!(report["data"]["local_sessions_queued"], 1);
    let event = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let chunk = stream
                .next()
                .await
                .ok_or("SSE ended before announcement")??;
            if String::from_utf8_lossy(&chunk).contains("article.announced") {
                return Ok::<_, Box<dyn std::error::Error>>(chunk);
            }
        }
    })
    .await??;
    assert!(String::from_utf8_lossy(&event).contains(&article.id.to_string()));
    drop(stream);
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    database.close().await?;
    Ok(())
}
