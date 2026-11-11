use std::sync::Arc;

use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHasher as _, PasswordVerifier as _, phc::PasswordHash},
};
use serde::Deserialize;
use tokio::sync::Semaphore;
use zeroize::Zeroizing;

use crate::contracts::hashing::Hasher;

use super::{HashError, HashFuture, HashInfo, HashOptions, HashResult, legacy_bcrypt, parameters};

#[derive(Clone, Copy, Deserialize)]
#[serde(default)]
pub struct BcryptConfig {
    pub rounds: u32,
    pub verify: bool,
    pub limit: Option<usize>,
}

impl Default for BcryptConfig {
    fn default() -> Self {
        Self {
            rounds: 12,
            verify: false,
            limit: None,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(default)]
pub struct ArgonConfig {
    pub memory: u32,
    pub time: u32,
    pub threads: u32,
    pub verify: bool,
}

impl Default for ArgonConfig {
    fn default() -> Self {
        Self {
            memory: 1024,
            time: 2,
            threads: 2,
            verify: false,
        }
    }
}

/// Upper bounds for work requested by options or an encoded password hash.
#[derive(Clone, Copy, Deserialize)]
#[serde(default)]
pub struct ExecutionLimits {
    pub max_argon_memory: u32,
    pub max_argon_time: u32,
    pub max_argon_threads: u32,
    pub max_bcrypt_rounds: u32,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_argon_memory: 262_144,
            max_argon_time: 10,
            max_argon_threads: 4,
            max_bcrypt_rounds: 16,
        }
    }
}

impl ExecutionLimits {
    fn bcrypt(self, rounds: u32) -> HashResult<()> {
        if rounds > self.max_bcrypt_rounds {
            return Err(HashError::ExecutionLimit);
        }
        Ok(())
    }

    fn argon(self, params: &Params) -> HashResult<()> {
        if params.m_cost() > self.max_argon_memory
            || params.t_cost() > self.max_argon_time
            || params.p_cost() > self.max_argon_threads
        {
            return Err(HashError::ExecutionLimit);
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct OperationPool {
    semaphore: Arc<Semaphore>,
    limits: ExecutionLimits,
}

impl OperationPool {
    pub(super) fn new(capacity: usize, limits: ExecutionLimits) -> HashResult<Self> {
        if !(1..=Semaphore::MAX_PERMITS).contains(&capacity) {
            return Err(HashError::InvalidOptions);
        }
        Ok(Self {
            semaphore: Arc::new(Semaphore::new(capacity)),
            limits,
        })
    }

    pub(super) fn for_driver() -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(2)),
            limits: ExecutionLimits::default(),
        }
    }

    async fn run<T: Send + 'static>(
        &self,
        value: &[u8],
        work: impl FnOnce(&[u8]) -> HashResult<T> + Send + 'static,
    ) -> HashResult<T> {
        let permit = self
            .semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|_| HashError::CapacityExhausted)?;
        let value = Zeroizing::new(value.to_vec());
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work(&value)
        })
        .await
        .map_err(|_| HashError::HashingFailed)?
    }
}

#[derive(Clone, Copy)]
pub(super) enum Settings {
    Bcrypt(BcryptConfig),
    Argon(ArgonConfig, Algorithm),
}

impl Settings {
    fn algorithm(self) -> &'static str {
        match self {
            Self::Bcrypt(_) => "bcrypt",
            Self::Argon(_, Algorithm::Argon2i) => "argon2i",
            Self::Argon(_, _) => "argon2id",
        }
    }

    fn verify(self) -> bool {
        match self {
            Self::Bcrypt(config) => config.verify,
            Self::Argon(config, _) => config.verify,
        }
    }
}

pub(super) struct Driver {
    settings: Settings,
    pool: OperationPool,
}

impl Driver {
    pub(super) fn new(settings: Settings, pool: OperationPool) -> Self {
        Self { settings, pool }
    }

    fn target(settings: Settings, options: &HashOptions) -> HashResult<[u32; 3]> {
        match settings {
            Settings::Bcrypt(config) => Ok([option(options, "rounds", config.rounds)?, 0, 0]),
            Settings::Argon(config, _) => Ok([
                option(options, "memory", config.memory)?,
                option(options, "time", config.time)?,
                option(options, "threads", config.threads)?,
            ]),
        }
    }

    fn stored(&self, encoded: &str) -> [i64; 3] {
        match self.settings {
            Settings::Bcrypt(_) => [parameters::bcrypt_cost(encoded), 0, 0],
            Settings::Argon(..) => parameters::argon_parameters(encoded, [0; 3]),
        }
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's hash API")]
impl Hasher for Driver {
    fn info(&self, hashed: &str) -> HashResult<HashInfo> {
        Ok(inspect(hashed))
    }

    fn make<'a>(
        &'a self,
        value: &'a [u8],
        options: &'a HashOptions,
    ) -> HashFuture<'a, HashResult<String>> {
        Box::pin(async move {
            let [first, second, third] = Self::target(self.settings, options)?;
            match self.settings {
                Settings::Bcrypt(config) => {
                    if value.contains(&0)
                        || config
                            .limit
                            .is_some_and(|limit| limit > 0 && value.len() > limit)
                    {
                        return Err(HashError::InvalidInput);
                    }
                    if !(4..=31).contains(&first) {
                        return Err(HashError::InvalidOptions);
                    }
                    self.pool.limits.bcrypt(first)?;
                    self.pool
                        .run(value, move |value| {
                            bcrypt::hash_with_result(value, first)
                                .map(|hash| hash.format_for_version(bcrypt::Version::TwoY))
                                .map_err(|_| HashError::HashingFailed)
                        })
                        .await
                }
                Settings::Argon(_, algorithm) => {
                    let params = Params::new(first, second, third, None)
                        .map_err(|_| HashError::InvalidOptions)?;
                    self.pool.limits.argon(&params)?;
                    self.pool
                        .run(value, move |value| {
                            Argon2::new(algorithm, Version::V0x13, params)
                                .hash_password(value)
                                .map(|hash| hash.to_string())
                                .map_err(|_| HashError::HashingFailed)
                        })
                        .await
                }
            }
        })
    }

    fn check<'a>(
        &'a self,
        value: &'a [u8],
        hashed: &'a str,
        _: &'a HashOptions,
    ) -> HashFuture<'a, HashResult<bool>> {
        Box::pin(async move {
            if hashed.is_empty() {
                return Ok(false);
            }
            if self.settings.verify() && inspect(hashed).algoName != self.settings.algorithm() {
                return Err(HashError::AlgorithmMismatch);
            }
            let limits = self.pool.limits;
            if hashed.starts_with("$2") {
                let Ok(parts) = hashed.parse::<bcrypt::HashParts>() else {
                    return Ok(false);
                };
                let rounds = parts.get_cost();
                if !(4..=31).contains(&rounds) {
                    return Ok(false);
                }
                limits.bcrypt(rounds)?;
                let hashed = hashed.to_owned();
                let value = value.split(|byte| *byte == 0).next().unwrap_or_default();
                return self
                    .pool
                    .run(value, move |value| {
                        Ok(if hashed.starts_with("$2x$") {
                            legacy_bcrypt::verify(value, &hashed)
                        } else {
                            bcrypt::verify(value, &hashed).unwrap_or(false)
                        })
                    })
                    .await;
            }
            let Some(hash) = supported_argon_hash(hashed) else {
                return Ok(false);
            };
            let Ok(params) = Params::try_from(&hash) else {
                return Ok(false);
            };
            limits.argon(&params)?;
            self.pool
                .run(value, move |value| {
                    Ok(Argon2::default().verify_password(value, &hash).is_ok())
                })
                .await
        })
    }

    fn needsRehash(&self, hashed: &str, options: &HashOptions) -> HashResult<bool> {
        if inspect(hashed).algoName != self.settings.algorithm() {
            return Ok(true);
        }
        Ok(self.stored(hashed) != Self::target(self.settings, options)?.map(i64::from))
    }

    fn verifyConfiguration(&self, hashed: &str) -> HashResult<bool> {
        let info = inspect(hashed);
        if info.algoName != self.settings.algorithm() {
            return Ok(false);
        }
        let names: &[&str] = match self.settings {
            Settings::Bcrypt(_) => &["cost"],
            Settings::Argon(..) => &["memory_cost", "time_cost", "threads"],
        };
        let target = Self::target(self.settings, &HashOptions::new())?;
        Ok(names.iter().zip(target).all(|(name, max)| {
            info.options
                .get(*name)
                .and_then(serde_json::Value::as_i64)
                .is_some_and(|value| value <= i64::from(max))
        }))
    }
}

fn option(options: &HashOptions, name: &str, fallback: u32) -> HashResult<u32> {
    use serde_json::Value;
    match options.get(name) {
        None | Some(Value::Null) => Ok(fallback),
        Some(Value::String(value)) => value.trim().parse().map_err(|_| HashError::InvalidOptions),
        Some(value) => value
            .as_u64()
            .and_then(|value| value.try_into().ok())
            .ok_or(HashError::InvalidOptions),
    }
}

fn inspect(hashed: &str) -> HashInfo {
    let mut options = HashOptions::new();
    let (algo, name) = if hashed.starts_with("$2y$") && hashed.len() == 60 {
        options.insert("cost".into(), parameters::bcrypt_cost(hashed).into());
        (Some("2y"), "bcrypt")
    } else if hashed.starts_with("$argon2i$") || hashed.starts_with("$argon2id$") {
        let algorithm = if hashed.starts_with("$argon2id$") {
            "argon2id"
        } else {
            "argon2i"
        };
        let params = parameters::argon_parameters(hashed, [65_536, 4, 1]);
        for (name, value) in ["memory_cost", "time_cost", "threads"]
            .into_iter()
            .zip(params)
        {
            options.insert(name.into(), value.into());
        }
        (Some(algorithm), algorithm)
    } else {
        (None, "unknown")
    };
    HashInfo {
        algo: algo.map(str::to_owned),
        algoName: name.into(),
        options,
    }
}

fn supported_argon_hash(encoded: &str) -> Option<PasswordHash> {
    let mut hash = PasswordHash::new(encoded).ok()?;
    if !matches!(hash.algorithm.as_str(), "argon2i" | "argon2id") {
        return None;
    }
    let offset = match hash.version {
        Some(16 | 19) => 3,
        None => {
            // The original Argon2 PHC encoding omitted the version field.
            hash.version = Some(16);
            2
        }
        _ => return None,
    };
    let mut parameters = encoded.split('$').nth(offset)?.split(',');
    let ordered = ["m=", "t=", "p="].iter().all(|prefix| {
        parameters
            .next()
            .is_some_and(|param| param.starts_with(prefix))
    });
    (ordered && parameters.next().is_none()).then_some(hash)
}

macro_rules! define_hasher {
    ($name:ident, $config:ty, $settings:expr, $doc:literal) => {
        #[doc = $doc]
        pub struct $name(super::driver::Driver);

        impl $name {
            pub fn new(config: $config) -> Self {
                Self::with_pool(config, super::driver::OperationPool::for_driver())
            }

            pub(super) fn with_pool(config: $config, pool: super::driver::OperationPool) -> Self {
                Self(super::driver::Driver::new(($settings)(config), pool))
            }
        }

        #[allow(non_snake_case, reason = "Matches Laravel's hash API")]
        impl crate::contracts::hashing::Hasher for $name {
            fn info(&self, hashed: &str) -> super::HashResult<super::HashInfo> {
                self.0.info(hashed)
            }

            fn make<'a>(
                &'a self,
                value: &'a [u8],
                options: &'a super::HashOptions,
            ) -> super::HashFuture<'a, super::HashResult<String>> {
                self.0.make(value, options)
            }

            fn check<'a>(
                &'a self,
                value: &'a [u8],
                hashed: &'a str,
                options: &'a super::HashOptions,
            ) -> super::HashFuture<'a, super::HashResult<bool>> {
                self.0.check(value, hashed, options)
            }

            fn needsRehash(
                &self,
                hashed: &str,
                options: &super::HashOptions,
            ) -> super::HashResult<bool> {
                self.0.needsRehash(hashed, options)
            }

            fn verifyConfiguration(&self, hashed: &str) -> super::HashResult<bool> {
                self.0.verifyConfiguration(hashed)
            }
        }
    };
}

pub(super) use define_hasher;
