use std::{sync::Arc, time::Duration};

use crate::{
    cache::CacheManager,
    container::Container,
    contracts::{cache::LockProvider, container::Container as _},
};

use super::{Payload, QueueError, QueueResult, backend};

/// Cache-lock policy returned by a registered job handler.
#[derive(Clone, Debug, Default)]
pub struct UniqueJob {
    pub id: String,
    pub for_seconds: Option<u64>,
    pub via: Option<String>,
    pub until_processing: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct UniqueJobLock {
    key: String,
    owner: String,
    store: String,
    pub(super) until_processing: bool,
}

pub(super) async fn acquire(
    app: &Container,
    payload: &mut Payload,
    policy: UniqueJob,
) -> QueueResult<bool> {
    let manager = cache_manager(app)?;
    let store = match policy.via {
        Some(store) => store,
        None => manager.getDefaultDriver().map_err(backend)?,
    };
    let repository = manager.store(Some(&store)).map_err(backend)?;
    // Frame the name so separators in job names or IDs cannot alias other jobs.
    let key = format!(
        "laravel_unique_job:{}:{}:{}",
        payload.name().len(),
        payload.name(),
        policy.id
    );
    let lease = policy
        .for_seconds
        .filter(|seconds| *seconds != 0)
        .map(Duration::from_secs);
    let mut lock = repository.lock(&key, lease, None).map_err(backend)?;
    if !lock.get().await.map_err(backend)? {
        return Ok(false);
    }
    payload.unique_lock = Some(UniqueJobLock {
        key,
        owner: lock.owner().into(),
        store,
        until_processing: policy.until_processing,
    });
    Ok(true)
}

pub(super) async fn release(app: &Container, payload: &Payload) -> QueueResult<()> {
    let Some(lock) = &payload.unique_lock else {
        return Ok(());
    };
    cache_manager(app)?
        .store(Some(&lock.store))
        .map_err(backend)?
        .restore_lock(&lock.key, &lock.owner)
        .map_err(backend)?
        .release()
        .await
        .map_err(backend)?;
    Ok(())
}

fn cache_manager(app: &Container) -> QueueResult<Arc<CacheManager>> {
    app.make::<CacheManager>().map_err(QueueError::Unavailable)
}
