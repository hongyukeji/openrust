//! Argon2id 密码哈希基础能力。

use std::str::FromStr;
use std::sync::Arc;

use crate::config::PasswordConfig;
use crate::config::auth::MIN_MEMORY_KIB;
use crate::core::{Error, Result};
use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use tokio::sync::Semaphore;

const MAX_PASSWORD_BYTES: usize = 1024;
const MAX_MEMORY_KIB: u32 = 262_144;
const MAX_ITERATIONS: u32 = 10;
const MAX_PARALLELISM: u32 = 4;
const MAX_CONCURRENT_OPERATIONS: usize = 4;

/// 编码后的 PHC 密码哈希。此类型有意不实现 `Debug`。
#[derive(Clone, PartialEq, Eq)]
pub struct EncodedPasswordHash(String);

impl EncodedPasswordHash {
    /// 解析已存储的 PHC 字符串；数据格式错误时返回内部错误。
    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let parsed = PasswordHash::new(&value).map_err(Error::internal)?;
        validate_encoded_hash(&parsed)?;
        Ok(Self(value))
    }

    /// 返回用于持久化的编码值。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 密码哈希与校验服务。
#[derive(Clone)]
pub struct Passwords {
    config: PasswordConfig,
    operation_slots: Arc<Semaphore>,
}

impl Passwords {
    /// 校验 Argon2id 参数后创建密码服务，默认最多并行执行两个操作。
    pub fn new(config: PasswordConfig) -> Result<Self> {
        Self::with_max_concurrent_operations(config, 2)
    }

    /// 创建密码服务并设置单实例的最大并发数；超出容量的任务会立即拒绝且不排队。
    pub fn with_max_concurrent_operations(
        config: PasswordConfig,
        max_concurrent_operations: usize,
    ) -> Result<Self> {
        if !(1..=MAX_CONCURRENT_OPERATIONS).contains(&max_concurrent_operations) {
            return Err(Error::Config(crate::config::ConfigError::Invalid(
                "auth.password max_concurrent_operations must be 1..=4".into(),
            )));
        }
        validate_config(&config)?;
        Ok(Self {
            operation_slots: Arc::new(Semaphore::new(max_concurrent_operations)),
            config,
        })
    }

    /// 使用随机盐在 Tokio 阻塞线程池中对非空密码生成哈希。
    pub async fn hash(&self, password: &str) -> Result<EncodedPasswordHash> {
        validate_password(password)?;
        let permit = self.operation_permit()?;
        let password = password.to_owned();
        let config = self.config.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
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

    /// 在 Tokio 阻塞线程池中校验密码；不匹配时返回 `Ok(false)`。
    pub async fn verify(&self, password: &str, encoded: &EncodedPasswordHash) -> Result<bool> {
        validate_password(password)?;
        let permit = self.operation_permit()?;
        let password = password.to_owned();
        let encoded = encoded.0.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
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

    fn operation_permit(&self) -> Result<tokio::sync::OwnedSemaphorePermit> {
        self.operation_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                Error::custom_with_status(
                    http::StatusCode::SERVICE_UNAVAILABLE,
                    "password_capacity_exhausted",
                    "密码处理容量已满，请稍后重试",
                )
            })
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

    #[tokio::test]
    async fn rejects_hash_operations_when_the_service_capacity_is_full()
    -> Result<(), Box<dyn std::error::Error>> {
        let passwords = Passwords::with_max_concurrent_operations(fast_config(), 1)?;
        let encoded = passwords.hash("correct horse").await?;
        let permit = passwords.operation_slots.clone().acquire_owned().await?;
        let cloned = passwords.clone();
        let error = passwords
            .hash("correct horse")
            .await
            .err()
            .ok_or_else(|| std::io::Error::other("hash should be rejected at capacity"))?;
        assert_eq!(error.code(), "password_capacity_exhausted");
        assert_eq!(error.status(), http::StatusCode::SERVICE_UNAVAILABLE);
        let verify_error = cloned
            .verify("correct horse", &encoded)
            .await
            .err()
            .ok_or_else(|| std::io::Error::other("verify should be rejected at capacity"))?;
        assert_eq!(verify_error.code(), "password_capacity_exhausted");
        drop(permit);
        assert!(passwords.hash("correct horse").await.is_ok());
        Ok(())
    }

    #[tokio::test]
    async fn default_capacity_allows_two_operations_and_rejects_the_third()
    -> Result<(), Box<dyn std::error::Error>> {
        let passwords = Passwords::new(fast_config())?;
        let first = passwords.operation_permit()?;
        let second = passwords.operation_permit()?;
        assert_eq!(
            passwords
                .operation_permit()
                .err()
                .ok_or_else(|| std::io::Error::other("third operation should be rejected"))?
                .code(),
            "password_capacity_exhausted"
        );
        drop((first, second));
        assert!(passwords.operation_permit().is_ok());
        Ok(())
    }

    #[test]
    fn rejects_invalid_operation_capacity() {
        assert!(Passwords::with_max_concurrent_operations(fast_config(), 0).is_err());
        assert!(Passwords::with_max_concurrent_operations(fast_config(), 5).is_err());
    }
}
