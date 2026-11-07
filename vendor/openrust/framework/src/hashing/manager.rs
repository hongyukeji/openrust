//! 按驱动名缓存实例；执行扩展工厂时不持管理器状态锁。

use super::{
    Argon2IdHasher, ArgonHasher, BcryptHasher, HashInfo, HashOptions, Hasher, HashingConfig,
    drivers::OperationPool, failure,
};
use crate::core::Result;
use crate::support::BoxFuture;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

fn self_reference() -> crate::core::Error {
    failure(
        "hashing_driver_self_reference",
        "哈希驱动不能引用所属管理器自身",
    )
}

type Factory = Arc<dyn Fn() -> Result<Arc<dyn Hasher>> + Send + Sync>;
#[derive(Default)]
struct State {
    factories: BTreeMap<String, Factory>,
    drivers: BTreeMap<String, Arc<dyn Hasher>>,
    creating: BTreeSet<String>,
}

/// 应用唯一哈希驱动管理器；扩展工厂可捕获已由同一容器解析的服务。
pub struct HashManager {
    source: Source,
    pool: OperationPool,
    state: Arc<Mutex<State>>,
}
enum Source {
    Snapshot(HashingConfig),
    Live(crate::config::repository::ConfigRepository),
}
impl Source {
    fn default_driver(&self) -> Result<String> {
        match self {
            Self::Snapshot(config) => Ok(config.driver.clone()),
            Self::Live(repository) => repository
                .section("hashing.driver")
                .map_err(crate::core::Error::from),
        }
    }
    fn bcrypt(&self) -> Result<super::BcryptConfig> {
        match self {
            Self::Snapshot(config) => Ok(config.bcrypt.clone()),
            Self::Live(repository) => repository
                .section("hashing.bcrypt")
                .map_err(crate::core::Error::from),
        }
    }
    fn argon(&self) -> Result<super::ArgonConfig> {
        match self {
            Self::Snapshot(config) => Ok(config.argon.clone()),
            Self::Live(repository) => repository
                .section("hashing.argon")
                .map_err(crate::core::Error::from),
        }
    }
    fn execution(&self) -> Result<super::ExecutionLimits> {
        match self {
            Self::Snapshot(config) => Ok(config.execution.clone()),
            Self::Live(repository) => repository
                .section("hashing.execution")
                .map_err(crate::core::Error::from),
        }
    }
}

struct Creation {
    state: Arc<Mutex<State>>,
    name: String,
}
impl Drop for Creation {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.creating.remove(&self.name);
        }
    }
}
/// 持有构造占位，发布或失败析构后才允许下一次同名构造。
struct Prepared {
    driver: Arc<dyn Hasher>,
    creation: Option<Creation>,
    // 实际工厂跨发布保活，最后捕获析构可在缓存可见后锁外重入。
    _factory: Option<Factory>,
}
impl Prepared {
    fn publish(self) -> Result<Arc<dyn Hasher>> {
        if let Some(creation) = &self.creation {
            let displaced = creation
                .state
                .lock()
                .map_err(|_| failure("hashing_manager_poisoned", "哈希管理器状态锁无效"))?
                .drivers
                .insert(creation.name.clone(), Arc::clone(&self.driver));
            drop(displaced);
        }
        Ok(Arc::clone(&self.driver))
    }
}
impl HashManager {
    /// 创建应用管理器，内置驱动共用两个并发操作容量。
    pub fn new(config: HashingConfig) -> Result<Self> {
        Self::with_max_concurrent_operations(config, 2)
    }
    /// 创建管理器并指定内置驱动共用容量，范围1至4。
    pub fn with_max_concurrent_operations(config: HashingConfig, capacity: usize) -> Result<Self> {
        Ok(Self {
            source: Source::Snapshot(config),
            pool: OperationPool::new(capacity)?,
            state: Arc::new(Mutex::new(State::default())),
        })
    }
    /// 使用应用同一活配置仓库；仅首次构造具名驱动时读取该驱动配置与预算。
    pub fn from_repository(
        repository: crate::config::repository::ConfigRepository,
        capacity: usize,
    ) -> Result<Self> {
        Ok(Self {
            source: Source::Live(repository),
            pool: OperationPool::new(capacity)?,
            state: Arc::new(Mutex::new(State::default())),
        })
    }
    /// 注册驱动工厂；首次创建时自定义工厂优先，已缓存实例保持稳定。
    pub fn extend<F>(&self, name: impl Into<String>, factory: F) -> Result<()>
    where
        F: Fn() -> Result<Arc<dyn Hasher>> + Send + Sync + 'static,
    {
        let name = name.into();
        let displaced = self
            .state
            .lock()
            .map_err(|_| failure("hashing_manager_poisoned", "哈希管理器状态锁无效"))?
            .factories
            .insert(name, Arc::new(factory));
        drop(displaced);
        Ok(())
    }
    /// 解析具名驱动；None、空名及PHP假值字符串0选择默认驱动。
    ///
    /// 同名工厂递归或并发创建明确拒绝，不持锁执行工厂、不等待用户回调。
    /// 工厂直接返回本管理器时使该工厂失效，须显式 extend 修复；普通错误仍可重试。
    pub fn driver(&self, name: Option<&str>) -> Result<Arc<dyn Hasher>> {
        let name = match name.filter(|name| !name.is_empty() && *name != "0") {
            Some(name) => name.to_owned(),
            None => self.source.default_driver()?,
        };
        self.prepare(&name)?.publish()
    }
    /// 固定本次操作的默认最终驱动；整条已知管理器链验证成功后才发布新缓存。
    pub fn driver_for_operation(&self) -> Result<Arc<dyn Hasher>> {
        let mut seen = vec![Arc::clone(&self.state)];
        let mut prepared = Vec::new();
        let mut next = self.prepare(&self.source.default_driver()?)?;
        loop {
            let driver = Arc::clone(&next.driver);
            // 每个实际候选仅投影一次，借用有效时完成下一层准备。
            let following = if let Some(nested) = driver.as_hash_manager() {
                if seen.iter().any(|state| Arc::ptr_eq(state, &nested.state)) {
                    return Err(self_reference());
                }
                seen.push(Arc::clone(&nested.state));
                Some(nested.prepare(&nested.source.default_driver()?)?)
            } else {
                None
            };
            prepared.push(next);
            if let Some(following) = following {
                next = following;
            } else {
                for entry in prepared.into_iter().rev() {
                    entry.publish()?;
                }
                return Ok(driver);
            }
        }
    }
    fn prepare(&self, name: &str) -> Result<Prepared> {
        let factory = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| failure("hashing_manager_poisoned", "哈希管理器状态锁无效"))?;
            if let Some(driver) = state.drivers.get(name) {
                return Ok(Prepared {
                    driver: Arc::clone(driver),
                    creation: None,
                    _factory: None,
                });
            }
            if !state.creating.insert(name.to_owned()) {
                return Err(failure(
                    "hashing_driver_busy",
                    "哈希驱动正在创建，不能递归或并发解析同名驱动",
                ));
            }
            state.factories.get(name).cloned()
        };
        let creation = Creation {
            state: Arc::clone(&self.state),
            name: name.to_owned(),
        };
        let driver: Arc<dyn Hasher> = if let Some(factory) = &factory {
            factory()?
        } else {
            match name {
                "bcrypt" => Arc::new(BcryptHasher::with_pool(
                    self.source.bcrypt()?,
                    self.pool.limited(self.source.execution()?),
                )),
                "argon" => Arc::new(ArgonHasher::with_pool(
                    self.source.argon()?,
                    self.pool.limited(self.source.execution()?),
                )),
                "argon2id" => Arc::new(Argon2IdHasher::with_pool(
                    self.source.argon()?,
                    self.pool.limited(self.source.execution()?),
                )),
                _ => return Err(failure("hashing_driver_unsupported", "未登记此哈希驱动")),
            }
        };
        // 工厂可返回任意 Hasher，但将本管理器缓存为自己的 driver 会造成递归及 Arc 环。
        if std::ptr::eq(
            Arc::as_ptr(&driver).cast::<()>(),
            std::ptr::from_ref(self).cast::<()>(),
        ) {
            // 仅废弃此次实际调用、且仍登记的同一工厂；保留哨兵阻止内置名静默回退。
            // 工厂与其捕获值的析构必须在状态锁外执行，允许析构重入管理器。
            let displaced = {
                let mut state = self
                    .state
                    .lock()
                    .map_err(|_| failure("hashing_manager_poisoned", "哈希管理器状态锁无效"))?;
                if factory.as_ref().is_some_and(|attempted| {
                    state
                        .factories
                        .get(name)
                        .is_some_and(|current| Arc::ptr_eq(current, attempted))
                }) {
                    let sentinel: Factory = Arc::new(|| Err(self_reference()));
                    state.factories.insert(name.to_owned(), sentinel)
                } else {
                    None
                }
            };
            drop(driver);
            drop(displaced);
            drop(factory);
            return Err(self_reference());
        }
        Ok(Prepared {
            driver,
            creation: Some(creation),
            _factory: factory,
        })
    }
    /// 查询当前默认驱动的完整输入能力，不执行 KDF 或改变通用哈希行为。
    pub fn accepts_exact_input(&self, value: impl AsRef<[u8]>) -> Result<bool> {
        self.driver(None)?.accepts_exact_input(value.as_ref())
    }
    /// 使用当前默认驱动生成哈希；文本与byte容器都只转发原字节。
    pub async fn make(&self, value: impl AsRef<[u8]>, options: &HashOptions) -> Result<String> {
        self.driver(None)?.make(value.as_ref(), options).await
    }
    /// 使用当前默认驱动验证；文本/bytes都保原文，verify策略由该驱动配置决定。
    pub async fn check(
        &self,
        value: impl AsRef<[u8]>,
        encoded: Option<&str>,
        options: &HashOptions,
    ) -> Result<bool> {
        self.driver(None)?
            .check(value.as_ref(), encoded, options)
            .await
    }
    /// 通过默认驱动读取元信息；内置驱动可识别其它内置算法。
    pub fn info(&self, encoded: &str) -> Result<HashInfo> {
        self.driver(None)?.info(encoded)
    }
    /// 根据当前默认驱动及覆盖参数判断是否需要重算。
    pub fn needs_rehash(&self, encoded: &str, options: &HashOptions) -> Result<bool> {
        self.driver(None)?.needs_rehash(encoded, options)
    }
    /// 是否为可识别哈希，不要求其算法等于默认驱动。
    pub fn is_hashed(&self, value: &str) -> Result<bool> {
        Ok(self.info(value)?.algo.is_some())
    }
    /// 检查已有哈希的写入配置边界。
    pub fn verify_configuration(&self, value: &str) -> Result<bool> {
        self.driver(None)?.verify_configuration(value)
    }
}

impl Hasher for HashManager {
    fn as_hash_manager(&self) -> Option<&HashManager> {
        Some(self)
    }
    fn accepts_exact_input(&self, value: &[u8]) -> Result<bool> {
        HashManager::accepts_exact_input(self, value)
    }
    fn info(&self, encoded: &str) -> Result<HashInfo> {
        HashManager::info(self, encoded)
    }
    fn make<'a>(
        &'a self,
        value: &'a [u8],
        options: &'a HashOptions,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move { HashManager::make(self, value, options).await })
    }
    fn check<'a>(
        &'a self,
        value: &'a [u8],
        encoded: Option<&'a str>,
        options: &'a HashOptions,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move { HashManager::check(self, value, encoded, options).await })
    }
    fn needs_rehash(&self, encoded: &str, options: &HashOptions) -> Result<bool> {
        HashManager::needs_rehash(self, encoded, options)
    }
    fn verify_configuration(&self, encoded: &str) -> Result<bool> {
        HashManager::verify_configuration(self, encoded)
    }
}

#[cfg(test)]
#[path = "operation_selection_tests.rs"]
mod operation_selection_tests;
