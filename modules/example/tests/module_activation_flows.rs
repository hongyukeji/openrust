//! 同一真实宿主上的持久模块管理：状态变化与当前已加载路由分别观察。

use figment::{Figment, providers::Serialized};
use openrust::config::{ConfigLoader, ConfigManifest, ConfigSources, FrameworkConfig};
use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode},
};
use openrust::testing::TestApp;
use openrust::{Error, Result};
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

type ServerApp = TestApp<crate::bootstrap::Server>;
const TIMEOUT: Duration = Duration::from_secs(5);
const MODULE_STATE: &str = "/api/example/module-state";

fn deployment() -> Result<tempfile::TempDir> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    crate::modules::example::write_discovery_fixture(root.path())?;
    for directory in [
        "modules/example/config",
        "modules/example/lang",
        "modules/example/resources/views",
        "resources/lang/vendor/example",
        "state",
    ] {
        std::fs::create_dir_all(root.path().join(directory)).map_err(Error::internal)?;
    }
    std::fs::write(
        root.path().join("state/example.json"),
        r#"{"example":true}"#,
    )
    .map_err(Error::internal)?;
    Ok(root)
}

fn sources(root: &Path) -> Result<ConfigSources> {
    let source = Figment::from(Serialized::defaults(FrameworkConfig::default())).merge(
        Serialized::defaults(json!({
            "application_root": root,
            "app": {"name": "Module management host", "env": "testing"},
            "modules": {"activator": "file", "activators": {
                "file": {"statuses-file": "state/example.json"}
            }},
            "database": {"url": null, "instances": {}, "read_replicas": []},
            "cache": {"default": "memory", "stores": {"memory": {"driver": "memory"}}},
            "storage": {"default_disk": "local", "disks": {"local": {
                "driver": "fs", "root": root.join("storage/app")
            }}},
            "security": {"encryption": {"active_key_id": "test", "keys": {
                "test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="
            }}}
        })),
    );
    let entries = source
        .extract::<serde_json::Map<String, Value>>()
        .map_err(openrust::config::ConfigError::from)?;
    // 独立命名空间与显式配置来源不修改进程环境/cwd；仍装配真实 Server Provider 和模块。
    // 空应用配置清单只避免 fixture 意外求值真实部署环境，模块代码沿同一次环境快照执行。
    ConfigLoader::application_code_sources_with_overrides(
        root,
        "EXAMPLE_MODULE_TEST_",
        None,
        &ConfigManifest::new(),
        entries,
    )
    .map_err(Into::into)
}

async fn start(root: &Path) -> Result<ServerApp> {
    ServerApp::builder(sources(root)?).build().await
}

async fn status(app: &ServerApp, path: &str, expected: StatusCode) -> Result<()> {
    app.request_with_limits(
        Request::get(path)
            .header("host", "localhost")
            .body(Body::empty())
            .map_err(Error::internal)?,
        4096,
        TIMEOUT,
    )
    .await?
    .assert_status(expected)?;
    Ok(())
}

async fn state(app: &ServerApp) -> Result<Value> {
    let response = app
        .request_with_limits(
            Request::get(MODULE_STATE)
                .header("host", "localhost")
                .body(Body::empty())
                .map_err(Error::internal)?,
            4096,
            TIMEOUT,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let body: Value = response.json()?;
    Ok(body["data"].clone())
}

fn expected_state(enabled: bool, events: &[&str]) -> Value {
    json!({"name": "example", "enabled": enabled, "loaded": true, "events": events})
}

fn check(condition: bool, message: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::custom(
            "example_module_activation_assertion",
            message,
        ))
    }
}

fn instance(apps: &[ServerApp], index: usize) -> Result<&ServerApp> {
    apps.get(index).ok_or_else(|| {
        Error::custom(
            "example_module_activation_instance",
            "缺少已经启动的测试实例",
        )
    })
}

async fn check_state(app: &ServerApp, enabled: bool, events: &[&str]) -> Result<()> {
    check(
        state(app).await? == expected_state(enabled, events),
        "模块的名称、持久状态、加载状态或事件顺序不匹配",
    )
}

async fn verify_changes(root: &Path, apps: &mut Vec<ServerApp>) -> Result<()> {
    apps.push(start(root).await?);
    check_state(instance(apps, 0)?, true, &[]).await?;
    instance(apps, 0)?
        .app()?
        .run_command("module:disable", vec!["example".to_owned()])
        .await?;
    let disabling = ["modules.example.disabling", "modules.example.disabled"];
    check_state(instance(apps, 0)?, false, &disabling).await?;
    status(instance(apps, 0)?, "/api/example", StatusCode::OK).await?;

    apps.push(start(root).await?);
    status(instance(apps, 1)?, MODULE_STATE, StatusCode::NOT_FOUND).await?;
    status(instance(apps, 1)?, "/api/example", StatusCode::NOT_FOUND).await?;
    status(instance(apps, 1)?, "/api", StatusCode::OK).await?;

    instance(apps, 0)?
        .app()?
        .run_command("module:enable", vec!["example".to_owned()])
        .await?;
    let changes = [
        "modules.example.disabling",
        "modules.example.disabled",
        "modules.example.enabling",
        "modules.example.enabled",
    ];
    check_state(instance(apps, 0)?, true, &changes).await?;
    // 已启动的第二实例不会因文件重新启用而热挂 Provider 或路由。
    status(instance(apps, 1)?, MODULE_STATE, StatusCode::NOT_FOUND).await?;
    status(instance(apps, 1)?, "/api", StatusCode::OK).await?;

    apps.push(start(root).await?);
    check_state(instance(apps, 2)?, true, &[]).await?;
    status(instance(apps, 2)?, "/api/example", StatusCode::OK).await?;
    check(
        !root.join("modules_statuses.json").exists(),
        "测试不应写入默认状态路径",
    )?;
    let persisted: Value = serde_json::from_slice(
        &std::fs::read(root.join("state/example.json")).map_err(Error::internal)?,
    )
    .map_err(Error::internal)?;
    check(
        persisted.get("example") == Some(&Value::Bool(true)),
        "最终状态未持久启用",
    )
}

async fn shutdown_all(mut apps: Vec<ServerApp>) -> Result<()> {
    let mut outcome = Ok(());
    // pop 保证逆序；即使一个实例关闭失败，仍尝试关闭所有更早启动的实例。
    while let Some(mut app) = apps.pop() {
        let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
        if outcome.is_ok() {
            outcome = shutdown;
        }
    }
    outcome
}

#[tokio::test]
async fn persistent_commands_change_future_boots_without_hot_mounting_current_routes() -> Result<()>
{
    let root = deployment()?;
    let mut apps = Vec::new();
    // 检查以 Result 返回；后续 build 或任何断言失败都必须先关闭已启动实例。
    let outcome = verify_changes(root.path(), &mut apps).await;
    let shutdown = shutdown_all(apps).await;
    if let (Err(_), Err(error)) = (&outcome, &shutdown) {
        tracing::error!(code = error.code(), "模块启停测试失败后资源关闭也失败");
    }
    outcome.and(shutdown)
}
