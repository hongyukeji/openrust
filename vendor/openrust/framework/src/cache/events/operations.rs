use crate::{cache::Value, contracts::events::Event};

macro_rules! cache_event {
    ($name:ident, $event_name:expr $(, $field:ident: $ty:ty)*) => {
        #[allow(non_snake_case, reason = "Matches Laravel's cache event fields")]
        #[derive(Clone, Debug)]
        pub struct $name {
            pub storeName: Option<String>,
            $(pub $field: $ty,)*
        }

        impl $name {
            pub const NAME: &str = $event_name;
        }

        impl Event for $name {
            fn name(&self) -> &str {
                Self::NAME
            }
        }
    };
}

cache_event!(RetrievingKey, "RetrievingKey", key: String, tags: Vec<String>);
cache_event!(RetrievingManyKeys, "RetrievingManyKeys", key: String, tags: Vec<String>, keys: Vec<String>);
cache_event!(CacheHit, "CacheHit", key: String, tags: Vec<String>, value: Value);
cache_event!(CacheMissed, "CacheMissed", key: String, tags: Vec<String>);
cache_event!(WritingKey, "WritingKey", key: String, tags: Vec<String>, value: Value, seconds: Option<u64>);
cache_event!(WritingManyKeys, "WritingManyKeys", key: String, tags: Vec<String>, keys: Vec<String>, values: Vec<Value>, seconds: Option<u64>);
cache_event!(KeyWritten, "KeyWritten", key: String, tags: Vec<String>, value: Value, seconds: Option<u64>);
cache_event!(KeyWriteFailed, "KeyWriteFailed", key: String, tags: Vec<String>, value: Value, seconds: Option<u64>);
cache_event!(ForgettingKey, "ForgettingKey", key: String, tags: Vec<String>);
cache_event!(KeyForgotten, "KeyForgotten", key: String, tags: Vec<String>);
cache_event!(KeyForgetFailed, "KeyForgetFailed", key: String, tags: Vec<String>);
cache_event!(CacheFlushing, "CacheFlushing", tags: Vec<String>);
cache_event!(CacheFlushed, "CacheFlushed", tags: Vec<String>);
cache_event!(CacheFlushFailed, "CacheFlushFailed", tags: Vec<String>);
cache_event!(CacheLocksFlushing, "CacheLocksFlushing");
cache_event!(CacheLocksFlushed, "CacheLocksFlushed");
cache_event!(CacheLocksFlushFailed, "CacheLocksFlushFailed");
cache_event!(CacheClearing, "cache:clearing", tags: Vec<String>);
cache_event!(CacheCleared, "cache:cleared", tags: Vec<String>);
