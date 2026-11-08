//! 通过应用服务管理器解析缓存、日志、数据库和文件系统属性。

use super::{
    Arc, AttributeParameter, AttributeValue, BoxFuture, ContextualAttribute, ParameterValue,
    Resolver, Result,
};
use crate::{database::Databases, filesystem::FilesystemManager, logging::LogManager};

/// 从应用缓存管理器注入默认或具名的 [`crate::cache::CacheStore`]。
#[derive(Default)]
pub struct Cache {
    store: Option<Arc<str>>,
    memo: bool,
}

impl Cache {
    /// 按配置中的精确名称选择 store。
    pub fn new(store: impl Into<Arc<str>>) -> Self {
        Self {
            store: Some(store.into()),
            memo: false,
        }
    }

    /// 在当前容器作用域内共享缓存读取结果。
    #[must_use]
    pub fn memoized(mut self) -> Self {
        self.memo = true;
        self
    }

    /// 借用声明的原始名称。未指定名称时返回 `None`。
    #[must_use]
    pub fn store_name(&self) -> Option<&str> {
        self.store.as_deref()
    }
}

impl ContextualAttribute for Cache {
    fn resolve(
        self: Arc<Self>,
        _parameter: AttributeParameter,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async move {
            let name = self.store_name();
            let store = if self.memo {
                crate::cache::Cache::memo(&resolver, name).await?
            } else {
                let cache = resolver.resolve::<crate::cache::Cache>().await?;
                let store = match name {
                    Some(name) => cache.store(name).await?,
                    None => cache.default_store().await?,
                };
                Arc::new(store)
            };
            Ok(AttributeValue::One(ParameterValue::service(store)))
        })
    }
}

/// 从应用日志管理器注入默认或具名的 [`crate::logging::Logger`]。
#[derive(Default)]
pub struct Log {
    channel: Option<Arc<str>>,
    name: Option<Arc<str>>,
}

impl Log {
    /// 按频道名选择日志输出目标。
    pub fn new(channel: impl Into<Arc<str>>) -> Self {
        Self {
            channel: Some(channel.into()),
            name: None,
        }
    }

    /// 为注入的句柄设置独立 logger 名称。
    #[must_use]
    pub fn named(mut self, name: impl Into<Arc<str>>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// 借用声明的频道名称。未指定时由管理器选默认频道。
    pub fn channel_name(&self) -> Option<&str> {
        self.channel.as_deref()
    }

    /// 借用声明的重命名值。未指定时保留频道自己的 logger 名称。
    pub fn logger_name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

impl ContextualAttribute for Log {
    fn resolve(
        self: Arc<Self>,
        _parameter: AttributeParameter,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async move {
            let manager = resolver.resolve::<LogManager>().await?;
            let logger = manager.channel(self.channel_name()).await?;
            let logger = match self.logger_name() {
                Some(name) => Arc::new(logger.with_name(name)),
                None => logger,
            };
            Ok(AttributeValue::One(ParameterValue::service(logger)))
        })
    }
}

/// 从应用的数据库管理器注入默认或具名 [`crate::database::Db`]。
#[derive(Default)]
pub struct Database {
    connection: Option<Arc<str>>,
}

impl Database {
    /// 按名称选择数据库。空字符串与字符串 `0` 选择默认数据库。
    pub fn new(connection: impl Into<Arc<str>>) -> Self {
        Self {
            connection: Some(connection.into()),
        }
    }

    /// 借用属性声明的原始名称。未指定名称时返回 `None`。
    #[must_use]
    pub fn connection_name(&self) -> Option<&str> {
        self.connection.as_deref()
    }

    async fn value(&self, resolver: &Resolver) -> Result<AttributeValue> {
        let databases = resolver.resolve::<Databases>().await?;
        let handle = match selected_name(self.connection_name()) {
            Some(name) => databases.get(name)?,
            None => databases.default_database(),
        };
        // 数据库 Provider 可以保留未配置的默认占位句柄。属性需要实际连接。
        handle.connection()?;
        Ok(AttributeValue::One(ParameterValue::service(Arc::new(
            handle,
        ))))
    }
}

impl ContextualAttribute for Database {
    fn resolve(
        self: Arc<Self>,
        _parameter: AttributeParameter,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async move { self.value(&resolver).await })
    }
}

/// Laravel 的 `DB` 属性。与 [`Database`] 共用解析逻辑，拥有独立属性身份。
#[derive(Default)]
pub struct DbAttribute(Database);

impl DbAttribute {
    /// 按名称选择数据库。空字符串与字符串 `0` 选择默认数据库。
    pub fn new(connection: impl Into<Arc<str>>) -> Self {
        Self(Database::new(connection))
    }

    /// 借用属性声明的原始连接名称。
    #[must_use]
    pub fn connection_name(&self) -> Option<&str> {
        self.0.connection_name()
    }
}

impl ContextualAttribute for DbAttribute {
    fn resolve(
        self: Arc<Self>,
        _parameter: AttributeParameter,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async move { self.0.value(&resolver).await })
    }
}

/// 从应用的文件系统管理器注入默认或具名 [`crate::filesystem::FilesystemAdapter`]。
#[derive(Default)]
pub struct Storage {
    disk: Option<Arc<str>>,
}

impl Storage {
    /// 按名称选择磁盘。空字符串与字符串 `0` 选择默认磁盘。
    pub fn new(disk: impl Into<Arc<str>>) -> Self {
        Self {
            disk: Some(disk.into()),
        }
    }

    /// 借用属性声明的原始磁盘名称。未指定名称时返回 `None`。
    #[must_use]
    pub fn disk_name(&self) -> Option<&str> {
        self.disk.as_deref()
    }
}

impl ContextualAttribute for Storage {
    fn resolve(
        self: Arc<Self>,
        _parameter: AttributeParameter,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async move {
            let manager = resolver.resolve::<FilesystemManager>().await?;
            let disk = match selected_name(self.disk_name()) {
                Some(name) => manager.disk(name).await?,
                None => manager.default_disk().await?,
            };
            Ok(AttributeValue::One(ParameterValue::service(Arc::new(disk))))
        })
    }
}

fn selected_name(name: Option<&str>) -> Option<&str> {
    name.filter(|name| !matches!(*name, "" | "0"))
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
