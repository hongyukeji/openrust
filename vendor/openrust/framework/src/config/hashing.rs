//! 裸驱动默认与应用安装默认分离，避免配置来源混淆。

use serde::{Deserialize, Serialize};

/// bcrypt 裸驱动配置；默认不强制检查已存算法。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BcryptConfig {
    /// 计算成本，默认 12。
    pub rounds: u32,
    /// 是否拒绝与当前驱动不同的已存算法。
    pub verify: bool,
    /// 可选输入字节上限；None 或 0 不增加上限。
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

/// Argon2i/id 共用的裸驱动配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ArgonConfig {
    /// 内存成本，单位 KiB；裸驱动默认 1024。
    pub memory: u32,
    /// 迭代次数；裸驱动默认 2。
    pub time: u32,
    /// 并行度；裸驱动默认 2。
    pub threads: u32,
    /// 是否拒绝与当前驱动不同的已存算法。
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

/// 应用安装配置的默认值，与直接创建驱动的默认值分开。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HashingConfig {
    /// 默认驱动名，默认 bcrypt。
    pub driver: String,
    /// bcrypt 安装选项，默认 verify=true。
    pub bcrypt: BcryptConfig,
    /// 两种 Argon 驱动共用选项，默认 65536KiB、4 次、1 并行及 verify=true。
    pub argon: ArgonConfig,
    /// 登录成功后是否通过 `UserProvider` 按需重哈希。
    pub rehash_on_login: bool,
    /// 内置 KDF 的显式计算预算，不改变元信息及重算比较语义。
    pub execution: ExecutionLimits,
}
impl Default for HashingConfig {
    fn default() -> Self {
        Self {
            driver: "bcrypt".into(),
            bcrypt: BcryptConfig {
                verify: true,
                ..BcryptConfig::default()
            },
            argon: ArgonConfig {
                memory: 65_536,
                time: 4,
                threads: 1,
                verify: true,
            },
            rehash_on_login: true,
            execution: ExecutionLimits::default(),
        }
    }
}

/// 通用安全增强：内置 KDF 执行上限，可按部署能力显式提高。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExecutionLimits {
    /// Argon最大内存，单位KiB；默认262144。
    pub max_argon_memory: u32,
    /// Argon最大迭代次数；默认10。
    pub max_argon_time: u32,
    /// Argon最大并行度；默认4。
    pub max_argon_threads: u32,
    /// bcrypt最大成本；默认16。
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
