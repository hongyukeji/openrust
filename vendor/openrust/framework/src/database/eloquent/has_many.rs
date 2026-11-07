//! Eloquent 一对一及一对多关系的批量加载。

use super::{
    HydrateModel, LoadedRelation, ModelRelations, RelationLoad, SaveContext, SaveModel,
    one_of_many::{self, CandidateFilter, RelationAggregate},
    refresh::hydrate_retrieved,
    relations::load_requested,
};
use crate::{
    Db, Error, Result,
    database::{QueryBuilder, TransactionContext, WhereGroup},
};
use serde_json::{Map, Value};
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
    cardinality: Cardinality,
    aggregate: Vec<(&'a str, RelationAggregate)>,
    candidate_filter: Option<Box<CandidateFilter<'a>>>,
}

#[derive(Clone, Copy)]
enum Cardinality {
    One,
    Many,
}

/// 根据父模型本地键和关联模型外键加载单个关联模型。
pub struct HasOne<'a, R>(HasMany<'a, R>);

impl<'a, R: HydrateModel> HasOne<'a, R> {
    /// 从关联模型原型和显式列名构造一对一关系。
    pub const fn new(related: &'a R, foreign_key: &'a str, local_key: &'a str) -> Self {
        Self(HasMany {
            related,
            foreign_key,
            local_key,
            cardinality: Cardinality::One,
            aggregate: Vec::new(),
            candidate_filter: None,
        })
    }

    /// 按优先级指定聚合列，未显式列出主键时以最大主键打破并列。
    pub fn of_many(mut self, columns: &[(&'a str, RelationAggregate)]) -> Result<Self> {
        let mut unique = std::collections::BTreeSet::new();
        if !columns.iter().all(|(column, _)| unique.insert(*column)) {
            return Err(Error::custom(
                "model_relation_aggregate_duplicate",
                "聚合列不可重复",
            ));
        }
        self.0.aggregate = columns.to_vec();
        let related: &'a R = self.0.related;
        let key = related.metadata().key_name.as_str();
        if !unique.contains(key) {
            self.0.aggregate.push((key, RelationAggregate::Max));
        }
        Ok(self)
    }

    /// 单列形式，按相同聚合方向以主键打破并列。
    #[must_use]
    pub fn of_many_by(mut self, column: &'a str, aggregate: RelationAggregate) -> Self {
        self.0.aggregate = vec![(column, aggregate)];
        let related: &'a R = self.0.related;
        let key = related.metadata().key_name.as_str();
        if column != key {
            self.0.aggregate.push((key, aggregate));
        }
        self
    }

    /// 按最大列值选取记录，默认使用关联模型主键。
    pub fn latest_of_many(self, column: Option<&'a str>) -> Result<Self> {
        let related: &'a R = self.0.related;
        let column = column.unwrap_or(related.metadata().key_name.as_str());
        self.of_many(&[(column, RelationAggregate::Max)])
    }

    /// 按最小列值选取记录，默认使用关联模型主键。
    pub fn oldest_of_many(self, column: Option<&'a str>) -> Result<Self> {
        let related: &'a R = self.0.related;
        let column = column.unwrap_or(related.metadata().key_name.as_str());
        self.of_many(&[(column, RelationAggregate::Min)])
    }

    /// 为每一步聚合设置候选行的查询条件。回调内使用关联表自身的列。
    ///
    /// 筛选条件通过现有 Query Builder 绑定，不改变外层查询的筛选与排序。
    #[must_use]
    pub fn where_candidates(
        mut self,
        filter: impl for<'q> Fn(WhereGroup<'q, 'q>) -> Result<WhereGroup<'q, 'q>> + Send + Sync + 'a,
    ) -> Self {
        self.0.candidate_filter = Some(Box::new(filter));
        self
    }

    /// 批量匹配首个关联模型，结果为 `Arc<R>`。无匹配或父键为空时保存已加载空值。
    ///
    /// 当多行匹配同一个父模型时，第一行由关联查询的结果顺序决定。
    pub async fn load<'q, P: HasRelationships>(
        &self,
        parents: &mut [P],
        name: &str,
        context: &SaveContext<'_>,
        constrain: impl FnOnce(QueryBuilder<'q, 'q>) -> Result<QueryBuilder<'q, 'q>>,
    ) -> Result<()> {
        self.0.load(parents, name, context, constrain).await
    }

    /// 为每个父模型构造默认实例，再以查得的首个关联模型覆盖默认结果。
    ///
    /// 工厂接收关联模型原型、父模型、已写入外键的原始属性以及选定的连接声明。
    /// 普通默认值应由工厂构造为未持久化实例。默认实例不经过数据库水合或 retrieved。
    pub async fn load_with_default<'q, P: HasRelationships>(
        &self,
        parents: &mut [P],
        relation: RelationLoad<'_>,
        context: &SaveContext<'_>,
        transaction: Option<&TransactionContext<'q>>,
        constrain: impl FnOnce(QueryBuilder<'q, 'q>) -> Result<QueryBuilder<'q, 'q>>,
        mut make_default: impl FnMut(&R, &P, Map<String, Value>, Option<&str>) -> Result<R>,
    ) -> Result<()> {
        let mut defaults = Vec::with_capacity(parents.len());
        for parent in &*parents {
            let key = parent
                .attributes()
                .get_attribute(parent.metadata(), self.0.local_key)?;
            let raw = Map::from_iter([(self.0.foreign_key.to_owned(), key.unwrap_or(Value::Null))]);
            let connection = self
                .0
                .related
                .metadata()
                .connection
                .as_deref()
                .or(parent.metadata().connection.as_deref());
            defaults.push(make_default(self.0.related, parent, raw, connection)?);
        }
        self.load_with(parents, relation, context, transaction, constrain)
            .await?;
        for (parent, default) in parents.iter_mut().zip(defaults) {
            if matches!(
                parent.relations().get_relation(relation.name),
                Some(LoadedRelation::Null)
            ) {
                parent.relations_mut().set_relation(
                    relation.name,
                    LoadedRelation::Related(Box::new(Arc::new(default))),
                );
            }
        }
        Ok(())
    }

    /// 在调用者选择的连接与表名映射上批量加载单个关联结果。
    ///
    /// 调用者须提供与关联模型声明一致的数据库，查询约束保留框架的外键条件。
    pub async fn load_on<'q, P: HasRelationships>(
        &self,
        parents: &mut [P],
        name: &str,
        context: &SaveContext<'_>,
        database: &Db,
        constrain: impl FnOnce(QueryBuilder<'q, 'q>) -> Result<QueryBuilder<'q, 'q>>,
    ) -> Result<()> {
        self.0
            .load_on(parents, name, context, database, constrain)
            .await
    }

    /// 批量加载单个关联结果，支持对查得的关联模型加载后续路径及传递事务。
    pub async fn load_with<'q, P: HasRelationships>(
        &self,
        parents: &mut [P],
        relation: RelationLoad<'_>,
        context: &SaveContext<'_>,
        transaction: Option<&TransactionContext<'q>>,
        constrain: impl FnOnce(QueryBuilder<'q, 'q>) -> Result<QueryBuilder<'q, 'q>>,
    ) -> Result<()> {
        self.0
            .load_with(parents, relation, context, transaction, constrain)
            .await
    }
}

impl<'a, R: HydrateModel> HasMany<'a, R> {
    /// 从关联模型原型和显式列名构造关系。
    pub const fn new(related: &'a R, foreign_key: &'a str, local_key: &'a str) -> Self {
        Self {
            related,
            foreign_key,
            local_key,
            cardinality: Cardinality::Many,
            aggregate: Vec::new(),
            candidate_filter: None,
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
            let table = &self.related.metadata().table;
            let query = db
                .table(table)?
                .where_in(self.foreign_key, bindings.into_values())?;
            let query = match database_transaction {
                Some(transaction) => query.using_transaction(transaction),
                None => query,
            };
            let query = if self.aggregate.is_empty() {
                query
            } else {
                one_of_many::constrain(
                    &db,
                    table,
                    self.foreign_key,
                    &self.aggregate,
                    query,
                    database_transaction,
                    self.candidate_filter.as_deref(),
                )?
            };
            let query = constrain(query)?;
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
            let matches = key.as_ref().and_then(|key| children.get(key));
            let loaded = match self.cardinality {
                Cardinality::One => matches
                    .and_then(|items| items.first())
                    .map_or(LoadedRelation::Null, |item| {
                        LoadedRelation::Related(Box::new(Arc::clone(item)))
                    }),
                Cardinality::Many => {
                    LoadedRelation::Related(Box::new(matches.cloned().unwrap_or_default()))
                }
            };
            parent.relations_mut().set_relation(relation.name, loaded);
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
