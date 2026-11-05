//! 成熟密码库驱动；参数解析与资源容量独立于认证业务。

use super::{ArgonConfig, BcryptConfig, ExecutionLimits, HashInfo, HashOptions, Hasher, failure};
use crate::core::{BoxFuture, Result};
use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
#[cfg(test)]
use std::future::Future;
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub(super) struct OperationPool {
    semaphore: Arc<Semaphore>,
    limits: ExecutionLimits,
}
impl OperationPool {
    pub(super) fn new(capacity: usize) -> Result<Self> {
        if !(1..=4).contains(&capacity) {
            return Err(failure(
                "hashing_capacity_invalid",
                "哈希并发容量必须为1至4",
            ));
        }
        Ok(Self {
            semaphore: Arc::new(Semaphore::new(capacity)),
            limits: ExecutionLimits::default(),
        })
    }
    pub(super) fn limited(&self, limits: ExecutionLimits) -> Self {
        Self {
            semaphore: Arc::clone(&self.semaphore),
            limits,
        }
    }
    fn reserve(&self) -> Result<tokio::sync::OwnedSemaphorePermit> {
        self.semaphore.clone().try_acquire_owned().map_err(|_| {
            crate::core::Error::custom_with_status(
                http::StatusCode::SERVICE_UNAVAILABLE,
                "password_capacity_exhausted",
                "密码处理容量已满，请稍后重试",
            )
        })
    }
    #[cfg(test)]
    pub(super) fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> impl Future<Output = Result<T>> + Send {
        let permit = self.reserve();
        async move { Self::run_reserved(permit?, work).await }
    }
    async fn run_reserved<T: Send + 'static>(
        permit: tokio::sync::OwnedSemaphorePermit,
        work: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> Result<T> {
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work()
        })
        .await
        .map_err(|_| failure("hashing_worker_failed", "哈希任务执行失败"))?
    }
}
impl ExecutionLimits {
    pub(super) fn bcrypt(&self, rounds: u32) -> Result<()> {
        if rounds > self.max_bcrypt_rounds {
            return Err(execution_limit());
        }
        Ok(())
    }
    pub(super) fn argon(&self, params: &Params) -> Result<()> {
        if params.m_cost() > self.max_argon_memory
            || params.t_cost() > self.max_argon_time
            || params.p_cost() > self.max_argon_threads
        {
            return Err(execution_limit());
        }
        Ok(())
    }
}
fn execution_limit() -> crate::core::Error {
    failure(
        "hashing_execution_limit_exceeded",
        "哈希计算参数超出当前执行预算",
    )
}
fn unsigned_option(options: &HashOptions, key: &str, fallback: u32) -> Result<u32> {
    u32::try_from(super::parameters::option(options, key, fallback))
        .map_err(|_| failure("hashing_options_invalid", "哈希计算参数无效"))
}
fn bcrypt_rounds(config: &BcryptConfig, options: &HashOptions) -> Result<u32> {
    let rounds = unsigned_option(options, "rounds", config.rounds)?;
    if !(4..=31).contains(&rounds) {
        return Err(failure("hashing_options_invalid", "bcrypt成本超出支持范围"));
    }
    Ok(rounds)
}
fn argon_params(config: &ArgonConfig, options: &HashOptions) -> Result<Params> {
    Params::new(
        unsigned_option(options, "memory", config.memory)?,
        unsigned_option(options, "time", config.time)?,
        unsigned_option(options, "threads", config.threads)?,
        None,
    )
    .map_err(|_| failure("hashing_options_invalid", "Argon参数无效"))
}

pub(super) fn inspect(encoded: &str) -> HashInfo {
    let mut info = HashInfo {
        algo: None,
        algo_name: "unknown".into(),
        options: HashOptions::new(),
    };
    if encoded.starts_with("$2y$") && encoded.len() == 60 {
        let cost = super::parameters::bcrypt_cost(encoded);
        info.algo = Some("2y".into());
        info.algo_name = "bcrypt".into();
        info.options.insert("cost".into(), cost.into());
        return info;
    }
    let algorithm = if encoded.starts_with("$argon2id$") {
        Some("argon2id")
    } else if encoded.starts_with("$argon2i$") {
        Some("argon2i")
    } else {
        None
    };
    if let Some(algorithm) = algorithm {
        info.algo = Some(algorithm.into());
        info.algo_name = algorithm.into();
        let [memory, time, threads] = super::parameters::argon_parameters(encoded, [65_536, 4, 1]);
        info.options.insert("memory_cost".into(), memory.into());
        info.options.insert("time_cost".into(), time.into());
        info.options.insert("threads".into(), threads.into());
    }
    info
}
fn verify_stored(value: &str, encoded: &str) -> bool {
    if encoded.parse::<bcrypt::HashParts>().is_ok() {
        // PHP bcrypt校验按首个NUL截断；make路径则拒绝任何NUL。
        let bytes = value
            .as_bytes()
            .split(|byte| *byte == 0)
            .next()
            .unwrap_or_default();
        if encoded.starts_with("$2x$") {
            return super::legacy_bcrypt::verify(bytes, encoded);
        }
        return bcrypt::verify(bytes, encoded).is_ok_and(|matched| matched);
    }
    let Ok(hash) = PasswordHash::new(encoded) else {
        return false;
    };
    if !matches!(hash.algorithm.as_str(), "argon2i" | "argon2id") || hash.version != Some(19) {
        return false;
    }
    let ordered = encoded.split('$').nth(3).is_some_and(|parameters| {
        let mut parts = parameters.split(',');
        parts.next().is_some_and(|part| part.starts_with("m="))
            && parts.next().is_some_and(|part| part.starts_with("t="))
            && parts.next().is_some_and(|part| part.starts_with("p="))
            && parts.next().is_none()
    });
    ordered
        && Argon2::default()
            .verify_password(value.as_bytes(), &hash)
            .is_ok()
}
fn valid_bcrypt_cost(encoded: &str) -> Option<u32> {
    let [first, second] = encoded.as_bytes().get(4..6)? else {
        return None;
    };
    if !first.is_ascii_digit() || !second.is_ascii_digit() {
        return None;
    }
    let rounds = u32::from(*first - b'0') * 10 + u32::from(*second - b'0');
    (4..=31).contains(&rounds).then_some(rounds)
}
fn validate_stored_budget(encoded: &str, limits: &ExecutionLimits) -> Result<bool> {
    if encoded.starts_with("$2") {
        let Some(rounds) = valid_bcrypt_cost(encoded) else {
            return Ok(false);
        };
        if encoded.parse::<bcrypt::HashParts>().is_err() {
            return Ok(false);
        }
        limits.bcrypt(rounds)?;
        return Ok(true);
    }
    let Ok(hash) = PasswordHash::new(encoded) else {
        return Ok(false);
    };
    if !matches!(hash.algorithm.as_str(), "argon2i" | "argon2id") || hash.version != Some(19) {
        return Ok(false);
    }
    let Ok(params) = Params::try_from(&hash) else {
        return Ok(false);
    };
    limits.argon(&params)?;
    Ok(true)
}
fn verification<'a>(
    value: &'a str,
    encoded: Option<&'a str>,
    verify: bool,
    algorithm: &'static str,
    pool: &'a OperationPool,
) -> BoxFuture<'a, Result<bool>> {
    Box::pin(async move {
        let Some(encoded) = encoded.filter(|value| !value.is_empty()) else {
            return Ok(false);
        };
        if verify && inspect(encoded).algo_name != algorithm {
            return Err(failure(
                "hashing_algorithm_mismatch",
                "已存哈希算法与当前驱动不匹配",
            ));
        }
        if !validate_stored_budget(encoded, &pool.limits)? {
            return Ok(false);
        }
        let permit = pool.reserve()?;
        let value = value.to_owned();
        let encoded = encoded.to_owned();
        OperationPool::run_reserved(permit, move || Ok(verify_stored(&value, &encoded))).await
    })
}
fn configuration_matches(info: &HashInfo, algorithm: &str, limits: &[(&str, u32)]) -> bool {
    info.algo_name == algorithm
        && limits.iter().all(|(name, max)| {
            info.options
                .get(*name)
                .and_then(serde_json::Value::as_i64)
                .is_some_and(|value| value <= i64::from(*max))
        })
}

/// bcrypt驱动；裸默认成本12、verify=false，不额外限制输入字节数。
#[derive(Clone)]
pub struct BcryptHasher {
    config: BcryptConfig,
    pool: OperationPool,
}
impl BcryptHasher {
    /// 创建裸驱动；默认最多同时执行两个密码操作。
    pub fn new(config: BcryptConfig) -> Result<Self> {
        Self::with_max_concurrent_operations(config, 2)
    }
    /// 指定密码操作容量，范围1至4。
    pub fn with_max_concurrent_operations(config: BcryptConfig, capacity: usize) -> Result<Self> {
        Ok(Self::with_pool(config, OperationPool::new(capacity)?))
    }
    /// 创建使用显式计算预算的裸驱动。
    pub fn with_execution_limits(config: BcryptConfig, limits: ExecutionLimits) -> Result<Self> {
        Ok(Self::with_pool(
            config,
            OperationPool::new(2)?.limited(limits),
        ))
    }
    pub(super) fn with_pool(config: BcryptConfig, pool: OperationPool) -> Self {
        Self { config, pool }
    }
}
fn bcrypt_accepts_exact_input(value: &str) -> bool {
    value.len() <= 72 && !value.as_bytes().contains(&0)
}

impl Hasher for BcryptHasher {
    fn accepts_exact_input(&self, value: &str) -> Result<bool> {
        Ok(bcrypt_accepts_exact_input(value))
    }
    fn info(&self, encoded: &str) -> Result<HashInfo> {
        Ok(inspect(encoded))
    }
    fn make<'a>(
        &'a self,
        value: &'a str,
        options: &'a HashOptions,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            if self
                .config
                .limit
                .is_some_and(|limit| limit > 0 && value.len() > limit)
            {
                return Err(failure(
                    "hashing_value_too_long",
                    "待哈希值超过配置字节上限",
                ));
            }
            if value.as_bytes().contains(&0) {
                return Err(failure("hashing_failed", "bcrypt无法处理此输入"));
            }
            let rounds = bcrypt_rounds(&self.config, options)?;
            self.pool.limits.bcrypt(rounds)?;
            let permit = self.pool.reserve()?;
            let value = value.to_owned();
            OperationPool::run_reserved(permit, move || {
                bcrypt::hash_with_result(value.as_bytes(), rounds)
                    .map(|hash| hash.format_for_version(bcrypt::Version::TwoY))
                    .map_err(|_| failure("hashing_failed", "bcrypt哈希失败"))
            })
            .await
        })
    }
    fn check<'a>(
        &'a self,
        value: &'a str,
        encoded: Option<&'a str>,
        _options: &'a HashOptions,
    ) -> BoxFuture<'a, Result<bool>> {
        verification(value, encoded, self.config.verify, "bcrypt", &self.pool)
    }
    fn needs_rehash(&self, encoded: &str, options: &HashOptions) -> Result<bool> {
        if inspect(encoded).algo_name != "bcrypt" {
            return Ok(true);
        }
        Ok(super::parameters::bcrypt_cost(encoded)
            != super::parameters::option(options, "rounds", self.config.rounds))
    }
    fn verify_configuration(&self, encoded: &str) -> Result<bool> {
        Ok(configuration_matches(
            &inspect(encoded),
            "bcrypt",
            &[("cost", self.config.rounds)],
        ))
    }
}

#[derive(Clone)]
struct ArgonDriver {
    config: ArgonConfig,
    algorithm: Algorithm,
    pool: OperationPool,
}
impl ArgonDriver {
    fn accepts_exact_input(&self, value: &str) -> bool {
        self.config.verify || bcrypt_accepts_exact_input(value)
    }
    fn new(config: ArgonConfig, algorithm: Algorithm, pool: OperationPool) -> Self {
        Self {
            config,
            algorithm,
            pool,
        }
    }
    fn name(&self) -> &'static str {
        match self.algorithm {
            Algorithm::Argon2i => "argon2i",
            _ => "argon2id",
        }
    }
    fn make<'a>(
        &'a self,
        value: &'a str,
        options: &'a HashOptions,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let params = argon_params(&self.config, options)?;
            self.pool.limits.argon(&params)?;
            let permit = self.pool.reserve()?;
            let algorithm = self.algorithm;
            let value = value.to_owned();
            OperationPool::run_reserved(permit, move || {
                Argon2::new(algorithm, Version::V0x13, params)
                    .hash_password(value.as_bytes())
                    .map(|hash| hash.to_string())
                    .map_err(|_| failure("hashing_failed", "Argon哈希失败"))
            })
            .await
        })
    }
    fn needs_rehash(&self, encoded: &str, options: &HashOptions) -> bool {
        if inspect(encoded).algo_name != self.name() {
            return true;
        }
        let stored = super::parameters::argon_parameters(encoded, [0, 0, 0]);
        let target = [
            super::parameters::option(options, "memory", self.config.memory),
            super::parameters::option(options, "time", self.config.time),
            super::parameters::option(options, "threads", self.config.threads),
        ];
        stored != target
    }
    fn verify_configuration(&self, encoded: &str) -> bool {
        configuration_matches(
            &inspect(encoded),
            self.name(),
            &[
                ("memory_cost", self.config.memory),
                ("time_cost", self.config.time),
                ("threads", self.config.threads),
            ],
        )
    }
}

macro_rules! argon_hasher {
    ($name:ident, $algorithm:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone)]
        pub struct $name(ArgonDriver);
        impl $name {
            /// 创建裸驱动，默认最多同时处理两个密码操作。
            pub fn new(config: ArgonConfig) -> Result<Self> {
                Self::with_max_concurrent_operations(config, 2)
            }
            /// 指定密码操作容量，范围1至4。
            pub fn with_max_concurrent_operations(
                config: ArgonConfig,
                capacity: usize,
            ) -> Result<Self> {
                Ok(Self::with_pool(config, OperationPool::new(capacity)?))
            }
            /// 创建使用显式计算预算的裸驱动。
            pub fn with_execution_limits(
                config: ArgonConfig,
                limits: ExecutionLimits,
            ) -> Result<Self> {
                Ok(Self::with_pool(
                    config,
                    OperationPool::new(2)?.limited(limits),
                ))
            }
            pub(super) fn with_pool(config: ArgonConfig, pool: OperationPool) -> Self {
                Self(ArgonDriver::new(config, Algorithm::$algorithm, pool))
            }
        }
        impl Hasher for $name {
            fn accepts_exact_input(&self, value: &str) -> Result<bool> {
                Ok(self.0.accepts_exact_input(value))
            }
            fn info(&self, encoded: &str) -> Result<HashInfo> {
                Ok(inspect(encoded))
            }
            fn make<'a>(
                &'a self,
                value: &'a str,
                options: &'a HashOptions,
            ) -> BoxFuture<'a, Result<String>> {
                self.0.make(value, options)
            }
            fn check<'a>(
                &'a self,
                value: &'a str,
                encoded: Option<&'a str>,
                _options: &'a HashOptions,
            ) -> BoxFuture<'a, Result<bool>> {
                verification(
                    value,
                    encoded,
                    self.0.config.verify,
                    self.0.name(),
                    &self.0.pool,
                )
            }
            fn needs_rehash(&self, encoded: &str, options: &HashOptions) -> Result<bool> {
                Ok(self.0.needs_rehash(encoded, options))
            }
            fn verify_configuration(&self, encoded: &str) -> Result<bool> {
                Ok(self.0.verify_configuration(encoded))
            }
        }
    };
}
argon_hasher!(
    ArgonHasher,
    Argon2i,
    "Argon2i裸驱动，默认1024KiB、2次迭代、2并行及verify=false。"
);
argon_hasher!(
    Argon2IdHasher,
    Argon2id,
    "Argon2id裸驱动，与Argon2i共用参数配置但严格区分算法。"
);
