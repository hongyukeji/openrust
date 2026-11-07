//! 固定 Passport v13.10.0 `ClientRepository` 的读取子范围；不提供模型保存生命周期。

use super::{ClientModel, ClientModelFactory};
use openrust::Error;
use openrust::Result;
use openrust::container::{Resolver, Scope};
use openrust::database::Databases;
use openrust::database::Db;
use openrust::database::eloquent::attributes_from_row;
#[cfg(test)]
use openrust::database::sea_orm::Value as SqlValue;
use openrust::foundation::RegisterContext;
use openrust::support::BoxFuture;
use serde_json::Value;
use std::sync::Arc;

#[path = "repository_cache.rs"]
mod cache;
pub(crate) use cache::ClientLookupCache;
use cache::RepositoryIdentity;

/// 同一选定客户端模型实例，不复制原始属性作为第二份模型。
pub type ClientRecord = Arc<dyn ClientModel>;

/// 对当前入口Scope或scoped工厂Resolver的短借用；不是身份、权限或事务证明。
///
/// 不创建scope、不保活Container。手动调用方负责同应用同入口来源；类型不证明非root。
#[derive(Clone, Copy)]
pub struct ClientLookupContext<'a> {
    source: LookupSource<'a>,
}
#[derive(Clone, Copy)]
enum LookupSource<'a> {
    Scope(&'a Scope),
    Resolver(&'a Resolver),
}
impl<'a> From<&'a Scope> for ClientLookupContext<'a> {
    fn from(scope: &'a Scope) -> Self {
        Self {
            source: LookupSource::Scope(scope),
        }
    }
}
impl<'a> From<&'a Resolver> for ClientLookupContext<'a> {
    fn from(resolver: &'a Resolver) -> Self {
        Self {
            source: LookupSource::Resolver(resolver),
        }
    }
}
impl ClientLookupContext<'_> {
    /// 委托原入口解析；原关闭、captive及容器销毁错误原样传播。
    pub fn resolve<T: ?Sized + Send + Sync + 'static>(&self) -> BoxFuture<'_, Result<Arc<T>>> {
        match self.source {
            LookupSource::Scope(scope) => Box::pin(scope.resolve::<T>()),
            LookupSource::Resolver(resolver) => resolver.resolve::<T>(),
        }
    }
    pub(crate) async fn ensure_open(&self) -> Result<()> {
        self.resolve::<ClientLookupCache>().await?;
        Ok(())
    }
}

/// 完整客户端读取扩展；自定义模型可替换此实现以接入自己的查询生命周期。
///
/// 默认实现目前复用通用 Db Query，尚不提供 Eloquent global scopes 或模型查询事件。
pub trait ClientLookup: Send + Sync {
    /// 依指定入口 Scope 查询；错误必须传播，不能作为不存在缓存。
    fn find<'a>(
        &'a self,
        scope: &'a ClientLookupContext<'a>,
        identifier: &'a Value,
    ) -> BoxFuture<'a, Result<Option<ClientRecord>>>;
    /// 查得模型后每次读取当前 revoked 属性，不缓存活动状态。
    fn find_active<'a>(
        &'a self,
        scope: &'a ClientLookupContext<'a>,
        identifier: &'a Value,
    ) -> BoxFuture<'a, Result<Option<ClientRecord>>> {
        Box::pin(async move {
            match self.find(scope, identifier).await? {
                Some(client) if !client.revoked()? => Ok(Some(client)),
                _ => Ok(None),
            }
        })
    }
    /// 协议字符串入口；默认保留原 `String` 并派发自定义 `find_active` 覆写。
    fn find_active_by_protocol<'a>(
        &'a self,
        scope: &'a ClientLookupContext<'a>,
        identifier: &'a str,
    ) -> BoxFuture<'a, Result<Option<ClientRecord>>> {
        Box::pin(async move {
            self.find_active(scope, &Value::String(identifier.to_owned()))
                .await
        })
    }
}

/// 默认客户端仓储；可以共享仓储配置，查询结果仅属于实际传入的 Scope。
#[derive(Clone)]
pub struct ClientRepository {
    db: Db,
    factory: Arc<dyn ClientModelFactory>,
    identity: Arc<RepositoryIdentity>,
}
impl ClientRepository {
    /// 按同一模型元数据选择连接；显式模型连接优先于 Passport 配置与应用默认。
    pub fn new(
        databases: &Databases,
        passport_connection: Option<&str>,
        factory: Arc<dyn ClientModelFactory>,
    ) -> Result<Self> {
        let db = factory
            .metadata()
            .resolve_connection(databases, passport_connection)?;
        Self::from_db(db, factory)
    }

    pub(crate) fn from_db(db: Db, factory: Arc<dyn ClientModelFactory>) -> Result<Self> {
        db.table_names().resolve(&factory.metadata().table)?;
        Ok(Self {
            db,
            factory,
            identity: Arc::new(RepositoryIdentity),
        })
    }

    async fn find_with_query_key(
        &self,
        scope: &ClientLookupContext<'_>,
        original: &Value,
        query: &Value,
    ) -> Result<Option<ClientRecord>> {
        let original_key = Identifier::from_value(original)?;
        let query_key = Identifier::from_value(query)?;
        if !self.factory.is_valid_identifier(query) {
            return Ok(None);
        }
        let cache = scope.resolve::<ClientLookupCache>().await?;
        if original_key == query_key {
            cache
                .find(&self.identity, original_key, || self.load(query))
                .await
        } else {
            cache
                .find_query(&self.identity, original_key, query_key, || self.load(query))
                .await
        }
    }

    async fn load(&self, identifier: &Value) -> Result<Option<ClientRecord>> {
        let metadata = self.factory.metadata();
        // 普通 find 沿 Db 默认读连接；当前没有事务绑定，不能用于签发期权威重验。
        let row = self
            .db
            .table(&metadata.table)?
            .where_eq(
                &metadata.key_name,
                self.factory
                    .identifier_binding(identifier, self.db.backend()?)?,
            )?
            .first()
            .await?;
        row.map(|row| self.factory.new_from_builder(attributes_from_row(&row)?))
            .transpose()
    }
}
impl ClientLookup for ClientRepository {
    fn find<'a>(
        &'a self,
        scope: &'a ClientLookupContext<'a>,
        identifier: &'a Value,
    ) -> BoxFuture<'a, Result<Option<ClientRecord>>> {
        Box::pin(async move {
            self.find_with_query_key(scope, identifier, identifier)
                .await
        })
    }

    fn find_active_by_protocol<'a>(
        &'a self,
        scope: &'a ClientLookupContext<'a>,
        identifier: &'a str,
    ) -> BoxFuture<'a, Result<Option<ClientRecord>>> {
        Box::pin(async move {
            let original = Value::String(identifier.to_owned());
            let Some(query) = self.factory.identifier_from_protocol(identifier) else {
                return Ok(None);
            };
            match self.find_with_query_key(scope, &original, &query).await? {
                Some(client) if !client.revoked()? => Ok(Some(client)),
                _ => Ok(None),
            }
        })
    }
}

/// 在同一 Container 登记私有入口缓存；由实际 Passport Provider 的 register 调用一次。
///
/// 仓储 singleton 不解析该 scoped 服务；HTTP、CLI 和队列必须传各自的 Scope。
pub async fn register_client_repository_scope(app: &mut RegisterContext<'_>) -> Result<()> {
    app.scoped::<ClientLookupCache, _>(|_| {
        Box::pin(async { Ok(Arc::new(ClientLookupCache::default())) })
    })
    .await
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Identifier {
    String(String),
    Integer(i64),
}
impl Identifier {
    fn from_value(value: &Value) -> Result<Self> {
        match value {
            Value::String(value) => Ok(Self::String(value.clone())),
            Value::Number(value) => value
                .as_i64()
                .map(Self::Integer)
                .ok_or_else(invalid_identifier),
            _ => Err(invalid_identifier()),
        }
    }
    #[cfg(test)]
    fn binding(&self) -> SqlValue {
        match self {
            Self::String(value) => value.clone().into(),
            Self::Integer(value) => (*value).into(),
        }
    }
}
fn invalid_identifier() -> Error {
    Error::custom_with_status(
        openrust::http::axum::http::StatusCode::BAD_REQUEST,
        "passport_client_identifier_invalid",
        "客户端查找标识必须是字符串或 PHP 64 位整数",
    )
}

#[cfg(test)]
#[path = "repository_tests.rs"]
mod tests;
