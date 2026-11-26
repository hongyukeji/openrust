use std::{future::Future, marker::PhantomData};

use crate::{
    database::{DatabaseResult, Value},
    pagination::{LengthAwarePaginator, PageOptions},
};

use super::{Attributes, Builder, Model};

/// 基于 Eloquent 的仓储。覆盖 `query` 可选择连接或限定读取范围。
/// 创建沿用 Builder 的模型属性语义，读取条件不会为新模型赋值。
/// 更新和删除使用调用方已选定的模型，保留实例事件和软删除语义。
pub trait Repository: Sync {
    type Model: Model;

    fn query(&self) -> DatabaseResult<Builder<'static, Self::Model>> {
        Self::Model::query()
    }

    fn find(
        &self,
        key: impl Into<Value>,
    ) -> impl Future<Output = DatabaseResult<Option<Self::Model>>> + Send {
        let key = key.into();
        async move { self.query()?.find(key).await }
    }

    fn find_or_fail(
        &self,
        key: impl Into<Value>,
    ) -> impl Future<Output = DatabaseResult<Self::Model>> + Send {
        let key = key.into();
        async move { self.query()?.find_or_fail(key).await }
    }

    fn create(
        &self,
        attributes: Attributes,
    ) -> impl Future<Output = DatabaseResult<Self::Model>> + Send {
        async move { self.query()?.create(attributes).await }
    }

    fn update(
        &self,
        model: &mut Self::Model,
        attributes: Attributes,
    ) -> impl Future<Output = DatabaseResult<bool>> + Send {
        model.update(attributes)
    }

    fn delete(&self, model: &mut Self::Model) -> impl Future<Output = DatabaseResult<bool>> + Send {
        model.delete()
    }

    /// 保留调用方在查询上已设置的筛选和排序。
    fn paginate(
        &self,
        query: Builder<'static, Self::Model>,
        options: PageOptions<'_>,
    ) -> impl Future<Output = DatabaseResult<LengthAwarePaginator<Self::Model>>> + Send {
        query.paginate_with(options)
    }
}

/// 直接使用模型默认查询的无状态仓储。
pub struct EloquentRepository<M>(PhantomData<fn() -> M>);

impl<M: Model> EloquentRepository<M> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<M: Model> Default for EloquentRepository<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: Model> Repository for EloquentRepository<M> {
    type Model = M;
}
