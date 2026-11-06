//! 仓储资源根取清单所在目录，只有编译文件包含跟随 metadata 覆盖。

use figment::{Figment, providers::Serialized};
use openrust::config::{ConfigSources, FrameworkConfig};
use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode},
};
use openrust::i18n::{I18n, Locale, MessageArgs};
use openrust::testing::TestApp;
use openrust::{Error, Result};
use openrust_modules::ModuleRepository;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;

type ServerApp = TestApp<crate::bootstrap::Server>;

fn deployment() -> Result<(tempfile::TempDir, PathBuf)> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    super::write_discovery_fixture(root.path())?;
    let directory = root.path().join("actual/example");
    for resource in ["config", "lang", "resources/views"] {
        std::fs::create_dir_all(root.path().join("modules/example").join(resource))
            .map_err(Error::internal)?;
    }
    std::fs::create_dir_all(directory.join("lang")).map_err(Error::internal)?;
    // 先删除默认包含文件，只有 metadata 覆盖指向的真实文件可让初始化成功。
    std::fs::remove_file(root.path().join("modules/example/module_init.rs"))
        .map_err(Error::internal)?;
    std::fs::write(
        directory.join("module_init.rs"),
        "// canonical include 路径占位，不执行此源码\n",
    )
    .map_err(Error::internal)?;
    std::fs::write(
        root.path().join("modules/example/lang/en.json"),
        r#"{"heartbeat":"MANIFEST_DIRECTORY","published_probe":"SOURCE_VALUE"}"#,
    )
    .map_err(Error::internal)?;
    std::fs::write(
        directory.join("lang/en.json"),
        r#"{"heartbeat":"INCLUDE_DIRECTORY_MUST_NOT_LOAD"}"#,
    )
    .map_err(Error::internal)?;
    std::fs::write(
        root.path().join("modules/example/module.json"),
        json!({
            "name":"example", "module_directory":directory, "priority":0,
            "providers":["ExampleModuleServiceProvider"], "files":["module_init.rs"],
        })
        .to_string(),
    )
    .map_err(Error::internal)?;
    std::fs::write(
        root.path().join("modules_statuses.json"),
        r#"{"example":true}"#,
    )
    .map_err(Error::internal)?;
    let catalog = root.path().join("modules/example");
    Ok((root, catalog))
}

fn sources(root: &Path) -> Result<ConfigSources> {
    let source = Figment::from(Serialized::defaults(FrameworkConfig::default()))
        .merge(Serialized::defaults(json!({
            "application_root":root, "app":{"env":"testing"},
            "database":{"url":null,"instances":{},"read_replicas":[]},
            "cache":{"default":"memory","stores":{"memory":{"driver":"memory"}}},
            "storage":{"default_disk":"local","disks":{"local":{"driver":"fs","root":root.join("storage/app")}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}},
        })));
    super::test_sources(&source)
}

fn check(condition: bool, code: &'static str, message: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::custom(code, message))
    }
}

async fn check_host(app: &ServerApp, directory: &Path) -> Result<()> {
    let response = app
        .request_with_limits(
            Request::get("/api/example/module-initialization")
                .header("host", "localhost")
                .body(Body::empty())
                .map_err(Error::internal)?,
            4096,
            Duration::from_secs(5),
        )
        .await?;
    response
        .assert_status(StatusCode::OK)?
        .assert_json(&json!({
            "success":true,"code":"ok","message":"ok",
            "data":{"initialized":true,"business_registered":true}
        }))?;
    let repository = app.app()?.state().resolve::<ModuleRepository>().await?;
    let module = repository.find_or_fail("example")?;
    check(
        module.path() == directory,
        "example_discovery_repository_directory",
        "仓储路径必须取清单所在目录，不能混入文件包含目录覆盖",
    )?;
    check(
        module.is_loaded(),
        "example_discovery_module_not_loaded",
        "发现模块没有在当前应用加载",
    )?;
    check(
        module.is_enabled()?,
        "example_discovery_module_not_enabled",
        "发现模块没有保留真实启用状态",
    )
}

async fn translation(app: &ServerApp, key: &str) -> Result<String> {
    let i18n = app.app()?.state().resolve::<I18n>().await?;
    let locale = Locale::new("en")?;
    Ok(i18n.translate(&locale, key, &MessageArgs::new()))
}

#[tokio::test]
async fn repository_resources_and_metadata_include_directory_are_independently_observable()
-> Result<()> {
    let (root, directory) = deployment()?;
    let mut source_app = ServerApp::builder(sources(root.path())?).build().await?;
    let source_outcome: Result<()> = async {
        check_host(&source_app, &directory).await?;
        check(
            translation(&source_app, "example::heartbeat").await? == "MANIFEST_DIRECTORY",
            "example_discovery_source_heartbeat",
            "业务 Provider 必须从模块仓储目录加载资源",
        )?;
        check(
            translation(&source_app, "example::published_probe").await? == "SOURCE_VALUE",
            "example_discovery_source_probe",
            "未发布资源时没有读取 metadata 目录中的原值",
        )
    }
    .await;
    let source_shutdown = source_app
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    source_outcome.and(source_shutdown)?;

    // 发布目录按当前公开契约整体替代源码目录；用新应用观察发布后的资源选择。
    let published = root.path().join("resources/lang/vendor/example");
    std::fs::create_dir_all(&published).map_err(Error::internal)?;
    std::fs::write(
        published.join("en.json"),
        r#"{"published_probe":"APPLICATION_ROOT"}"#,
    )
    .map_err(Error::internal)?;
    let mut published_app = ServerApp::builder(sources(root.path())?).build().await?;
    let published_outcome: Result<()> = async {
        check_host(&published_app, &directory).await?;
        check(
            translation(&published_app, "example::published_probe").await? == "APPLICATION_ROOT",
            "example_discovery_published_probe",
            "应用发布资源没有继续使用 application_root 覆盖源码原值",
        )
    }
    .await;
    let published_shutdown = published_app
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    published_outcome.and(published_shutdown)
}
