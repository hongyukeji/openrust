//! 通过真实 `ProviderBuilder` 与 Container 验证共享实例及应用覆盖。

use super::*;
use crate::core::{ProviderBuilder, ServiceProvider};
use crate::hashing::{Argon2IdHasher, ArgonConfig, HashOptions};
use figment::{Figment, providers::Serialized};
use serde_json::json;

fn sources() -> Figment {
    Figment::from(Serialized::defaults(
        json!({"hashing":{"driver":"argon2id","argon":{"memory":1024,"time":1,"threads":1}}}),
    ))
}

#[tokio::test]
async fn provider_default_config_and_trait_alias_share_manager_cached_driver() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder.provider(HashServiceProvider::new());
    let runtime = builder.start(Figment::new()).await?;
    let config = runtime.config().section::<HashingConfig>("hashing")?;
    assert_eq!(config.driver, "bcrypt");
    assert_eq!(config.bcrypt.rounds, 12);
    assert!(config.bcrypt.verify);
    assert_eq!(
        (config.argon.memory, config.argon.time, config.argon.threads),
        (65_536, 4, 1)
    );
    assert!(config.rehash_on_login);
    let first = runtime.container().resolve::<HashManager>().await?;
    let second = runtime.container().resolve::<HashManager>().await?;
    assert!(Arc::ptr_eq(&first, &second));
    let driver = runtime.container().resolve::<dyn Hasher>().await?;
    assert!(Arc::ptr_eq(&driver, &first.driver(None)?));
    assert!(Arc::ptr_eq(
        &driver,
        &runtime.container().resolve::<dyn Hasher>().await?
    ));
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}

#[tokio::test]
async fn provider_reads_application_hashing_config_in_same_lifecycle() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder.provider(HashServiceProvider::new());
    let runtime = builder.start(sources()).await?;
    let driver = runtime.container().resolve::<dyn Hasher>().await?;
    let options = HashOptions::new();
    let encoded = driver.make("test-password", &options).await?;
    assert_eq!(driver.info(&encoded)?.algo_name, "argon2id");
    assert_eq!(
        driver.info(&encoded)?.options.get("memory_cost"),
        Some(&1024.into())
    );
    assert!(
        driver
            .check("test-password", Some(&encoded), &options)
            .await?
    );
    assert!(
        runtime
            .config()
            .section::<HashingConfig>("hashing")?
            .argon
            .verify
    );
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}

struct CustomBindings {
    manager: Option<Arc<HashManager>>,
    hasher: Option<Arc<dyn Hasher>>,
}
impl ServiceProvider for CustomBindings {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if let Some(manager) = &self.manager {
                app.instance(Arc::clone(manager)).await?;
            }
            if let Some(hasher) = &self.hasher {
                app.instance::<dyn Hasher>(Arc::clone(hasher)).await?;
            }
            Ok(())
        })
    }
}
fn custom_hasher() -> Result<Arc<dyn Hasher>> {
    Ok(Arc::new(Argon2IdHasher::new(ArgonConfig {
        memory: 1024,
        time: 1,
        threads: 1,
        verify: true,
    })?))
}

#[tokio::test]
async fn application_manager_binding_is_preserved_and_trait_alias_uses_it() -> Result<()> {
    let manager = Arc::new(HashManager::new(HashingConfig {
        driver: "custom".into(),
        ..HashingConfig::default()
    })?);
    let custom = custom_hasher()?;
    manager.extend("custom", {
        let custom = Arc::clone(&custom);
        move || Ok(Arc::clone(&custom))
    })?;
    let mut builder = ProviderBuilder::new();
    builder
        .provider(CustomBindings {
            manager: Some(Arc::clone(&manager)),
            hasher: None,
        })
        .provider(HashServiceProvider::new());
    let runtime = builder.start(Figment::new()).await?;
    assert!(Arc::ptr_eq(
        &manager,
        &runtime.container().resolve::<HashManager>().await?
    ));
    assert!(Arc::ptr_eq(
        &custom,
        &runtime.container().resolve::<dyn Hasher>().await?
    ));
    assert_eq!(
        runtime.config().section::<HashingConfig>("hashing")?.driver,
        "bcrypt"
    );
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}

#[tokio::test]
async fn explicit_application_hasher_contract_is_not_rebound() -> Result<()> {
    let custom = custom_hasher()?;
    let mut builder = ProviderBuilder::new();
    builder
        .provider(CustomBindings {
            manager: None,
            hasher: Some(Arc::clone(&custom)),
        })
        .provider(HashServiceProvider::new());
    let runtime = builder.start(Figment::new()).await?;
    assert!(Arc::ptr_eq(
        &custom,
        &runtime.container().resolve::<dyn Hasher>().await?
    ));
    assert!(runtime.container().contains::<HashManager>());
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}

struct RepeatedDelegation;
impl ServiceProvider for RepeatedDelegation {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let provider = HashServiceProvider::new();
            provider.register(app).await?;
            provider.register(app).await
        })
    }
}

#[tokio::test]
async fn repeated_delegation_and_independent_provider_registration_are_safe() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder
        .provider(RepeatedDelegation)
        .provider(HashServiceProvider::new())
        .provider(HashServiceProvider::new());
    let runtime = builder.start(sources()).await?;
    let manager = runtime.container().resolve::<HashManager>().await?;
    assert!(Arc::ptr_eq(
        &manager.driver(None)?,
        &runtime.container().resolve::<dyn Hasher>().await?
    ));
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}

#[tokio::test]
async fn schema_rejects_unknown_fields_and_wrong_types() {
    for hashing in [
        json!({"driver":1}),
        json!({"bcrypt":{"rounds":"unsafe-value"}}),
        json!({"argon":{"verify":1}}),
    ] {
        let mut builder = ProviderBuilder::new();
        builder.provider(HashServiceProvider::new());
        assert!(
            builder
                .start(Figment::from(Serialized::defaults(
                    json!({"hashing":hashing})
                )))
                .await
                .is_err()
        );
    }
    assert!(HashServiceProvider::with_max_concurrent_operations(0).is_err());
    assert!(HashServiceProvider::with_max_concurrent_operations(5).is_err());
}

#[tokio::test]
async fn manager_reads_live_default_and_first_named_options_while_alias_stays_cached() -> Result<()>
{
    let mut builder = ProviderBuilder::new();
    builder.provider(HashServiceProvider::new());
    let runtime = builder.start(Figment::new()).await?;
    let manager = runtime.container().resolve::<HashManager>().await?;
    let auth_reference: Arc<dyn Hasher> = Arc::clone(&manager) as Arc<dyn Hasher>;
    let old_alias = runtime.container().resolve::<dyn Hasher>().await?;
    let repository = runtime.config();
    repository.set("hashing.driver", json!("argon2id"))?;
    repository.set("hashing.argon.memory", json!(2048))?;
    repository.set("hashing.argon.time", json!(1))?;
    repository.set("hashing.argon.threads", json!(1))?;
    let selected = manager.driver(None)?;
    assert!(!Arc::ptr_eq(&selected, &old_alias));
    assert!(Arc::ptr_eq(
        &old_alias,
        &runtime.container().resolve::<dyn Hasher>().await?
    ));
    let options = HashOptions::new();
    let encoded = auth_reference.make("test", &options).await?;
    assert_eq!(manager.info(&encoded)?.algo_name, "argon2id");
    assert!(
        auth_reference
            .check("test", Some(&encoded), &options)
            .await?
    );
    assert!(auth_reference.verify_configuration(&encoded)?);
    assert!(!old_alias.verify_configuration(&encoded)?);
    assert!(!auth_reference.needs_rehash(&encoded, &options)?);
    assert!(old_alias.needs_rehash(&encoded, &options)?);
    assert_eq!(
        selected.info(&encoded)?.options.get("memory_cost"),
        Some(&2048.into())
    );
    repository.set("hashing.argon.memory", json!(4096))?;
    assert!(Arc::ptr_eq(&selected, &manager.driver(Some(""))?));
    let cached = selected.make("test", &options).await?;
    assert_eq!(
        selected.info(&cached)?.options.get("memory_cost"),
        Some(&2048.into())
    );
    let first_argon = manager.driver(Some("argon"))?;
    let new_hash = first_argon.make("test", &options).await?;
    assert_eq!(
        first_argon.info(&new_hash)?.options.get("memory_cost"),
        Some(&4096.into())
    );
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}

#[tokio::test]
async fn custom_configuration_remains_in_live_repository_for_extension_factory() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder.provider(HashServiceProvider::new());
    let runtime = builder
        .start(Figment::from(Serialized::defaults(
            json!({"hashing":{"driver":"custom","custom":{"marker":"application-value"}}}),
        )))
        .await?;
    let manager = runtime.container().resolve::<HashManager>().await?;
    let repository = runtime.config().clone();
    manager.extend("custom", move || {
        let marker: String = repository.section("hashing.custom.marker")?;
        if marker != "application-value" {
            return Err(failure(
                "custom_hash_config_missing",
                "自定义驱动配置不在同仓储中",
            ));
        }
        custom_hasher()
    })?;
    let driver = runtime.container().resolve::<dyn Hasher>().await?;
    assert!(Arc::ptr_eq(&driver, &manager.driver(None)?));
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}

#[tokio::test]
async fn invalid_make_cost_can_be_configured_without_breaking_metadata_or_override() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder.provider(HashServiceProvider::new());
    let runtime = builder
        .start(Figment::from(Serialized::defaults(
            json!({"hashing":{"bcrypt":{"rounds":1}}}),
        )))
        .await?;
    let driver = runtime.container().resolve::<dyn Hasher>().await?;
    assert_eq!(driver.info("unknown")?.algo, None);
    assert!(driver.make("test", &HashOptions::new()).await.is_err());
    assert!(
        driver
            .make("test", &HashOptions::from([("rounds".into(), 4.into())]))
            .await
            .is_ok()
    );
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}

#[tokio::test]
async fn first_driver_resolution_captures_current_execution_budget() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder.provider(HashServiceProvider::new());
    let runtime = builder.start(sources()).await?;
    let manager = runtime.container().resolve::<HashManager>().await?;
    runtime
        .config()
        .set("hashing.execution.max_argon_memory", json!(512))?;
    let blocked = manager.driver(None)?;
    assert!(
        matches!(blocked.make("test", &HashOptions::new()).await, Err(error) if error.code() == "hashing_execution_limit_exceeded")
    );
    runtime
        .config()
        .set("hashing.execution.max_argon_memory", json!(1024))?;
    assert!(Arc::ptr_eq(&blocked, &manager.driver(None)?));
    assert!(blocked.make("test", &HashOptions::new()).await.is_err());
    let first_argon = manager.driver(Some("argon"))?;
    assert!(first_argon.make("test", &HashOptions::new()).await.is_ok());
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}

#[tokio::test]
async fn exact_input_capability_tracks_live_default_without_changing_cached_alias() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder.provider(HashServiceProvider::new());
    let runtime = builder.start(Figment::new()).await?;
    let manager = runtime.container().resolve::<HashManager>().await?;
    let manager_trait: Arc<dyn Hasher> = Arc::clone(&manager) as Arc<dyn Hasher>;
    let alias = runtime.container().resolve::<dyn Hasher>().await?;
    let long = "a".repeat(73);
    for input in ["abc\0tail", long.as_str()] {
        assert!(!alias.accepts_exact_input(input)?);
        assert!(!manager.accepts_exact_input(input)?);
        assert!(!manager_trait.accepts_exact_input(input)?);
    }
    runtime.config().set("hashing.driver", json!("argon2id"))?;
    runtime.config().set("hashing.argon.verify", json!(true))?;
    for input in ["abc\0tail", long.as_str()] {
        assert!(manager.accepts_exact_input(input)?);
        assert!(manager_trait.accepts_exact_input(input)?);
        assert!(!alias.accepts_exact_input(input)?);
    }
    runtime.config().set("hashing.argon.verify", json!(false))?;
    runtime.config().set("hashing.driver", json!("argon"))?;
    for input in ["abc\0tail", long.as_str()] {
        assert!(!manager.accepts_exact_input(input)?);
        assert!(!manager_trait.accepts_exact_input(input)?);
    }
    runtime.config().set("hashing.driver", json!("argon2id"))?;
    assert!(manager.accepts_exact_input("abc\0tail")?);
    assert!(Arc::ptr_eq(
        &alias,
        &runtime.container().resolve::<dyn Hasher>().await?
    ));
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}
