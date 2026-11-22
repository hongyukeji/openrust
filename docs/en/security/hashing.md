# Hashing

## Introduction

The `Hash` facade creates and verifies password hashes. It supports bcrypt, Argon2i, and Argon2id. Password inputs are byte strings. bcrypt follows PHP’s 72-byte behavior. Set `BCRYPT_LIMIT=72` to reject longer inputs when creating new hashes.

## Configuration

Select `bcrypt`, `argon`, or `argon2id` with `HASH_DRIVER`. The default is bcrypt. Set `BCRYPT_ROUNDS` or the Argon parameters `ARGON_MEMORY` in KiB, `ARGON_TIME`, and `ARGON_THREADS` in `config/hashing.rs`. Use `Hash::driver(Some("argon2id"))` to choose a named driver.

### Custom Drivers {#custom-hash-drivers}

Register an application-specific driver with `Hash::extend` or `HashManager::extend` during provider boot. Select its name with `HASH_DRIVER`, or pass it explicitly to `driver`. The factory receives the manager and returns an `Arc<dyn Hasher>`. It can access application bindings through `getContainer()`.

```rust
use std::sync::Arc;

use openrust::hashing::HashManager;

pub fn register(manager: &HashManager) {
    manager.extend("project", |manager| {
        Ok(Arc::new(manager.createArgon2idDriver()?))
    });
}
```

The manager creates the driver on first use and caches it by name. Registering a replacement leaves the cached instance in use until `forgetDrivers()`. A failed factory is retried on the next lookup. Recursive or concurrent creation of an in-flight driver returns `DriverBusy`. Built-in drivers created through the manager share the application’s hashing limits.

## Basic Usage

Call `Hash::make` to create a hash and `Hash::check` to verify a password. `HashOptions::new()` uses the configured costs. Set `rounds` for a bcrypt operation or `memory`, `time`, and `threads` for Argon. Use `needsRehash` after a successful verification to decide whether to save a replacement hash.

```rust
use openrust::{
    hashing::{HashOptions, HashResult},
    support::facades::Hash,
};

pub async fn refresh_password(password: &[u8], stored: &str) -> HashResult<Option<String>> {
    let options = HashOptions::new();
    if Hash::check(password, stored, &options).await? && Hash::needsRehash(stored, &options)? {
        return Hash::make(password, &options).await.map(Some);
    }
    Ok(None)
}
```

## Hash Algorithm Verification

`HASH_VERIFY` is enabled by default in the application. A stored hash using another algorithm returns `HashError::AlgorithmMismatch`. For a migration, `HASH_VERIFY=false` permits verification using any of the supported stored algorithms. New hashes use the currently configured driver.

OpenRust runs password operations on blocking workers. The application defaults to two concurrent operations and returns `CapacityExhausted` when both are busy. A stored hash or operation may also return `ExecutionLimit` if its costs exceed the application budget. The default maxima are bcrypt cost 16 and Argon memory 256 MiB, time cost 10, and 4 lanes. Configure these limits with `hashing.max_concurrent_operations` and `hashing.execution_limits`.


## Changing instance defaults

On a mutable concrete hasher, call `BcryptHasher::setRounds(rounds)` or `ArgonHasher` / `Argon2IdHasher` methods `setMemory(kib)`, `setTime(iterations)`, and `setThreads(threads)`. These chainable setters change that instance’s defaults. Options passed to an individual operation take precedence; resource limits are checked when the operation runs.
