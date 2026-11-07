//! 通过当前模型的外键加载所属模型。

use super::{HasMany, HasRelationships, HydrateModel, LoadedRelation, RelationLoad, SaveContext};
use crate::{
    Db, Result,
    database::{QueryBuilder, TransactionContext},
};
use serde_json::Value;
use std::sync::Arc;

/// 当前模型通过本地外键引用的单个所属模型。
pub struct BelongsTo<'a, R>(HasMany<'a, R>);

impl<'a, R: HydrateModel> BelongsTo<'a, R> {
    /// 构造反向关系，省略所属键时使用所属模型声明的主键。
    pub fn new(owner: &'a R, foreign_key: &'a str, owner_key: Option<&'a str>) -> Self {
        let owner_key = owner_key.unwrap_or(owner.metadata().key_name.as_str());
        Self(HasMany::inverse(owner, owner_key, foreign_key))
    }

    /// 使用所属模型的键更新外键，并把同一所属模型写入已加载关系。
    ///
    /// 外键经模型属性 setter 赋值。关系状态只在赋值成功后改变。调用者另行保存模型。
    pub async fn associate<'m, M: HasRelationships>(
        &self,
        model: &'m mut M,
        name: &str,
        owner: Arc<R>,
    ) -> Result<&'m mut M> {
        let (_, owner_key) = self.0.relation_keys();
        let key = owner
            .attributes()
            .get_attribute(owner.metadata(), owner_key)?
            .unwrap_or(Value::Null);
        self.set_foreign_key(model, key).await?;
        model
            .relations_mut()
            .set_relation(name, LoadedRelation::Related(Box::new(owner)));
        Ok(model)
    }

    /// 直接设置外键值，并清除该关系的已加载状态。调用者另行保存模型。
    pub async fn associate_key<'m, M: HasRelationships>(
        &self,
        model: &'m mut M,
        name: &str,
        key: Value,
    ) -> Result<&'m mut M> {
        self.set_foreign_key(model, key).await?;
        model.relations_mut().unset_relation(name);
        Ok(model)
    }

    /// 将外键置空，并把关系标记为已加载的空值。调用者另行保存模型。
    pub async fn dissociate<'m, M: HasRelationships>(
        &self,
        model: &'m mut M,
        name: &str,
    ) -> Result<&'m mut M> {
        self.set_foreign_key(model, Value::Null).await?;
        model
            .relations_mut()
            .set_relation(name, LoadedRelation::Null);
        Ok(model)
    }

    /// `dissociate` 的别名。
    pub async fn disassociate<'m, M: HasRelationships>(
        &self,
        model: &'m mut M,
        name: &str,
    ) -> Result<&'m mut M> {
        self.dissociate(model, name).await
    }

    async fn set_foreign_key<M: HasRelationships>(&self, model: &M, key: Value) -> Result<()> {
        let (foreign_key, _) = self.0.relation_keys();
        model
            .attributes()
            .set_attribute(model.metadata(), foreign_key, key)
            .await
    }

    /// 批量查找所属模型，保存 `Arc<R>` 或已加载空值。
    ///
    /// 对有效外键去重后使用同一条查询，外键为空的模型不产生查询条件。
    pub async fn load<'q, M: HasRelationships>(
        &self,
        models: &mut [M],
        name: &str,
        context: &SaveContext<'_>,
        constrain: impl FnOnce(QueryBuilder<'q, 'q>) -> Result<QueryBuilder<'q, 'q>>,
    ) -> Result<()> {
        self.0.load(models, name, context, constrain).await
    }

    /// 批量加载所属模型，为没有匹配的子模型保留工厂提供的默认实例。
    ///
    /// 工厂接收所属模型原型、当前子模型及连接。默认所属模型不预填子模型外键，
    /// 也不经数据库水合或派发 retrieved。匹配到真实所属模型时以查询结果为准。
    pub async fn load_with_default<'q, M: HasRelationships>(
        &self,
        models: &mut [M],
        relation: RelationLoad<'_>,
        context: &SaveContext<'_>,
        transaction: Option<&TransactionContext<'q>>,
        constrain: impl FnOnce(QueryBuilder<'q, 'q>) -> Result<QueryBuilder<'q, 'q>>,
        make_default: impl FnMut(&R, &M, Option<&str>) -> Result<R>,
    ) -> Result<()> {
        self.0
            .load_defaulting(
                models,
                relation,
                context,
                transaction,
                constrain,
                make_default,
            )
            .await
    }

    /// 在调用者选择的连接及表名映射上加载所属模型。
    pub async fn load_on<'q, M: HasRelationships>(
        &self,
        models: &mut [M],
        name: &str,
        context: &SaveContext<'_>,
        database: &Db,
        constrain: impl FnOnce(QueryBuilder<'q, 'q>) -> Result<QueryBuilder<'q, 'q>>,
    ) -> Result<()> {
        self.0
            .load_on(models, name, context, database, constrain)
            .await
    }

    /// 加载反向关系及其子路径，可使用已核验来源的事务。
    pub async fn load_with<'q, M: HasRelationships>(
        &self,
        models: &mut [M],
        relation: RelationLoad<'_>,
        context: &SaveContext<'_>,
        transaction: Option<&TransactionContext<'q>>,
        constrain: impl FnOnce(QueryBuilder<'q, 'q>) -> Result<QueryBuilder<'q, 'q>>,
    ) -> Result<()> {
        self.0
            .load_with(models, relation, context, transaction, constrain)
            .await
    }
}
