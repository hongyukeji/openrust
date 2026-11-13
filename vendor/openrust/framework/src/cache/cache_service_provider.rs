use std::sync::Arc;

use crate::{
    contracts::{
        cache::Factory, config::Repository as Config, container::Container as _,
        foundation::Application as _,
    },
    database::DatabaseManager,
    foundation::Application,
    http::HttpFuture,
    redis::RedisManager,
    support::{ProviderResult, ServiceProvider},
};

use super::memoized_store::MemoizedRepositories;
use super::{CacheManager, RateLimiter};

/// Registers the cache manager in the application container.
pub struct CacheServiceProvider;

impl ServiceProvider for CacheServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container();
            let base = app.base_path().to_path_buf();
            let owner = Arc::downgrade(&app.container_handle());
            container.scoped::<MemoizedRepositories, _>(|_| Ok(Arc::default()))?;
            container.singletonNamed("cache", move |container| {
                let config = container.get::<dyn Config>("config")?;
                let database = container.get::<DatabaseManager>("db")?;
                let redis = container.get::<RedisManager>("redis")?;
                Ok(Arc::new(
                    CacheManager::new(config, database, base.clone())
                        .with_redis(redis)
                        .with_owner(owner.clone()),
                ))
            })?;
            container.alias(
                "cache",
                crate::container::ServiceKey::typed::<CacheManager>(),
            )?;
            container.singletonNamed("rateLimiter", |container| {
                let config = container.get::<dyn Config>("config")?;
                let store = config
                    .get("cache.limiter")
                    .and_then(|value| value.as_str().map(str::to_owned));
                Ok(Arc::new(RateLimiter::with_cache(
                    container.get::<CacheManager>("cache")?,
                    store,
                )))
            })?;
            container.alias(
                "rateLimiter",
                crate::container::ServiceKey::typed::<RateLimiter>(),
            )?;
            container.singleton::<dyn Factory, _>(|container| {
                let manager = container.get::<CacheManager>("cache")?;
                Ok(manager)
            })?;
            Ok(())
        })
    }
}
