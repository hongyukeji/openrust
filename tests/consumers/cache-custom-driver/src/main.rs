//! 展示独立应用如何注册公开缓存 driver，并通过命名 store 使用。

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use figment::providers::Format;
use openrust::{
    BoxFuture, Error, Result,
    cache::{
        Cache, CacheBackend, CacheDriver, CacheDriverConfig, CacheScope, CacheServiceProvider,
    },
    core::ProviderBuilder,
};
use tokio::sync::Mutex;

struct PrefixDriver;
impl CacheDriver for PrefixDriver {
    fn name(&self) -> &'static str {
        "example-prefix"
    }

    fn build<'a>(
        &'a self,
        config: &'a CacheDriverConfig,
    ) -> BoxFuture<'a, Result<Arc<dyn CacheBackend>>> {
        Box::pin(async move {
            let prefix = config
                .option::<String>("key_prefix")?
                .unwrap_or_else(|| config.store_name().to_owned());
            Ok(Arc::new(PrefixBackend {
                prefix,
                values: Mutex::new(HashMap::new()),
            }) as Arc<dyn CacheBackend>)
        })
    }
}

struct PrefixBackend {
    prefix: String,
    values: Mutex<HashMap<String, (Vec<u8>, Option<Instant>)>>,
}

#[async_trait]
impl CacheBackend for PrefixBackend {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let mut values = self.values.lock().await;
        let key = self.key(key);
        if values
            .get(&key)
            .is_some_and(|(_, expires)| expires.is_some_and(|time| time <= Instant::now()))
        {
            values.remove(&key);
        }
        Ok(values.get(&key).map(|(value, _)| value.clone()))
    }

    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()> {
        let expires = ttl
            .map(|duration| {
                Instant::now().checked_add(duration).ok_or_else(|| {
                    Error::custom("example_ttl_invalid", "TTL exceeds supported range")
                })
            })
            .transpose()?;
        self.values
            .lock()
            .await
            .insert(self.key(key), (value, expires));
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<()> {
        self.values.lock().await.remove(&self.key(key));
        Ok(())
    }

    fn scope(&self) -> CacheScope {
        CacheScope::Process
    }
}

impl PrefixBackend {
    fn key(&self, key: &str) -> String {
        format!("{}:{key}", self.prefix)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = figment::Figment::new().merge(figment::providers::Toml::string(
        "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'example-prefix'\nkey_prefix = 'example'\n",
    ));
    let mut builder = ProviderBuilder::new();
    builder.provider(CacheServiceProvider::new().with_driver(PrefixDriver));
    let runtime = builder.start(config).await?;
    let cache = runtime.container().resolve::<Cache>().await?;
    cache
        .set("greeting", &"hello", Some(Duration::from_secs(30)))
        .await?;
    if cache.get::<String>("greeting").await?.as_deref() != Some("hello") {
        return Err(Error::custom(
            "example_cache_failed",
            "custom cache driver failed its roundtrip",
        ));
    }
    runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    struct PublicResource;

    #[openrust::resource]
    impl PublicResource {
        async fn list() -> &'static str {
            "public"
        }
    }

    #[test]
    fn resource_macro_resolves_the_public_framework_dependency() {
        // 框架迁回根工程后，宏必须继续通过下游的 openrust 依赖找到 Resource。
        let _ = <PublicResource as openrust::http::Resource>::actions(
            openrust::http::ResourceActions::new(),
        );
    }
}
