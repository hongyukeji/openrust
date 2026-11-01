"""在全新生成应用中安装 Rust 配置区段、Provider 和具名 HTTP 回归。"""

import hashlib
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def formatted_rust(source: str) -> str:
    return subprocess.run(
        ["rustfmt", "--edition", "2024"],
        input=source,
        text=True,
        capture_output=True,
        check=True,
        timeout=15,
    ).stdout


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("用法：install_generated_fixture.py <生成应用目录> <本分支框架目录>")
    project = Path(sys.argv[1]).resolve(strict=True)
    framework = Path(sys.argv[2]).resolve(strict=True)
    consumer = (framework / "tests/consumers/config-repository-reading").resolve(strict=True)
    manifest = project / "Cargo.toml"
    bootstrap = project / "bootstrap/app.rs"
    providers = project / "bootstrap/providers.rs"
    config_manifest = project / "config/mod.rs"
    cache_config = project / "config/cache.rs"
    test = project / "tests/config_repository_reading.rs"
    reader = project / "config/reader.rs"
    if (project / "config").is_symlink() or not (project / "config").is_dir():
        raise SystemExit("生成应用的 config 目录必须是真实目录")
    for path in (manifest, bootstrap, providers, config_manifest, cache_config, test, reader):
        if path.is_symlink() or not path.resolve().is_relative_to(project):
            raise SystemExit("fixture 路径必须属于生成应用且不是符号链接")
    data = tomllib.loads(manifest.read_text())
    resolved = (project / data["workspace"]["dependencies"]["openrust"]["path"]).resolve(strict=True)
    if resolved != framework:
        raise SystemExit(f"生成应用依赖的框架不属于当前分支：{resolved}")
    source_manifest = manifest.read_text()
    source_bootstrap = bootstrap.read_text()
    source_providers = providers.read_text()
    source_config_manifest = config_manifest.read_text()
    source_cache_config = cache_config.read_text()
    if (
        "config-repository-reading-consumer" in source_manifest
        or test.exists()
        or test.is_symlink()
        or reader.exists()
        or reader.is_symlink()
    ):
        raise SystemExit("fixture 已安装；只接受全新生成应用")
    match = re.search(r"pub struct (\w+);", source_bootstrap)
    if match is None:
        raise SystemExit("无法定位生成应用类型")
    provider_marker = "    providers.push(Box::new(crate::app::providers::AppServiceProvider));"
    module_marker = "pub mod app;"
    section_marker = '.section("app", app::config)'
    cache_marker = 'default: ctx.env_or("CACHE_STORE", "redis"),'
    if (
        source_providers.count(provider_marker) != 1
        or source_config_manifest.count(module_marker) != 1
        or source_config_manifest.count(section_marker) != 1
        or source_cache_config.count(cache_marker) != 1
        or source_manifest.count("[dependencies]\n") != 1
    ):
        raise SystemExit("生成模板结构不符合预期")
    test_source = f'''#[path = "../app/mod.rs"]
mod app;
#[path = "../bootstrap/app.rs"]
mod bootstrap;
#[path = "../config/mod.rs"]
mod config;
#[path = "../modules/mod.rs"]
mod modules;
#[path = "../routes/web.rs"]
mod routes;

use bootstrap::{match.group(1)} as GeneratedApplication;
use openrust::http::axum::{{body::Body, http::{{Request, StatusCode}}}};
use openrust::config::ConfigLoader;
use openrust::core::Application;
use openrust::testing::TestApp;
use std::time::Duration;

#[tokio::test]
async fn generated_application_installs_config_reading_provider()
-> Result<(), Box<dyn std::error::Error>> {{
    let sources = ConfigLoader::application_code_sources(
        env!("CARGO_MANIFEST_DIR"), "APP_", None, &GeneratedApplication::config(),
    )?.merge_ordered_entries([("app", serde_json::json!({{"env": "testing"}}))])?;
    assert_eq!(
        sources.figment().extract_inner::<openrust::config::Environment>("app.env")?,
        openrust::config::Environment::Testing,
    );
    let name: String = sources.figment().extract_inner("app.name")?;
    assert_eq!(
        sources.figment().extract_inner::<serde_json::Value>("reader")?,
        serde_json::json!({{"z_label": "先声明", "a_label": "后声明"}}),
    );
    let mut app = TestApp::<GeneratedApplication>::builder(sources).build().await?;
    let order = app.request(Request::get("/api/config-reading/order").body(Body::empty())?).await?;
    order.assert_status(StatusCode::OK)?;
    let order_body: serde_json::Value = serde_json::from_slice(order.body())?;
    assert_eq!(order_body["data"], serde_json::json!({{"keys": ["z_label", "a_label"]}}));
    let response = app.request(Request::get("/api/config-reading").body(Body::empty())?).await?;
    response.assert_status(StatusCode::OK)?;
    let body: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(body["data"], serde_json::json!({{"name": name, "locale": "zh_CN"}}));
    let startup_name = app.app()?.config().app.name.clone();
    let write = app.request(Request::post("/api/config-reading/review").body(Body::empty())?).await?;
    write.assert_status(StatusCode::OK)?;
    let write_body: serde_json::Value = serde_json::from_slice(write.body())?;
    assert_eq!(write_body["data"], serde_json::json!({{"name": "已复核应用"}}));
    let response = app.request(Request::get("/api/config-reading").body(Body::empty())?).await?;
    response.assert_status(StatusCode::OK)?;
    let body: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(body["data"], serde_json::json!({{"name": "已复核应用", "locale": "zh_CN"}}));
    assert_eq!(app.app()?.config_repository().get("app.name")?, Some(serde_json::json!("已复核应用")));
    assert_eq!(app.app()?.config_repository().string("app.name")?, "已复核应用");
    app.app()?.config_repository().set("reader.z_label", serde_json::json!("已复核"))?;
    assert_eq!(config_repository_reading_consumer::public_reader_key_order(&app.app()?.config_repository())?, ["z_label", "a_label"]);
    let order = app.request(Request::get("/api/config-reading/order").body(Body::empty())?).await?;
    order.assert_status(StatusCode::OK)?;
    let order_body: serde_json::Value = serde_json::from_slice(order.body())?;
    assert_eq!(order_body["data"], serde_json::json!({{"keys": ["z_label", "a_label"]}}));
    assert_eq!(app.app()?.config().app.name, startup_name);
    app.shutdown(openrust::tokio::time::Instant::now() + Duration::from_secs(5)).await?;
    Ok(())
}}
'''
    before = {
        "Cargo.toml": digest(manifest),
        "bootstrap/app.rs": digest(bootstrap),
        "bootstrap/providers.rs": digest(providers),
        "config/mod.rs": digest(config_manifest),
        "config/cache.rs": digest(cache_config),
        "tests/config_repository_reading.rs": None,
        "config/reader.rs": None,
    }
    updated_manifest = source_manifest.replace(
        "[dependencies]\n",
        "[dependencies]\nconfig-repository-reading-consumer = { path = "
        + json.dumps(str(consumer), ensure_ascii=False)
        + " }\n",
        1,
    )
    updated_providers = formatted_rust(source_providers.replace(
        provider_marker,
        provider_marker
        + "\n    providers.push(Box::new(config_repository_reading_consumer::ReadingProvider));",
        1,
    ))
    updated_config_manifest = formatted_rust(
        source_config_manifest.replace(module_marker, module_marker + "\npub mod reader;", 1)
        .replace(section_marker, section_marker + '\n        .section("reader", reader::config)', 1)
    )
    updated_cache_config = formatted_rust(source_cache_config.replace(
        cache_marker,
        'default: "default".to_owned(),',
        1,
    ).replace(
        "    cache.stores.insert(",
        '    cache.stores.insert("default".to_owned(), CacheStoreConfig { '
        'driver: "memory".to_owned(), namespace: "config-reader-fixture".to_owned(), '
        'options: std::collections::BTreeMap::new() });\n    cache.stores.insert(',
        1,
    ))
    updated_test = formatted_rust(test_source)
    updated_reader = formatted_rust('''//! 独立 consumer 的业务配置区段。

use openrust::config::{ConfigContext, ConfigError};

/// 保留业务键的声明顺序并进入同一 ConfigRepository。
pub fn config(_ctx: &ConfigContext) -> Result<serde_json::Value, ConfigError> {
    Ok(serde_json::json!({"z_label": "先声明", "a_label": "后声明"}))
}
''')
    manifest.write_text(updated_manifest)
    providers.write_text(updated_providers)
    config_manifest.write_text(updated_config_manifest)
    cache_config.write_text(updated_cache_config)
    test.write_text(updated_test)
    reader.write_text(updated_reader)
    after = {
        "Cargo.toml": digest(manifest),
        "bootstrap/app.rs": digest(bootstrap),
        "bootstrap/providers.rs": digest(providers),
        "config/mod.rs": digest(config_manifest),
        "config/cache.rs": digest(cache_config),
        "tests/config_repository_reading.rs": digest(test),
        "config/reader.rs": digest(reader),
    }
    print(json.dumps({"framework": str(framework), "before": before, "after": after}, ensure_ascii=False))


if __name__ == "__main__":
    main()
