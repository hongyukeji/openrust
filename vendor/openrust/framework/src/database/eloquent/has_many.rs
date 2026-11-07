//! Eloquent 一对多关系的批量加载。

use super::{
    HydrateModel, LoadedRelation, ModelRelations, RelationLoad, SaveContext, SaveModel,
    refresh::hydrate_retrieved, relations::load_requested,
};
use crate::{
    Db, Error, Result,
    database::{QueryBuilder, TransactionContext},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap, btree_map::Entry},
    sync::Arc,
};

/// 持有实例级已加载关系的模型。
pub trait HasRelationships: SaveModel {
    /// 借用已加载的关系。
    fn relations(&self) -> &ModelRelations;

    /// 更新本实例的关系加载状态。
    fn relations_mut(&mut self) -> &mut ModelRelations;
}

/// 根据父模型本地键和关联模型外键加载一对多关系。
pub struct HasMany<'a, R> {
    related: &'a R,
    foreign_key: &'a str,
    local_key: &'a str,
}

impl<'a, R: HydrateModel> HasMany<'a, R> {
    /// 从关联模型原型和显式列名构造关系。
    pub const fn new(related: &'a R, foreign_key: &'a str, local_key: &'a str) -> Self {
        Self {
            related,
            foreign_key,
            local_key,
        }
    }

    /// 批量加载指定关系，使用一条查询获取子行并按外键匹配。
    ///
    /// 对每个父模型设置 `Vec<Arc<R>>`，缺少匹配时保存空集合。支持整数与字符串键。
    /// 当关联模型没有显式连接时，父模型必须共享一个连接声明。查询约束可添加过滤和排序。
    pub async fn load<'q, P: HasRelationships>(
        &self,
        parents: &mut [P],
        name: &str,
        context: &SaveContext<'_>,
        constrain: impl FnOnce(QueryBuilder<'q, 'q>) -> Result<QueryBuilder<'q, 'q>>,
    ) -> Result<()> {
        self.load_with(
            parents,
            RelationLoad { name, nested: &[] },
            context,
            None,
            constrain,
        )
        .await
    }

    /// 在调用者的事务内加载同一连接上的关系，并把该事务传给关联模型事件。
    ///
    /// 关联模型所选句柄必须是事务来源。不同连接的关系应分别加载。
    pub async fn load_in_transaction<'q, P: HasRelationships>(
        &self,
        parents: &mut [P],
        name: &str,
        context: &SaveContext<'_>,
        transaction: &TransactionContext<'q>,
        constrain: impl FnOnce(QueryBuilder<'q, 'q>) -> Result<QueryBuilder<'q, 'q>>,
    ) -> Result<()> {
        self.load_with(
            parents,
            RelationLoad { name, nested: &[] },
            context,
            Some(transaction),
            constrain,
        )
        .await
    }

    /// 批量加载关系，并在匹配父模型前对整个子模型集合加载后续路径。
    pub async fn load_with<'q, P: HasRelationships>(
        &self,
        parents: &mut [P],
        relation: RelationLoad<'_>,
        context: &SaveContext<'_>,
        transaction: Option<&TransactionContext<'q>>,
        constrain: impl FnOnce(QueryBuilder<'q, 'q>) -> Result<QueryBuilder<'q, 'q>>,
    ) -> Result<()> {
        self.load_from(
            parents,
            relation,
            context,
            transaction,
            constrain,
            |parents| {
                let fallback = parents
                    .first()
                    .and_then(|p| p.metadata().connection.as_deref());
                let metadata = self.related.metadata();
                if metadata.connection.is_none()
                    && parents
                        .iter()
                        .any(|p| p.metadata().connection.as_deref() != fallback)
                {
                    return Err(Error::custom(
                        "model_relation_connection_mismatch",
                        "批量关系加载需要一致的父模型连接",
                    ));
                }
                let connection = metadata
                    .connection
                    .as_deref()
                    .or(fallback)
                    .map(str::to_owned);
                Ok((
                    metadata.resolve_connection(context.databases, fallback)?,
                    connection,
                ))
            },
        )
        .await
    }

    /// 在调用者选定的连接与表名映射上加载关系。
    ///
    /// 调用者须提供与关联模型声明一致的数据库。该句柄可为同一连接指定不同的表名映射。
    /// 水合保留关联模型的显式连接声明；查询约束仍在框架的外键过滤之后应用。
    pub async fn load_on<'q, P: HasRelationships>(
        &self,
        parents: &mut [P],
        name: &str,
        context: &SaveContext<'_>,
        database: &Db,
        constrain: impl FnOnce(QueryBuilder<'q, 'q>) -> Result<QueryBuilder<'q, 'q>>,
    ) -> Result<()> {
        self.load_from(
            parents,
            RelationLoad { name, nested: &[] },
            context,
            None,
            constrain,
            |_| Ok((database.clone(), self.related.metadata().connection.clone())),
        )
        .await
    }

    async fn load_from<'q, P: HasRelationships>(
        &self,
        parents: &mut [P],
        relation: RelationLoad<'_>,
        context: &SaveContext<'_>,
        transaction: Option<&TransactionContext<'q>>,
        constrain: impl FnOnce(QueryBuilder<'q, 'q>) -> Result<QueryBuilder<'q, 'q>>,
        resolve: impl FnOnce(&[P]) -> Result<(Db, Option<String>)>,
    ) -> Result<()> {
        let mut bindings = BTreeMap::new();
        let mut keys = Vec::with_capacity(parents.len());
        for parent in &*parents {
            let value = parent
                .attributes()
                .get_attribute(parent.metadata(), self.local_key)?;
            let key = relation_key(value.as_ref())?;
            if let (Some(key), Some(value)) = (&key, value)
                && let Entry::Vacant(entry) = bindings.entry(key.clone())
            {
                entry.insert(self.related.bind_attribute(self.foreign_key, &value)?);
            }
            keys.push(key);
        }

        let mut children = HashMap::<_, Vec<_>>::new();
        if !bindings.is_empty() {
            let (db, connection) = resolve(parents)?;
            if transaction.is_some_and(|transaction| !transaction.is_from(&db)) {
                return Err(Error::custom(
                    "model_relation_transaction_source_mismatch",
                    "关系查询事务不属于关联模型连接",
                ));
            }
            let database_transaction = transaction.map(TransactionContext::transaction);
            let query = constrain(
                db.table(&self.related.metadata().table)?
                    .where_in(self.foreign_key, bindings.into_values())?,
            )?;
            let query = match database_transaction {
                Some(transaction) => query.using_transaction(transaction),
                None => query,
            };
            let rows = query.get().await?;
            let mut related = Vec::with_capacity(rows.len());
            for row in rows {
                related.push(
                    hydrate_retrieved(
                        self.related,
                        context,
                        row,
                        connection.as_deref(),
                        database_transaction,
                    )
                    .await?,
                );
            }
            load_requested(&mut related, relation.nested, context, transaction).await?;
            for child in related {
                let value = child
                    .attributes()
                    .get_attribute(child.metadata(), self.foreign_key)?;
                if let Some(key) = relation_key(value.as_ref())? {
                    children.entry(key).or_default().push(Arc::new(child));
                }
            }
        }

        for (parent, key) in parents.iter_mut().zip(keys) {
            let matches = key
                .as_ref()
                .and_then(|key| children.get(key))
                .cloned()
                .unwrap_or_default();
            parent
                .relations_mut()
                .set_relation(relation.name, LoadedRelation::Related(Box::new(matches)));
        }
        Ok(())
    }
}

fn relation_key(value: Option<&Value>) -> Result<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(Value::Number(value)) if value.is_i64() || value.is_u64() => {
            Ok(Some(value.to_string()))
        }
        _ => Err(Error::custom(
            "model_relation_key_type",
            "关系键必须为整数或字符串",
        )),
    }
}
