"""在全新生成应用中显式安装配置读取 Provider 和具名 HTTP 回归。"""

import hashlib
import json
import re
import sys
import tomllib
from pathlib import Path


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("用法：install_generated_fixture.py <生成应用目录> <本分支框架目录>")
    project = Path(sys.argv[1]).resolve(strict=True)
    framework = Path(sys.argv[2]).resolve(strict=True)
    consumer = (framework / "tests/consumers/config-repository-reading").resolve(strict=True)
    manifest = project / "Cargo.toml"
    bootstrap = project / "bootstrap/app.rs"
    test = project / "tests/config_repository_reading.rs"
    reader = project / "config/reader.toml"
    if (project / "config").is_symlink() or not (project / "config").is_dir():
        raise SystemExit("生成应用的 config 目录必须是真实目录")
    data = tomllib.loads(manifest.read_text())
    resolved = (project / data["workspace"]["dependencies"]["openrust"]["path"]).resolve(strict=True)
    if resolved != framework:
        raise SystemExit(f"生成应用依赖的框架不属于当前分支：{resolved}")
    source_manifest = manifest.read_text()
    source_bootstrap = bootstrap.read_text()
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
    marker = "        providers.push(Box::new(RoutesProvider));"
    if source_bootstrap.count(marker) != 1 or source_manifest.count("[dependencies]\n") != 1:
        raise SystemExit("生成模板结构不符合预期")
    test_source = f'''#[path = "../bootstrap/app.rs"]
mod bootstrap;
#[path = "../modules/mod.rs"]
mod modules;
#[path = "../routes/web.rs"]
mod routes;

use bootstrap::{match.group(1)} as GeneratedApplication;
use figment::{{Figment, providers::{{Format, Serialized, Toml}}}};
use openrust::http::axum::{{body::Body, http::{{Request, StatusCode}}}};
use openrust::config::ConfigSources;
use openrust::testing::TestApp;
use std::time::Duration;

#[tokio::test]
async fn generated_application_installs_config_reading_provider()
-> Result<(), Box<dyn std::error::Error>> {{
    let source = Figment::from(Serialized::defaults(openrust::config::FrameworkConfig::default()))
        .merge(Toml::string("[app]\\nname = 'generated-config-reader'\\n[cache]\\ndefault = 'default'\\n[cache.stores.default]\\ndriver = 'memory'\\n[view]\\ndirectory = 'resources/views'\\n"));
    let root: serde_json::Value = source.extract()?;
    let mut entries = root.as_object().ok_or("生成配置根必须是对象")?.iter()
        .map(|(key, value)| (key.clone(), value.clone())).collect::<Vec<_>>();
    entries.push(("reader".to_owned(), serde_json::json!({{"z_label": "先声明", "a_label": "后声明"}})));
    let sources = ConfigSources::from_ordered_entries(entries)?;
    let mut app = TestApp::<GeneratedApplication>::builder(sources).build().await?;
    let order = app.request(Request::get("/api/config-reading/order").body(Body::empty())?).await?;
    order.assert_status(StatusCode::OK)?;
    let order_body: serde_json::Value = serde_json::from_slice(order.body())?;
    assert_eq!(order_body["data"], serde_json::json!({{"keys": ["z_label", "a_label"]}}));
    let response = app.request(Request::get("/api/config-reading").body(Body::empty())?).await?;
    response.assert_status(StatusCode::OK)?;
    let body: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(body["data"], serde_json::json!({{"name": "generated-config-reader", "locale": "zh_CN"}}));
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
        "tests/config_repository_reading.rs": None,
        "config/reader.toml": None,
    }
    manifest.write_text(
        source_manifest.replace(
            "[dependencies]\n",
            "[dependencies]\nconfig-repository-reading-consumer = { path = "
            + json.dumps(str(consumer), ensure_ascii=False)
            + " }\n",
            1,
        )
    )
    bootstrap.write_text(
        source_bootstrap.replace(
            marker,
            "        providers.push(Box::new(config_repository_reading_consumer::ReadingProvider));\n"
            + marker,
            1,
        )
    )
    test.write_text(test_source)
    reader.write_text('[reader]\nz_label = "先声明"\na_label = "后声明"\n')
    after = {
        "Cargo.toml": digest(manifest),
        "bootstrap/app.rs": digest(bootstrap),
        "tests/config_repository_reading.rs": digest(test),
        "config/reader.toml": digest(reader),
    }
    print(json.dumps({"framework": str(framework), "before": before, "after": after}, ensure_ascii=False))


if __name__ == "__main__":
    main()
