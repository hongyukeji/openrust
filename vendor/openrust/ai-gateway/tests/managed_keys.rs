use std::{collections::BTreeMap, error::Error, path::Path};

use openrust::{
    config::{ConfigError, Environment},
    http::{Body, StatusCode},
    testing::{TestApp, TestResponse},
};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

fn config(base: &Path, env: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    let mut gateway = openrust_ai_gateway::default_config(env)?;
    gateway["enabled"] = true.into();
    gateway["managed_keys"] = json!({"enabled":true,"connection":"sqlite"});
    gateway["models"] = json!({"managed-model":{"provider":"deepseek","model":"fixture"}});
    gateway["groups"] = json!({"managed-team":{"models":["managed-model"]}});
    Ok(BTreeMap::from([
        ("ai_gateway".into(), gateway),
        (
            "database".into(),
            json!({
                "default":"sqlite",
                "connections":{"sqlite":{"driver":"sqlite","database":base.join("gateway.sqlite")}},
                "migrations":{"table":"migrations"}
            }),
        ),
    ]))
}

async fn catalogue(
    app: &mut TestApp,
    token: &str,
) -> Result<TestResponse, Box<dyn Error + Send + Sync>> {
    app.call(
        http::Request::get("/v1/models")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())?,
    )
    .await
}

#[tokio::test]
async fn managed_gateway_keys_take_effect_without_restarting_the_application() -> TestResult {
    let mut app = TestApp::create(|builder| {
        let mut providers = openrust_ai::openrust_providers();
        providers.extend(openrust_ai_gateway::openrust_providers());
        builder.with_providers(providers).with_config(config)
    })?;
    std::fs::File::create(app.root().join("gateway.sqlite"))?;
    let migration = app.artisan(["migrate", "--force"]).await;
    assert_eq!(migration.status, 0, "{migration:?}");

    let issued = app
        .artisan(["ai:gateway:key:create", "fixture", "managed-team"])
        .await;
    assert_eq!(issued.status, 0);
    let issued: Value = serde_json::from_str(&issued.stdout)?;
    let id = issued["id"].as_str().unwrap();
    let old = issued["token"].as_str().unwrap();
    let response = catalogue(&mut app, old).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.json::<Value>()?["data"][0]["id"], "managed-model");

    let rotation = app.artisan(["ai:gateway:key:rotate", id]).await;
    assert_eq!(rotation.status, 0);
    let rotation: Value = serde_json::from_str(&rotation.stdout)?;
    let new = rotation["token"].as_str().unwrap();
    assert_eq!(
        catalogue(&mut app, old).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(catalogue(&mut app, new).await?.status(), StatusCode::OK);

    let revoke = app.artisan(["ai:gateway:key:revoke", id]).await;
    assert_eq!(revoke.status, 0);
    assert_eq!(
        catalogue(&mut app, new).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_ne!(app.artisan(["ai:gateway:key:rotate", id]).await.status, 0);

    let list = app.artisan(["ai:gateway:key:list"]).await;
    assert_eq!(list.status, 0);
    assert!(!list.stdout.contains(old) && !list.stdout.contains(new));
    let list: Value = serde_json::from_str(&list.stdout)?;
    assert_eq!(list["keys"][0]["id"], id);
    assert!(list["keys"][0]["revoked_at"].is_i64());
    Ok(())
}
