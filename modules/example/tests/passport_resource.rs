//! 真实 MySQL 验证根应用的公钥资源入口和即时撤销。

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use figment::{Figment, providers::Serialized};
use openrust::{
    database::Db,
    http::axum::{
        body::Body,
        http::{Request, StatusCode},
    },
    testing::{TestApp, TestDatabase},
};
use openrust_passport::{OAuthAccessSigner, PassportConfig};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;
const CLIENT: &str = "77ba6e4c-e927-4f12-bdd7-41eec66ba9b2";
const TOKEN_ID: &str = "example-passport-resource";

#[tokio::test]
async fn example_passport_public_key_route_observes_mysql_revocation() -> TestResult {
    let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("跳过 Passport 示例资源测试，需真实 MySQL URL");
        return Ok(());
    };
    if !admin.starts_with("mysql://") {
        return Err("Passport 示例资源测试需要 MySQL".into());
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
        Some("13"),
        &["read-example".into()],
        now,
        now + 600,
    )?;
    let database = TestDatabase::create::<crate::database::Migrator>(&admin).await?;
    let outcome = check_resource(&database, public, &token).await;
    let cleanup = database.close().await;
    outcome?;
    cleanup?;
    Ok(())
}

async fn check_resource(database: &TestDatabase, public_key: String, token: &str) -> TestResult {
    let passport = serde_json::to_value(PassportConfig {
        public_key: Some(public_key),
        ..PassportConfig::default()
    })?;
    let source = Figment::from(Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(Serialized::defaults(json!({
        "app":{"env":"test"},
        "database":{"url":database.url()},
        "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
        "passport":passport
    })));
    let mut app = TestApp::<crate::bootstrap::Server>::builder(super::test_sources(&source)?)
        .build()
        .await
        .map_err(|error| format!("Passport 示例应用启动失败：{error}"))?;
    let db = Db::default();
    db.initialize(database.connection()?)?;
    let tokens = db.table("oauth_access_tokens")?;
    let request = || {
        Request::builder()
            .uri("/api/example/passport/resource")
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
    let response = app.request(request()?).await?;
    response.assert_status(StatusCode::OK)?;
    assert_eq!(
        serde_json::from_slice::<Value>(response.body())?,
        json!({"success": true, "code": "ok", "message": "ok", "data": {"client_id": CLIENT}})
    );
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
