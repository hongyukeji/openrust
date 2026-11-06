//! 从原核心 Provider 测试迁入，保留模块配置/依赖/Events 合同。
#![allow(
    clippy::unwrap_used,
    reason = "保留原受控测试日志锁断言，不用于生产代码"
)]

use figment::Figment;
use openrust::config::repository::SectionSchema;
use openrust::core::{
    BootContext, BoxFuture, Error, ProviderBuilder, RegisterContext, Result, ServiceProvider,
};
use openrust_modules::{
    ModuleConfigFiles, ModuleDescriptor, ModuleManifest, ModuleProviderBuilderExt,
};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::time::{Duration, Instant};

struct ModuleBefore(Arc<Mutex<Vec<String>>>);
struct ModuleSupport {
    root: std::path::PathBuf,
    reads: Arc<Mutex<Vec<String>>>,
}
struct ModuleAfter(Arc<Mutex<Vec<String>>>);

impl ServiceProvider for ModuleBefore {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.config_section(
                "sample",
                serde_json::json!({"name":"Default","items":["a","b","c"],"settings":{"value":"default"}}),
                SectionSchema::any(),
                |_| Ok(()),
            )
        })
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let value: Value = app.section("sample")?;
            self.0
                .lock()
                .unwrap()
                .push(format!("before:{}", value["name"]));
            Ok(())
        })
    }
}

impl ServiceProvider for ModuleSupport {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let before: Value = app.section("sample")?;
            self.reads
                .lock()
                .unwrap()
                .push(format!("support-before:{}", before["name"]));
            ModuleConfigFiles::new("sample", &self.root)?.overlay(app)?;
            let after: Value = app.section("sample")?;
            self.reads
                .lock()
                .unwrap()
                .push(format!("support-after:{}", after["name"]));
            Ok(())
        })
    }
}

impl ServiceProvider for ModuleAfter {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let value: Value = app.section("sample")?;
            assert_eq!(value["name"], "Module");
            assert_eq!(value["items"], serde_json::json!(["x", "b", "c"]));
            assert_eq!(value["settings"]["value"], "settings");
            self.0.lock().unwrap().push("after".to_owned());
            Ok(())
        })
    }
}

#[tokio::test]
async fn module_support_overlays_at_its_own_boot_point() -> Result<()> {
    let temporary = tempfile::tempdir().map_err(Error::internal)?;
    let config = temporary.path().join("config");
    std::fs::create_dir(&config).map_err(Error::internal)?;
    std::fs::write(
        config.join("config.toml"),
        "name = 'Module'\nitems = ['x']\n",
    )
    .map_err(Error::internal)?;
    std::fs::write(config.join("settings.toml"), "value = 'settings'\n")
        .map_err(Error::internal)?;
    let reads = Arc::new(Mutex::new(Vec::new()));
    let module = ModuleDescriptor::new("sample", temporary.path(), 0)
        .provider(ModuleBefore(Arc::clone(&reads)))
        .provider(ModuleSupport {
            root: temporary.path().to_path_buf(),
            reads: Arc::clone(&reads),
        })
        .provider(ModuleAfter(Arc::clone(&reads)));
    let mut builder = ProviderBuilder::new();
    builder.module_manifest(ModuleManifest::new().module(module).enable("sample"));
    let source = Figment::from(figment::providers::Serialized::defaults(
        serde_json::json!({"sample":{"name":"App","items":["a","b","c"],"settings":{"value":"app"}}}),
    ));
    let runtime = builder.start(source).await?;
    assert_eq!(
        *reads.lock().unwrap(),
        [
            "before:\"App\"",
            "support-before:\"App\"",
            "support-after:\"Module\"",
            "after"
        ]
    );
    assert!(runtime.config().version() >= 2);
    runtime
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await
}

#[tokio::test]
async fn module_dependency_orders_provider_register_and_boot() -> Result<()> {
    struct Producer(Arc<Mutex<Vec<&'static str>>>);
    struct Consumer(Arc<Mutex<Vec<&'static str>>>);

    impl ServiceProvider for Producer {
        fn register<'a>(&'a self, _app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                self.0
                    .lock()
                    .map_err(|_| Error::custom("test_event_lock", "测试事件锁已中毒"))?
                    .push("producer.register");
                Ok(())
            })
        }

        fn boot<'a>(&'a self, _app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            let events = Arc::clone(&self.0);
            Box::pin(async move {
                events
                    .lock()
                    .map_err(|_| Error::custom("test_event_lock", "测试事件锁已中毒"))?
                    .push("producer.boot");
                Ok(())
            })
        }
    }

    impl ServiceProvider for Consumer {
        fn register<'a>(&'a self, _app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                self.0
                    .lock()
                    .map_err(|_| Error::custom("test_event_lock", "测试事件锁已中毒"))?
                    .push("consumer.register");
                Ok(())
            })
        }

        fn boot<'a>(&'a self, _app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            let events = Arc::clone(&self.0);
            Box::pin(async move {
                events
                    .lock()
                    .map_err(|_| Error::custom("test_event_lock", "测试事件锁已中毒"))?
                    .push("consumer.boot");
                Ok(())
            })
        }
    }

    let temporary = tempfile::tempdir().map_err(Error::internal)?;
    let producer_root = temporary.path().join("producer");
    let consumer_root = temporary.path().join("consumer");
    std::fs::create_dir(&producer_root).map_err(Error::internal)?;
    std::fs::create_dir(&consumer_root).map_err(Error::internal)?;
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut builder = ProviderBuilder::new();
    builder.module_manifest(
        ModuleManifest::new()
            .module(
                ModuleDescriptor::new("consumer", &consumer_root, -100)
                    .requires("producer")
                    .provider(Consumer(Arc::clone(&events))),
            )
            .module(
                ModuleDescriptor::new("producer", &producer_root, 100)
                    .provider(Producer(Arc::clone(&events))),
            )
            .enable("consumer")
            .enable("producer"),
    );

    let runtime = builder.start(Figment::new()).await?;
    let observed = {
        let events = events
            .lock()
            .map_err(|_| Error::custom("test_event_lock", "测试事件锁已中毒"))?;
        events.clone()
    };
    assert_eq!(
        observed,
        vec![
            "producer.register",
            "consumer.register",
            "producer.boot",
            "consumer.boot"
        ]
    );
    runtime
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await
}
