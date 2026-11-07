//! 同一入口 Scope 内的客户端 once 缓存，不在 singleton 仓储持久保存查询结果。

use super::{ClientRecord, Identifier};
use openrust::{Error, Result};
use std::{
    collections::HashMap,
    future::Future,
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
};
use tokio::sync::OnceCell;

pub(super) struct RepositoryIdentity;

#[derive(Clone)]
struct LookupKey {
    repository: Arc<RepositoryIdentity>,
    identifier: Identifier,
    query: Identifier,
}
impl PartialEq for LookupKey {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.repository, &other.repository)
            && self.identifier == other.identifier
            && self.query == other.query
    }
}
impl Eq for LookupKey {}
impl Hash for LookupKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.repository).hash(state);
        self.identifier.hash(state);
        self.query.hash(state);
    }
}

#[derive(Default)]
pub(crate) struct ClientLookupCache {
    entries: Mutex<HashMap<LookupKey, Arc<OnceCell<Option<ClientRecord>>>>>,
}
impl ClientLookupCache {
    pub(super) async fn find<F, Fut>(
        &self,
        repository: &Arc<RepositoryIdentity>,
        identifier: Identifier,
        initialize: F,
    ) -> Result<Option<ClientRecord>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Option<ClientRecord>>>,
    {
        self.find_query(repository, identifier.clone(), identifier, initialize)
            .await
    }

    pub(super) async fn find_query<F, Fut>(
        &self,
        repository: &Arc<RepositoryIdentity>,
        identifier: Identifier,
        query: Identifier,
        initialize: F,
    ) -> Result<Option<ClientRecord>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Option<ClientRecord>>>,
    {
        let cell = {
            let mut entries = self.entries.lock().map_err(|_| {
                Error::custom_with_status(
                    openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "passport_client_cache_unavailable",
                    "客户端入口缓存状态不可用",
                )
            })?;
            Arc::clone(
                entries
                    .entry(LookupKey {
                        repository: Arc::clone(repository),
                        identifier,
                        query,
                    })
                    .or_default(),
            )
        };
        // OnceCell 只存成功结果（包括 None）；错误或取消释放初始化权，后继可重试。
        // 用户工厂及数据库等待均发生在同步 map 锁之外。
        cell.get_or_try_init(initialize).await.cloned()
    }
}
