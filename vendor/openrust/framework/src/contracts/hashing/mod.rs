//! Password hashing contract.

#![allow(non_snake_case)]

use crate::hashing::{HashFuture, HashInfo, HashOptions, HashResult};

/// Accepts password bytes without lossy text conversions.
pub trait Hasher: Send + Sync {
    #[doc(hidden)]
    fn as_hash_manager(&self) -> Option<&crate::hashing::HashManager> {
        None
    }

    fn info(&self, hashed_value: &str) -> HashResult<HashInfo>;
    fn make<'a>(
        &'a self,
        value: &'a [u8],
        options: &'a HashOptions,
    ) -> HashFuture<'a, HashResult<String>>;
    fn check<'a>(
        &'a self,
        value: &'a [u8],
        hashed_value: &'a str,
        options: &'a HashOptions,
    ) -> HashFuture<'a, HashResult<bool>>;
    fn verify_configuration(&self, _hashed_value: &str) -> HashResult<bool> {
        Ok(true)
    }
    fn needs_rehash(&self, hashed_value: &str, options: &HashOptions) -> HashResult<bool>;
}
