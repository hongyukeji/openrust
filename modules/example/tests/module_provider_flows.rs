//! 父 Provider 通过同一内核注册子项，测试移除仍作用于子项的独立类型身份。

use figment::{Figment, providers::Serialized};
use openrust::config::FrameworkConfig;
use openrust::core::{ModuleRepository, ProviderId};
use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode},
};
use openrust::testing::TestApp;
use openrust::{Error, Result};
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn removing_business_child_does_not_remove_module_parent_or_restore_child() -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    for directory in ["config", "lang", "resources/views"] {
        std::fs::create_dir_all(root.path().join("modules/example").join(directory))
            .map_err(Error::internal)?;
    }
    std::fs::write(
        root.path().join("modules_statuses.json"),
        r#"{"example":true}"#,
    )
    .map_err(Error::internal)?;
    let source = Figment::from(Serialized::defaults(FrameworkConfig::default())).merge(
        Serialized::defaults(json!({
            "application_root": root.path(),
            "app": {"env": "testing"},
            "database": {"url": null, "instances": {}, "read_replicas": []},
            "cache": {"default": "memory", "stores": {"memory": {"driver": "memory"}}},
            "storage": {"default_disk": "local", "disks": {"local": {
                "driver": "fs", "root": root.path().join("storage/app")
            }}},
            "security": {"encryption": {"active_key_id": "test", "keys": {
                "test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="
            }}}
        })),
    );
    let mut builder = TestApp::<crate::bootstrap::Server>::builder(super::test_sources(&source)?);
    builder.remove_provider(ProviderId::of::<super::providers::ExampleServiceProvider>());
    let mut app = builder.build().await?;
    let outcome: Result<()> = async {
        let module = app
            .app()?
            .state()
            .resolve::<ModuleRepository>()
            .await?
            .find_or_fail("example")?;
        if !module.is_loaded() || !module.is_enabled()? {
            return Err(Error::custom(
                "example_parent_not_loaded",
                "移除业务子 Provider 不应移除活跃模块",
            ));
        }
        if app
            .app()?
            .state()
            .container()
            .contains::<super::services::ExampleMessage>()
        {
            return Err(Error::custom(
                "example_child_restored",
                "父 Provider 不应重新装回测试已移除的子 Provider",
            ));
        }
        // 状态日志子项仍通过父 register 独立登记，业务子项的移除不应吞掉它。
        app.app()?
            .state()
            .resolve::<super::providers::module_state::ModuleStateLog>()
            .await?;
        for (path, expected) in [
            ("/api", StatusCode::OK),
            ("/api/example", StatusCode::NOT_FOUND),
        ] {
            app.request_with_limits(
                Request::get(path)
                    .header("host", "localhost")
                    .body(Body::empty())
                    .map_err(Error::internal)?,
                4096,
                Duration::from_secs(5),
            )
            .await?
            .assert_status(expected)?;
        }
        Ok(())
    }
    .await;
    let shutdown = app
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    outcome.and(shutdown)
}
