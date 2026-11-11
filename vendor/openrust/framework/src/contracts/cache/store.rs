use std::num::NonZeroU64;

use crate::cache::{CacheFuture, CacheResult, Value};

use super::LockProvider;

/// Stores cache values using backend-level operations.
pub trait Store: Send + Sync {
    fn get<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>>;
    fn many<'a>(&'a self, keys: &'a [&'a str]) -> CacheFuture<'a, CacheResult<Vec<Option<Value>>>>;
    fn put<'a>(
        &'a self,
        key: &'a str,
        value: &'a Value,
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>>;
    fn put_many<'a>(
        &'a self,
        values: &'a [(String, Value)],
        seconds: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>>;
    fn add<'a>(
        &'a self,
        key: &'a str,
        value: &'a Value,
        seconds: Option<NonZeroU64>,
    ) -> CacheFuture<'a, CacheResult<bool>>;
    fn pull<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>>;
    fn forever<'a>(&'a self, key: &'a str, value: &'a Value) -> CacheFuture<'a, CacheResult<bool>>;
    fn touch<'a>(&'a self, key: &'a str, seconds: NonZeroU64)
    -> CacheFuture<'a, CacheResult<bool>>;
    fn forget<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<bool>>;
    fn increment<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>>;
    fn decrement<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>>;
    fn flush(&self) -> CacheFuture<'_, CacheResult<bool>>;
    fn get_prefix(&self) -> &str;
    fn lock_provider(&self) -> Option<&dyn LockProvider>;
}
