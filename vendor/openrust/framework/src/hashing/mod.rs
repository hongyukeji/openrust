//! Password hashing and named hash drivers.

mod argon2id_hasher;
mod argon_hasher;
mod bcrypt_hasher;
mod driver;
mod facade;
mod hash_manager;
mod hashing_service_provider;
mod legacy_bcrypt;
mod parameters;

use std::{future::Future, pin::Pin};

pub use argon_hasher::ArgonHasher;
pub use argon2id_hasher::Argon2IdHasher;
pub use bcrypt_hasher::BcryptHasher;
pub use driver::{ArgonConfig, BcryptConfig, ExecutionLimits};
pub use facade::Hash;
pub use hash_manager::HashManager;
pub use hashing_service_provider::HashingServiceProvider;

pub type HashFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type HashResult<T> = Result<T, HashError>;
pub type HashOptions = serde_json::Map<String, serde_json::Value>;

/// Algorithm identifier and parameters obtained from an encoded hash.
#[allow(non_snake_case)]
#[derive(Debug, serde::Serialize)]
pub struct HashInfo {
    pub algo: Option<String>,
    pub algoName: String,
    pub options: HashOptions,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum HashError {
    UnknownDriver,
    Unavailable(crate::contracts::container::BindingResolutionError),
    ExecutionLimit,
    CapacityExhausted,
    AlgorithmMismatch,
    InvalidOptions,
    InvalidInput,
    HashingFailed,
}

impl std::fmt::Display for HashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::UnknownDriver => "Unknown password hashing driver.",
            Self::Unavailable(_) => "The application hash manager is unavailable.",
            Self::AlgorithmMismatch => "The stored hash does not use the configured algorithm.",
            Self::InvalidOptions => "Invalid password hashing options.",
            Self::InvalidInput => "Invalid password input for the configured hash driver.",
            Self::ExecutionLimit => "Password hashing parameters exceed the execution budget.",
            Self::CapacityExhausted => "Password hashing capacity is exhausted.",
            Self::HashingFailed => "The password hashing operation failed.",
        })
    }
}

impl std::error::Error for HashError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unavailable(error) => Some(error),
            _ => None,
        }
    }
}
