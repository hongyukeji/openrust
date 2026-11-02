//! 标准 Provider 的 SQLite 具名连接沿唯一业务模块实际访问；不启用默认库业务。

use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode},
};

#[tokio::test]
async fn named_sqlite_reporting_uses_standard_provider_and_module_http() -> openrust::Result<()> {
    let source = figment::Figment::from(figment::providers::Serialized::defaults(
        serde_json::json!({
            "app":{"env":"testing"},
            "modules":{"activator":"registered"},
            "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
            "database":{"url":null,"instances":{"reporting":{"url":"sqlite::memory:","max_connections":4}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
        }),
    ));
    let mut app = openrust::testing::TestApp::<crate::bootstrap::Server>::builder(
        crate::modules::example::test_sources(&source)?,
    )
    .build()
    .await?;
    let outcome: openrust::Result<()> = async {
        app.request(Request::get("/api/example/reporting-connection").header("host","localhost").body(Body::empty()).map_err(openrust::Error::internal)?)
            .await?.assert_status(StatusCode::OK)?.assert_json(&serde_json::json!({"success":true,"code":"ok","message":"ok","data":{"reachable":true,"backend":"sqlite"}}))?;
        let default = app.app()?.state().resolve::<openrust::Db>().await?;
        assert!(default.backend().is_err());
        Ok(())
    }.await;
    let cleanup = app
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    outcome?;
    cleanup
}
