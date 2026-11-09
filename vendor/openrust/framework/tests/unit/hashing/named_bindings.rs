//! 通过应用容器验证哈希管理器与默认驱动的公开具名入口。

use crate::Result;
use crate::container::ServiceKey;
use crate::foundation::{ProviderBuilder, RegisterContext};
use crate::hashing::{
    Argon2IdHasher, ArgonConfig, HashManager, HashOptions, HashServiceProvider, Hasher,
    HashingConfig,
};
use crate::support::{BoxFuture, ServiceProvider};
use figment::{Figment, providers::Serialized};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

#[tokio::test]
async fn named_hash_bindings_share_the_selected_driver_and_its_singleton() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder.provider(HashServiceProvider::new());
    let runtime = builder
        .start(Figment::from(Serialized::defaults(json!({
            "hashing": {"argon": {"memory": 1024, "time": 1, "threads": 1}}
        }))))
        .await?;
    runtime.config().set("hashing.driver", json!("argon2id"))?;

    let container = runtime.container();
    let manager_key = ServiceKey::named("hash");
    let driver_key = ServiceKey::named("hash.driver");
    assert!(container.is_alias(&manager_key));
    assert!(container.is_alias(&driver_key));
    let manager = container.resolve_key::<HashManager>(manager_key).await?;
    assert!(Arc::ptr_eq(
        &manager,
        &container.resolve::<HashManager>().await?
    ));
    let driver = container
        .resolve_key::<dyn Hasher>(driver_key.clone())
        .await?;
    assert!(Arc::ptr_eq(&driver, &manager.driver(None)?));
    assert!(Arc::ptr_eq(
        &driver,
        &container.resolve::<dyn Hasher>().await?
    ));
    let options = HashOptions::new();
    let encoded = driver.make(b"named-driver", &options).await?;
    let info = driver.info(&encoded)?;
    assert_eq!(info.algo_name, "argon2id");
    assert_eq!(info.options.get("memory_cost"), Some(&json!(1024)));

    runtime.config().set("hashing.driver", json!("bcrypt"))?;
    assert!(!Arc::ptr_eq(&driver, &manager.driver(None)?));
    assert!(Arc::ptr_eq(
        &driver,
        &container.resolve_key::<dyn Hasher>(driver_key).await?
    ));
    assert!(Arc::ptr_eq(
        &driver,
        &container.resolve::<dyn Hasher>().await?
    ));
    runtime
        .shutdown(Instant::now() + Duration::from_secs(2))
        .await
}

struct TypedOverrides {
    manager: Arc<HashManager>,
    driver: Arc<dyn Hasher>,
}

struct NamedOverrides {
    manager: Arc<HashManager>,
    driver: Arc<dyn Hasher>,
}

impl ServiceProvider for NamedOverrides {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let manager = ServiceKey::named("hash");
            let driver = ServiceKey::named("hash.driver");
            app.instance_key(manager.clone(), Arc::clone(&self.manager))
                .await?;
            app.instance_key(driver.clone(), Arc::clone(&self.driver))
                .await?;
            app.alias(manager, ServiceKey::typed::<HashManager>())?;
            app.alias(driver, ServiceKey::typed::<dyn Hasher>())
        })
    }
}

impl ServiceProvider for TypedOverrides {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.instance(Arc::clone(&self.manager)).await?;
            app.instance(Arc::clone(&self.driver)).await
        })
    }
}

fn custom_driver(memory: u32) -> Result<Arc<dyn Hasher>> {
    Ok(Arc::new(Argon2IdHasher::new(ArgonConfig {
        memory,
        time: 1,
        threads: 1,
        verify: true,
    })?))
}

#[tokio::test]
async fn named_hash_bindings_keep_the_application_owned_alias_direction() -> Result<()> {
    let manager = Arc::new(HashManager::new(HashingConfig::default())?);
    let driver = custom_driver(1024)?;
    let mut builder = ProviderBuilder::new();
    builder
        .provider(NamedOverrides {
            manager: Arc::clone(&manager),
            driver: Arc::clone(&driver),
        })
        .provider(HashServiceProvider::new());
    let runtime = builder.start(Figment::new()).await?;
    let container = runtime.container();
    let named_manager = ServiceKey::named("hash");
    let named_driver = ServiceKey::named("hash.driver");
    assert_eq!(
        container.get_alias(&ServiceKey::typed::<HashManager>())?,
        named_manager
    );
    assert_eq!(
        container.get_alias(&ServiceKey::typed::<dyn Hasher>())?,
        named_driver
    );
    assert!(Arc::ptr_eq(
        &manager,
        &container.resolve::<HashManager>().await?
    ));
    assert!(Arc::ptr_eq(
        &driver,
        &container.resolve::<dyn Hasher>().await?
    ));
    assert!(Arc::ptr_eq(
        &manager,
        &container.resolve_key::<HashManager>(named_manager).await?
    ));
    assert!(Arc::ptr_eq(
        &driver,
        &container.resolve_key::<dyn Hasher>(named_driver).await?
    ));
    runtime
        .shutdown(Instant::now() + Duration::from_secs(2))
        .await
}

#[tokio::test]
async fn named_hash_driver_decorates_the_same_typed_application_override() -> Result<()> {
    let manager = Arc::new(HashManager::new(HashingConfig::default())?);
    let driver = custom_driver(1024)?;
    let decorated = custom_driver(2048)?;
    let mut builder = ProviderBuilder::new();
    builder
        .provider(TypedOverrides {
            manager: Arc::clone(&manager),
            driver: Arc::clone(&driver),
        })
        .provider(HashServiceProvider::new());
    let runtime = builder.start(Figment::new()).await?;
    let container = runtime.container();
    let driver_key = ServiceKey::named("hash.driver");
    let named_manager = container
        .resolve_key::<HashManager>(ServiceKey::named("hash"))
        .await?;
    assert!(Arc::ptr_eq(&manager, &named_manager));
    assert!(Arc::ptr_eq(
        &manager,
        &container.resolve::<HashManager>().await?
    ));
    assert!(Arc::ptr_eq(
        &driver,
        &container
            .resolve_key::<dyn Hasher>(driver_key.clone())
            .await?
    ));
    assert!(Arc::ptr_eq(
        &driver,
        &container.resolve::<dyn Hasher>().await?
    ));

    container
        .extend_key::<dyn Hasher, _>(&driver_key, {
            let original = Arc::clone(&driver);
            let decorated = Arc::clone(&decorated);
            move |value, _| {
                assert!(Arc::ptr_eq(&value, &original));
                let decorated = Arc::clone(&decorated);
                Box::pin(async move { Ok(decorated) })
            }
        })
        .await?;
    let named = container.resolve_key::<dyn Hasher>(driver_key).await?;
    let typed = container.resolve::<dyn Hasher>().await?;
    assert!(Arc::ptr_eq(&decorated, &named));
    assert!(Arc::ptr_eq(&decorated, &typed));
    runtime
        .shutdown(Instant::now() + Duration::from_secs(2))
        .await
}
