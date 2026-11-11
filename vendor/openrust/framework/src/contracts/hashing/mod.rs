//! Password hashing contract.

#![allow(non_snake_case)]

use crate::hashing::{HashFuture, HashInfo, HashOptions, HashResult};

/// Accepts password bytes without lossy text conversions.
pub trait Hasher: Send + Sync {
    fn info(&self, hashedValue: &str) -> HashResult<HashInfo>;
    fn make<'a>(
        &'a self,
        value: &'a [u8],
        options: &'a HashOptions,
    ) -> HashFuture<'a, HashResult<String>>;
    fn check<'a>(
        &'a self,
        value: &'a [u8],
        hashedValue: &'a str,
        options: &'a HashOptions,
    ) -> HashFuture<'a, HashResult<bool>>;
    fn verifyConfiguration(&self, _hashedValue: &str) -> HashResult<bool> {
        Ok(true)
    }
    fn needsRehash(&self, hashedValue: &str, options: &HashOptions) -> HashResult<bool>;
}
