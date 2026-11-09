//! 验证启用 Passport 的生成应用使用同一 Provider 和迁移入口。
#![cfg(feature = "passport")]

#[path = "../app/mod.rs"]
mod app;
#[path = "../bootstrap/app.rs"]
mod bootstrap;
#[path = "../config/mod.rs"]
mod config;
{{grpc_module}}#[path = "../modules/mod.rs"]
mod modules;
#[path = "../routes/mod.rs"]
mod routes;
#[path = "../database/seeders.rs"]
mod seed;

use openrust::{
    config::ConfigLoader,
    database::Db,
    foundation::{ApplicationDefinition, BootContext},
    http::{
        Inject, Request,
        axum::{
            body::Body,
            http::{Request as HttpRequest, StatusCode},
        },
    },
    support::{BoxFuture, ServiceProvider},
    testing::{TestApp, TestDatabase},
};
use openrust_passport::{OAuthAccessSigner, OAuthError, PassportConfig, ResourceServer};
use serde_json::json;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

type TestResult = Result<(), Box<dyn std::error::Error>>;
const CLIENT: &str = "0d77bb3a-b474-4c1f-9608-7734fa199834";
const TOKEN_ID: &str = "generated-passport-resource";

struct ProtectedRoute;

impl ServiceProvider for ProtectedRoute {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.routes(|routes| routes.get("/test-passport-resource", protected))?;
            Ok(())
        })
    }
}

async fn protected(
    Inject(server): Inject<ResourceServer>,
    request: Request,
) -> Result<StatusCode, OAuthError> {
    server.validate_authenticated_request(&request).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[tokio::test]
async fn generated_passport_uses_the_application_provider_and_migrated_revocation_store()
-> TestResult {
    let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("跳过生成 Passport 资源测试，需真实 MySQL URL");
        return Ok(());
    };
    if !admin.starts_with("mysql://") {
        return Err("生成 Passport 资源测试需要 MySQL".into());
    }
    let private = std::fs::read(
        std::env::var_os("OPENRUST_TEST_PASSPORT_PRIVATE_KEY_FILE")
            .ok_or("缺少测试 RSA 私钥路径")?,
    )?;
    let public = std::fs::read_to_string(
        std::env::var_os("OPENRUST_TEST_PASSPORT_PUBLIC_KEY_FILE")
            .ok_or("缺少测试 RSA 公钥路径")?,
    )?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let token = OAuthAccessSigner::from_pem(&private)?.sign_at(
        CLIENT,
        TOKEN_ID,
        None,
        &["read".into()],
        now,
        now + 600,
    )?;
    let database = TestDatabase::create::<openrust_migration::Migrator>(&admin).await?;
    let outcome = check_resource(&database, public, &token).await;
    let cleanup = database.close().await;
    outcome?;
    cleanup?;
    Ok(())
}

async fn check_resource(database: &TestDatabase, public_key: String, token: &str) -> TestResult {
    type Generated = bootstrap::{{type_name}};
    let passport = serde_json::to_value(PassportConfig {
        public_key: Some(public_key),
        ..PassportConfig::default()
    })?;
    let source = ConfigLoader::application_code_sources_with_overrides(
        env!("CARGO_MANIFEST_DIR"),
        "APP_",
        None,
        &Generated::config(),
        [
            (
                "app",
                json!({"env":"test"}),
            ),
            ("database", json!({"url":database.url()})),
            (
                "cache",
                json!({"default":"default","stores":{"default":{"driver":"memory"}}}),
            ),
            ("passport", passport),
        ],
    )?;
    let mut builder = TestApp::<Generated>::builder(source);
    builder.configure_providers(|providers| {
        providers.provider(ProtectedRoute);
    });
    let mut app = builder
        .build()
        .await
        .map_err(|error| format!("生成 Passport 应用启动失败：{error}"))?;
    let db = Db::default();
    db.initialize(database.connection()?)?;
    let tokens = db.table("oauth_access_tokens")?;
    let request = || {
        HttpRequest::builder()
            .uri("/test-passport-resource")
            .header("host", "example.test")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
    };
    app.request(request()?)
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    tokens
        .clone()
        .insert(vec![
            ("id", TOKEN_ID.into()),
            ("client_id", CLIENT.into()),
            ("revoked", false.into()),
        ])
        .await?;
    app.request(request()?)
        .await?
        .assert_status(StatusCode::NO_CONTENT)?;
    assert_eq!(
        tokens
            .where_eq("id", TOKEN_ID)?
            .update(vec![("revoked", true.into())])
            .await?,
        1
    );
    app.request(request()?)
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(10))
        .await?;
    Ok(())
}
