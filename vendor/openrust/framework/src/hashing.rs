//! Laravel `HashManager` 对应：统一密码与客户端秘密的可扩展哈希服务。
// 对标 Laravel v13.39.0 / d52c3b3129e4c8e213c97cdc68f94b0b7d27f181
// Illuminate/Hashing；上游版权 Taylor Otwell，MIT 许可。

mod drivers;
mod legacy_bcrypt;
mod manager;
mod parameters;
mod provider;

pub use crate::config::hashing::ExecutionLimits;
pub use crate::config::{ArgonConfig, BcryptConfig, HashingConfig};
pub use drivers::{Argon2IdHasher, ArgonHasher, BcryptHasher};
pub use manager::HashManager;
pub use provider::HashServiceProvider;

use crate::core::Error;
use crate::core::Result;
use crate::support::BoxFuture;
use std::collections::BTreeMap;

/// 每次调用的上游选项；内置驱动识别 rounds 或 memory/time/threads。
pub type HashOptions = BTreeMap<String, serde_json::Value>;

/// 已编码哈希的算法与参数元信息，不包含盐、摘要或原始密码。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct HashInfo {
    /// PHP 对应算法标识；无法识别时为 None。
    pub algo: Option<String>,
    /// 算法名称；无法识别时为 unknown，JSON字段保持官方algoName。
    #[serde(rename = "algoName")]
    pub algo_name: String,
    /// 存储哈希中的非敏感资源参数；空集合按 PHP 数组语义序列化为 []。
    #[serde(serialize_with = "serialize_info_options")]
    pub options: HashOptions,
}

fn serialize_info_options<S>(
    options: &HashOptions,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    if options.is_empty() {
        serializer.collect_seq(std::iter::empty::<()>())
    } else {
        serde::Serialize::serialize(options, serializer)
    }
}

/// 可由同一容器管理器解析的哈希驱动；实现不得输出秘密原值。
pub trait Hasher: Send + Sync {
    /// 查询增强认证策略能否保留完整输入字节；Rust 安全扩展，不改变通用 make/check 行为。
    ///
    /// 自定义驱动默认承诺完整输入；可能接受 bcrypt 迁移哈希的驱动应声明其别名边界。
    fn accepts_exact_input(&self, _value: &str) -> Result<bool> {
        Ok(true)
    }
    /// 检查已编码值的元信息；未知格式返回 unknown，驱动构造错误仍传播。
    fn info(&self, encoded: &str) -> Result<HashInfo>;
    /// 使用当前驱动和可选覆盖参数生成随机盐哈希。
    fn make<'a>(
        &'a self,
        value: &'a str,
        options: &'a HashOptions,
    ) -> BoxFuture<'a, Result<String>>;
    /// 校验明文与可选存储值；空存储值返回 false。
    fn check<'a>(
        &'a self,
        value: &'a str,
        encoded: Option<&'a str>,
        options: &'a HashOptions,
    ) -> BoxFuture<'a, Result<bool>>;
    /// 比较算法和有效资源参数，不因随机盐变化要求重算。
    fn needs_rehash(&self, encoded: &str, options: &HashOptions) -> Result<bool>;
    /// 检查已有哈希能否在当前配置下直接写入；自定义驱动默认不额外限制。
    fn verify_configuration(&self, _encoded: &str) -> Result<bool> {
        Ok(true)
    }
}

fn failure(code: &'static str, message: &'static str) -> Error {
    Error::custom(code, message)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod regression_tests;
