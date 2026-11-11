use std::{sync::Arc, time::SystemTime};

use chrono::{DateTime, Utc};

use crate::{
    cache::{CacheResult, Repository, Ttl, Value},
    console::ConsoleFuture,
    contracts::cache::Repository as _,
};

use super::{Event, SchedulingMutex};

/// Elects one runner per event and UTC minute using atomic cache insertion.
pub struct CacheSchedulingMutex(Arc<Repository>);

impl CacheSchedulingMutex {
    pub fn new(store: Arc<Repository>) -> Self {
        Self(store)
    }
}

fn key(event: &Event, time: SystemTime) -> String {
    let slot = DateTime::<Utc>::from(time).timestamp().div_euclid(60);
    format!("{}:slot:{slot}", event.mutex_key())
}

impl SchedulingMutex for CacheSchedulingMutex {
    fn create<'a>(
        &'a self,
        event: &'a Event,
        time: SystemTime,
    ) -> ConsoleFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            self.0
                .add(
                    &key(event, time),
                    &Value::from(serde_json::json!(true)),
                    Some(Ttl::Seconds(3600)),
                )
                .await
        })
    }

    fn exists<'a>(
        &'a self,
        event: &'a Event,
        time: SystemTime,
    ) -> ConsoleFuture<'a, CacheResult<bool>> {
        Box::pin(async move { Ok(self.0.get(&key(event, time)).await?.is_some()) })
    }
}
