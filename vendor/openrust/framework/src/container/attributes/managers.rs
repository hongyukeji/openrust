//! 通过应用服务管理器解析数据库和文件系统属性。

use super::{
    Arc, AttributeParameter, AttributeValue, BoxFuture, ContextualAttribute, ParameterValue,
    Resolver, Result,
};
use crate::{database::Databases, filesystem::FilesystemManager};

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
