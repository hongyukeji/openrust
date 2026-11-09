//! 固定 Laravel 有序标签与 Rust 缓存身份隔离回归。

use super::*;
use crate::cache::{Cache, CacheServiceProvider};
use crate::foundation::ProviderBuilder;
use crate::tenant::{TenantContext, TenantId};
use figment::{Figment, providers::Serialized};
use serde_json::json;
use tokio::sync::Notify;

const TTL: Duration = Duration::from_secs(10);

async fn memory_store() -> Result<CacheStore> {
    let mut builder = ProviderBuilder::new();
    builder.provider(CacheServiceProvider::new());
    let runtime = builder
        .start(Figment::from(Serialized::defaults(json!({
            "cache": {"default": "memory", "stores": {"memory": {"driver": "memory"}}}
        }))))
        .await?;
    runtime
        .container()
        .resolve::<Cache>()
        .await?
        .default_store()
        .await
}

#[tokio::test]
async fn tagged_values_distinguish_tag_order_duplicates_and_key_boundaries() -> Result<()> {
    let store = memory_store().await?;
    let first = store.tagged(&["first", "second"])?;
    let reverse = store.tagged(&["second", "first"])?;
    let duplicate = store.tagged(&["first", "first", "second"])?;
    let second = store.tagged(&["second"])?;
    let forged = format!("item:{}:0", tag_key(&store, "first"));
    first.set("item", &1, TTL).await?;
    reverse.set("item", &2, TTL).await?;
    duplicate.set("item", &3, TTL).await?;
    second.set(&forged, &4, TTL).await?;
    assert_eq!(first.get::<u8>("item").await?, Some(1));
    assert_eq!(reverse.get::<u8>("item").await?, Some(2));
    assert_eq!(duplicate.get::<u8>("item").await?, Some(3));
    assert_eq!(second.get::<u8>(&forged).await?, Some(4));

    store.tagged(&["first"])?.flush().await?;
    assert_eq!(first.get::<u8>("item").await?, None);
    assert_eq!(reverse.get::<u8>("item").await?, None);
    assert_eq!(duplicate.get::<u8>("item").await?, None);
    assert_eq!(second.get::<u8>(&forged).await?, Some(4));
    Ok(())
}

#[tokio::test]
async fn tenant_and_tag_boundaries_cannot_share_a_generation() -> Result<()> {
    let store = memory_store().await?;
    TenantContext::scope(TenantId::new("a:b")?, async {
        store.tagged(&["c"])?.set("item", &1, TTL).await
    })
    .await??;
    TenantContext::scope(TenantId::new("a")?, async {
        let other = store.tagged(&["b:c"])?;
        other.set("item", &2, TTL).await
    })
    .await??;
    TenantContext::scope(TenantId::new("a:b")?, store.invalidate_tags(&["c"])).await??;
    let retained = TenantContext::scope(TenantId::new("a")?, async {
        store.tagged(&["b:c"])?.get::<u8>("item").await
    })
    .await??;
    assert_eq!(retained, Some(2));
    Ok(())
}

#[tokio::test]
async fn concurrent_tag_orders_have_independent_swr_loads() -> Result<()> {
    let store = memory_store().await?;
    let first = store.tagged(&["first", "second"])?;
    let second = store.tagged(&["second", "first"])?;
    let entered = Arc::new(Notify::new());
    let finished = Arc::new(Notify::new());
    let pending = {
        let entered = Arc::clone(&entered);
        let finished = Arc::clone(&finished);
        first.get_stale_while_revalidate("item", "load", TTL, TTL, move || {
            let entered = Arc::clone(&entered);
            let finished = Arc::clone(&finished);
            async move {
                entered.notify_one();
                finished.notified().await;
                Ok(1_u8)
            }
        })
    };
    let (first_result, second_result) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(pending, async {
            entered.notified().await;
            let value = second
                .get_stale_while_revalidate("item", "load", TTL, TTL, || async { Ok(2_u8) })
                .await;
            finished.notify_one();
            value
        })
    })
    .await
    .map_err(Error::internal)?;
    assert_eq!(first_result?, 1);
    assert_eq!(second_result?, 2);
    Ok(())
}

#[tokio::test]
async fn invalidation_releases_new_swr_load_from_the_previous_generation() -> Result<()> {
    let store = memory_store().await?;
    let tagged = store.tagged(&["record"])?;
    let entered = Arc::new(Notify::new());
    let finished = Arc::new(Notify::new());
    let previous = {
        let entered = Arc::clone(&entered);
        let finished = Arc::clone(&finished);
        tagged.get_stale_while_revalidate("item", "load", TTL, TTL, move || {
            let entered = Arc::clone(&entered);
            let finished = Arc::clone(&finished);
            async move {
                entered.notify_one();
                finished.notified().await;
                Ok(1_u8)
            }
        })
    };
    let (previous, next) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(previous, async {
            entered.notified().await;
            let next = async {
                store.invalidate_tags(&["record"]).await?;
                tagged
                    .get_stale_while_revalidate("item", "load", TTL, TTL, || async { Ok(2_u8) })
                    .await
            }
            .await;
            finished.notify_one();
            next
        })
    })
    .await
    .map_err(Error::internal)?;
    assert!(matches!(previous, Err(error) if error.code() == "cache_tag_invalidated"));
    assert_eq!(next?, 2);
    Ok(())
}
