//! 在应用唯一容器中登记哈希管理器与默认 Hasher 契约。

use super::{HashManager, Hasher, HashingConfig, failure};
use crate::core::Result;
use crate::foundation::RegisterContext;
use crate::support::BoxFuture;
use crate::support::ServiceProvider;
use std::sync::Arc;

/// 应用哈希服务 Provider；独立于认证与 Passport 的启用状态。
#[derive(Debug, Clone, Copy)]
pub struct HashServiceProvider {
    capacity: usize,
}
impl Default for HashServiceProvider {
    fn default() -> Self {
        Self::new()
    }
}
impl HashServiceProvider {
    /// 创建默认容量为两个阻塞密码操作的 Provider。
    pub const fn new() -> Self {
        Self { capacity: 2 }
    }
    /// 创建指定并发容量的 Provider；支持1至4个内置密码操作。
    pub fn with_max_concurrent_operations(capacity: usize) -> Result<Self> {
        if !(1..=4).contains(&capacity) {
            return Err(failure(
                "hashing_capacity_invalid",
                "哈希并发容量必须为1至4",
            ));
        }
        Ok(Self { capacity })
    }
}

// 与业务服务绑定分开记录区段登记，保证应用自定义绑定也能取得统一配置。
struct HashingSectionRegistered;

impl ServiceProvider for HashServiceProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if !app.is_bound::<HashingSectionRegistered>() {
                app.config_section("hashing", HashingConfig::default(), schema(), |_| Ok(()))?;
                app.instance(Arc::new(HashingSectionRegistered)).await?;
            }
            if !app.is_bound::<HashManager>() {
                let capacity = self.capacity;
                let repository = app.config_repository();
                app.singleton::<HashManager, _>(move |_resolver| {
                    let repository = repository.clone();
                    Box::pin(async move {
                        Ok(Arc::new(HashManager::from_repository(
                            repository, capacity,
                        )?))
                    })
                })
                .await?;
            }
            if !app.is_bound::<dyn Hasher>() {
                app.singleton::<dyn Hasher, _>(|resolver| {
                    Box::pin(async move { resolver.resolve::<HashManager>().await?.driver(None) })
                })
                .await?;
            }
            Ok(())
        })
    }
}

fn schema() -> crate::config::repository::SectionSchema {
    use crate::config::repository::SectionSchema as S;
    S::object([
        ("driver", S::String),
        (
            "bcrypt",
            S::object([
                ("rounds", S::Integer),
                ("verify", S::Boolean),
                ("limit", S::Integer.nullable()),
            ]),
        ),
        (
            "argon",
            S::object([
                ("memory", S::Integer),
                ("time", S::Integer),
                ("threads", S::Integer),
                ("verify", S::Boolean),
            ]),
        ),
        ("rehash_on_login", S::Boolean),
        (
            "execution",
            S::object([
                ("max_argon_memory", S::Integer),
                ("max_argon_time", S::Integer),
                ("max_argon_threads", S::Integer),
                ("max_bcrypt_rounds", S::Integer),
            ]),
        ),
    ])
    .open()
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
