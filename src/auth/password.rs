//! Argon2id password hashing primitives.

use std::str::FromStr;

use crate::config::PasswordConfig;
use crate::config::auth::MIN_MEMORY_KIB;
use crate::core::{Error, Result};
use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};

const MAX_PASSWORD_BYTES: usize = 1024;
const MAX_MEMORY_KIB: u32 = 262_144;
const MAX_ITERATIONS: u32 = 10;
const MAX_PARALLELISM: u32 = 4;

/// Encoded PHC password hash. It deliberately does not implement `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct EncodedPasswordHash(String);

impl EncodedPasswordHash {
    /// Parses a stored PHC string, returning an internal error for malformed data.
    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let parsed = PasswordHash::new(&value).map_err(Error::internal)?;
        validate_encoded_hash(&parsed)?;
        Ok(Self(value))
    }

    /// Returns the encoded value for persistence.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Password hashing and verification service.
#[derive(Clone)]
pub struct Passwords {
    config: PasswordConfig,
}

impl Passwords {
    /// Creates a service after validating its resource-bounded Argon2id parameters.
    pub fn new(config: PasswordConfig) -> Result<Self> {
        validate_config(&config)?;
        Ok(Self { config })
    }

    /// Hashes a non-empty password on Tokio's blocking pool using a random salt.
    pub async fn hash(&self, password: &str) -> Result<EncodedPasswordHash> {
        validate_password(password)?;
        let password = password.to_owned();
        let config = self.config.clone();
        tokio::task::spawn_blocking(move || {
            let params = argon_params(&config)?;
            let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
            let encoded = argon
                .hash_password(password.as_bytes())
                .map_err(Error::internal)?
                .to_string();
            Ok(EncodedPasswordHash(encoded))
        })
        .await
        .map_err(Error::internal)?
    }

    /// Verifies a password on Tokio's blocking pool; a mismatch returns `Ok(false)`.
    pub async fn verify(&self, password: &str, encoded: &EncodedPasswordHash) -> Result<bool> {
        validate_password(password)?;
        let password = password.to_owned();
        let encoded = encoded.0.clone();
        tokio::task::spawn_blocking(move || {
            let parsed = PasswordHash::new(&encoded).map_err(Error::internal)?;
            validate_encoded_hash(&parsed)?;
            let argon = Argon2::default();
            match argon.verify_password(password.as_bytes(), &parsed) {
                Ok(()) => Ok(true),
                Err(argon2::password_hash::Error::PasswordInvalid) => Ok(false),
                Err(error) => Err(Error::internal(error)),
            }
        })
        .await
        .map_err(Error::internal)?
    }
}

fn validate_encoded_hash(hash: &PasswordHash) -> Result<()> {
    if hash.algorithm.as_str() != "argon2id" || hash.version != Some(19) {
        return Err(Error::internal(StoredHashParameters));
    }
    let params = Params::try_from(hash).map_err(Error::internal)?;
    if !(MIN_MEMORY_KIB..=MAX_MEMORY_KIB).contains(&params.m_cost())
        || !(1..=MAX_ITERATIONS).contains(&params.t_cost())
        || !(1..=MAX_PARALLELISM).contains(&params.p_cost())
        || params.m_cost() < 8 * params.p_cost()
    {
        return Err(Error::internal(StoredHashParameters));
    }
    Ok(())
}

fn validate_config(config: &PasswordConfig) -> Result<()> {
    if !(MIN_MEMORY_KIB..=MAX_MEMORY_KIB).contains(&config.memory_kib)
        || !(1..=MAX_ITERATIONS).contains(&config.iterations)
        || !(1..=MAX_PARALLELISM).contains(&config.parallelism)
        || config.memory_kib < 8 * config.parallelism
    {
        return Err(Error::Config(crate::config::ConfigError::Invalid(
            "auth.password memory_kib must be 19456..=262144, iterations 1..=10, and parallelism 1..=4".into(),
        )));
    }
    Ok(())
}

fn argon_params(config: &PasswordConfig) -> Result<Params> {
    Params::new(
        config.memory_kib,
        config.iterations,
        config.parallelism,
        None,
    )
    .map_err(Error::internal)
}

fn validate_password(password: &str) -> Result<()> {
    if password.is_empty() || password.len() > MAX_PASSWORD_BYTES {
        return Err(Error::BadRequest(
            "password must contain between 1 and 1024 UTF-8 bytes".into(),
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct StoredHashParameters;

impl std::fmt::Display for StoredHashParameters {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("stored password hash uses unsupported or unsafe parameters")
    }
}

impl std::error::Error for StoredHashParameters {}

impl FromStr for EncodedPasswordHash {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        Self::parse(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{EncodedPasswordHash, PasswordConfig, Passwords};

    fn fast_config() -> PasswordConfig {
        PasswordConfig {
            memory_kib: 19_456,
            iterations: 1,
            parallelism: 1,
        }
    }

    #[tokio::test]
    async fn hashes_use_distinct_random_salts_and_verify_passwords()
    -> Result<(), Box<dyn std::error::Error>> {
        let passwords = Passwords::new(fast_config())?;
        let first = passwords.hash("correct horse").await?;
        let second = passwords.hash("correct horse").await?;
        assert_ne!(first.as_str(), second.as_str());
        assert!(passwords.verify("correct horse", &first).await?);
        assert!(!passwords.verify("wrong horse", &first).await?);
        Ok(())
    }

    #[tokio::test]
    async fn rejects_stored_hashes_with_unsafe_resource_parameters()
    -> Result<(), Box<dyn std::error::Error>> {
        let passwords = Passwords::new(fast_config())?;
        let hash = passwords.hash("correct horse").await?;
        let oversized = hash.as_str().replace("m=19456", "m=262145");
        assert!(EncodedPasswordHash::parse(oversized).is_err());
        Ok(())
    }

    #[test]
    fn rejects_out_of_range_parameters_and_malformed_hashes() {
        assert!(
            Passwords::new(PasswordConfig {
                memory_kib: 1024,
                ..fast_config()
            })
            .is_err()
        );
        assert!(EncodedPasswordHash::parse("not-a-phc-hash").is_err());
    }

    #[tokio::test]
    async fn rejects_empty_and_oversized_passwords() -> Result<(), Box<dyn std::error::Error>> {
        let passwords = Passwords::new(fast_config())?;
        assert!(passwords.hash("").await.is_err());
        assert!(passwords.hash(&"x".repeat(1025)).await.is_err());
        Ok(())
    }
}
