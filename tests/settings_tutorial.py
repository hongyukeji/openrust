"""提取隔离快照中的三语 Settings 教程，生成真实 Rust 消费者验收。"""
from pathlib import Path
import argparse
import re

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--source", type=Path, required=True, help="项目 target/tmp/ 内的隔离源码快照")
args = parser.parse_args()
source_root = args.source.resolve()
if not source_root.is_relative_to(root / "target/tmp") or not (source_root / "Cargo.toml").is_file():
    parser.error("只能向项目 target/tmp/ 内的既有源码快照生成测试")
fence = chr(96) * 3
blocks = {
    lang: re.findall(fence + r"rust[^\n]*\n(.*?)" + fence,
                     (source_root / "docs" / lang / "packages/settings.md").read_text(), re.S)
    for lang in ["en", "zh-Hans", "zh-Hant"]
}


def pick(lang, token):
    found = [block for block in blocks[lang] if token in block]
    assert len(found) == 1, (lang, token, len(found))
    return found[0]


site = pick("en", "pub(crate) struct SiteSettings")
provider = pick("en", "fn boot<'a>")
for lang in blocks:
    assert pick(lang, "pub(crate) struct SiteSettings") == site
    assert pick(lang, "fn boot<'a>") == provider
imports, method = provider.split("fn boot<'a>", 1)
source = """mod support;
use openrust::{
    contracts::{config::Repository as _, foundation::Application as _},
    foundation::Application, http::HttpFuture, support::{ProviderResult, ServiceProvider},
};
use openrust_settings::{SettingsManager, SettingsServiceProvider};
use serde_json::json;
mod app { pub(crate) mod settings {
""" + site + """
}}
mod consumer {
""" + imports + """
pub struct Consumer;
impl openrust::support::ServiceProvider for Consumer {
fn boot<'a>""" + method + """
}}
struct Configure { database: serde_json::Value, table: String }
impl ServiceProvider for Configure {
 fn register<'a>(&'a mut self, app:&'a Application)->HttpFuture<'a,ProviderResult> {
  Box::pin(async move {
   let config=app.config()?;
   config.set("database",self.database.clone())?;
   config.set("settings.repositories.database.table",json!(self.table))?;
   config.set("settings.refresh_interval",json!(0))?;
   config.set("cache",json!({"default":"array","stores":{"array":{"driver":"array"}}}))?;
   Ok(())
  })
 }
}
"""
for lang in blocks:
    name = lang.lower().replace("-", "_")
    source += """
#[tokio::test]
async fn tutorial_""" + name + """() {
 let fixture=support::Fixture::new().await;
 let root=tempfile::tempdir().unwrap();
 let mut app=Application::configure(root.path().to_owned()).with_providers(vec![
   Box::new(SettingsServiceProvider), Box::new(Configure {database:fixture.config.get("database").unwrap(),table:fixture.table.clone()}), Box::new(consumer::Consumer)
 ]).create().unwrap();
 app.boot().await.unwrap();
 let result: Result<(),Box<dyn std::error::Error+Send+Sync>>=app.container().scope(async {
""" + pick(lang, "let current = settings.get().await;") + pick(lang, 'settings.lock("title").await?;') + pick(lang, "let mut transaction =") + pick(lang, "let handler = manager.on_change") + """
 let cache=app.make::<CacheManager>().await?.store(None)?;
 cache.put("site:rendered", &openrust::cache::Value::Json(json!("old")),None).await?;
 settings.fill(json!({"title":"changed after callback registration"})).await?;
 settings.save().await?;
 manager.poll().await?;
 assert!(cache.get("site:rendered").await?.is_none());
 let _ = (current,state);
""" + pick(lang, 'manager.forget_handler("site", handler).await;') + """
 Ok(())
 }).await;
 result.unwrap();
 app.make::<SettingsManager>().await.unwrap().shutdown().await;
 drop(app);
 fixture.close().await;
}
"""
target = source_root / "vendor/openrust/settings/tests/tutorial_generated.rs"
if not target.resolve().is_relative_to(source_root):
    parser.error("生成路径不能通过符号链接逃逸源码快照")
target.write_text(source)
print("已直接提取三语教程生成真实消费者验收")
