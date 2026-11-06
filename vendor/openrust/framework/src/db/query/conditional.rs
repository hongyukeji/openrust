//! 根据查询存在性惰性执行一次异步回调。

use super::{QueryBuilder, Result};
use std::future::Future;

/// 存在性条件终结操作的短路结果或回调返回值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExistsOrResult<T> {
    /// 查询满足短路条件，未调用回调或创建其 Future。
    True,
    /// 查询不满足短路条件，保留异步回调成功返回的值。
    Callback(T),
}

impl QueryBuilder<'_, '_> {
    /// 存在行时返回 `True`，否则执行一次异步回调并保留其结果。
    ///
    /// 只执行一次存在性查询；查询失败不调用回调，回调错误原样返回。
    pub async fn exists_or<F, Fut, T>(&self, callback: F) -> Result<ExistsOrResult<T>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        if self.exists().await? {
            Ok(ExistsOrResult::True)
        } else {
            callback().await.map(ExistsOrResult::Callback)
        }
    }

    /// 不存在行时返回 `True`，否则执行一次异步回调并保留其结果。
    ///
    /// 只执行一次不存在性查询；查询失败不调用回调，回调错误原样返回。
    pub async fn doesnt_exist_or<F, Fut, T>(&self, callback: F) -> Result<ExistsOrResult<T>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        if self.doesnt_exist().await? {
            Ok(ExistsOrResult::True)
        } else {
            callback().await.map(ExistsOrResult::Callback)
        }
    }
}

#[cfg(all(test, feature = "db-sqlite"))]
#[path = "conditional_tests.rs"]
mod tests;
