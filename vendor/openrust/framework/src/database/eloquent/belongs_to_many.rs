//! 通过中间表批量加载关联模型和各自的 Pivot 数据。

use super::{
    HasRelationships, HydrateModel, LoadedRelation, ModelAttributes, ModelMetadata, RelationLoad,
    SaveContext, SaveModel, has_many::relation_key, refresh::hydrate_retrieved,
    relations::load_requested,
};
use crate::{
    Db, Error, Result,
    database::{DatabaseTableNames, JoinKind, QueryBuilder, RawSql, TransactionContext},
};
use serde_json::{Map, Value};

#[path = "pivot_writes.rs"]
mod pivot_writes;
pub use pivot_writes::{PivotAttachment, PivotChanges, PivotToggleChanges};
use pivot_writes::{
    PivotDeleter, PivotInserter, PivotUpdater, delete_custom_pivot, insert_custom_pivot,
    update_custom_pivot,
};
use std::{
    collections::{BTreeMap, HashMap, btree_map::Entry},
    sync::Arc,
};

/// 关联模型从中间表读取的字段。
pub struct Pivot(Map<String, Value>);

impl Pivot {
    /// 返回中间表字段，包括两侧外键。
    pub fn attributes(&self) -> &Map<String, Value> {
        &self.0
    }
}

/// 多对多关系的表名与两侧键。
pub struct PivotKeys<'a> {
    /// 中间表逻辑名称。
    pub table: &'a str,
    /// 中间表引用当前模型的列。
    pub parent_foreign_key: &'a str,
    /// 中间表引用关联模型的列。
    pub related_foreign_key: &'a str,
    /// 当前模型被引用的列。
    pub parent_key: &'a str,
    /// 关联模型被引用的列。
    pub related_key: &'a str,
}

/// 关系构造自定义中间表模型时提供的表、键和父模型声明。
pub struct PivotSource<'a> {
    /// 中间表及关系两侧的键。
    pub keys: &'a PivotKeys<'a>,
    /// 父模型声明，提供中间表模型继承的连接及默认时间列。
    pub parent: &'a ModelMetadata,
    /// 关系通过 `with_timestamps` 显式声明的创建、更新时间列。
    pub timestamps: Option<(&'a str, &'a str)>,
}

/// 通过关系声明从中间表原始行构造实际的自定义模型。
pub trait CustomPivot: SaveModel + Sized {
    /// 从已同步原值的属性构造模型。使用关系提供的表、父模型连接和持久化状态。
    fn from_raw_pivot(
        attributes: ModelAttributes,
        source: PivotSource<'_>,
        exists: bool,
    ) -> Result<Self>;

    /// 将中间表写入输入转为模型原始值，由自定义类型保留列的精度与空值语义。
    fn raw_pivot_attribute(name: &str, value: crate::database::sea_orm::Value) -> Result<Value>;

    /// 将自定义模型标为已删除，在 deleted 事件派发前调用。
    fn mark_pivot_deleted(&mut self);
}

type PivotHydrator = fn(ModelAttributes, PivotSource<'_>) -> Result<LoadedRelation>;

#[derive(Clone, Copy)]
struct CustomPivotOperations {
    hydrate: PivotHydrator,
    insert: PivotInserter,
    update: PivotUpdater,
    delete: PivotDeleter,
}

fn hydrate_custom_pivot<P: CustomPivot>(
    attributes: ModelAttributes,
    source: PivotSource<'_>,
) -> Result<LoadedRelation> {
    Ok(LoadedRelation::Pivot(Box::new(P::from_raw_pivot(
        attributes, source, true,
    )?)))
}

/// 通过中间表关联多个模型。
pub struct BelongsToMany<'a, R> {
    related: &'a R,
    keys: PivotKeys<'a>,
    columns: Vec<&'a str>,
    accessor: &'a str,
    timestamps: Option<(&'a str, &'a str)>,
    touch_related_on_write: bool,
    touch_parent_on_write: bool,
    custom_pivot: Option<CustomPivotOperations>,
}

impl<'a, R: HydrateModel> BelongsToMany<'a, R> {
    /// 声明中间表及键，默认只读取两侧外键。
    pub fn new(related: &'a R, keys: PivotKeys<'a>) -> Self {
        let columns = vec![keys.parent_foreign_key, keys.related_foreign_key];
        Self {
            related,
            keys,
            columns,
            accessor: "pivot",
            timestamps: None,
            touch_related_on_write: false,
            touch_parent_on_write: false,
            custom_pivot: None,
        }
    }

    /// 追加读取中间表字段，保留已声明的两侧外键。
    #[must_use]
    pub fn with_pivot(mut self, columns: &[&'a str]) -> Self {
        for &column in columns {
            if !self.columns.contains(&column) {
                self.columns.push(column);
            }
        }
        self
    }

    /// 自动维护并读取中间表时间戳，列名由中间表迁移声明。
    pub fn with_timestamps(self, created_at: &'a str, updated_at: &'a str) -> Result<Self> {
        if created_at.is_empty()
            || updated_at.is_empty()
            || created_at.eq_ignore_ascii_case(updated_at)
        {
            return Err(Error::custom(
                "model_pivot_timestamp_columns_invalid",
                "中间表时间戳列必须非空且互异",
            ));
        }
        if [created_at, updated_at].into_iter().any(|column| {
            column.eq_ignore_ascii_case(self.keys.parent_foreign_key)
                || column.eq_ignore_ascii_case(self.keys.related_foreign_key)
        }) {
            return Err(Error::custom(
                "model_pivot_key_override",
                "时间戳列不能覆盖关系键",
            ));
        }
        let mut relation = self.with_pivot(&[created_at, updated_at]);
        relation.timestamps = Some((created_at, updated_at));
        Ok(relation)
    }

    /// 写入中间表后更新当前仍关联的模型时间戳。
    #[must_use]
    pub fn with_touching_related(mut self) -> Self {
        self.touch_related_on_write = true;
        self
    }

    /// 声明关联模型的反向关系要求触碰父模型，写入后走父模型的正常保存流程。
    #[must_use]
    pub fn with_touching_parent(mut self) -> Self {
        self.touch_parent_on_write = true;
        self
    }

    /// 指定用于水合和保存的自定义中间表模型。
    #[must_use]
    pub fn using<P: CustomPivot>(mut self) -> Self {
        self.custom_pivot = Some(CustomPivotOperations {
            hydrate: hydrate_custom_pivot::<P>,
            insert: insert_custom_pivot::<P>,
            update: update_custom_pivot::<P>,
            delete: delete_custom_pivot::<P>,
        });
        self
    }

    /// 设置中间表值在关联模型中的关系名称。
    #[must_use]
    pub fn with_pivot_accessor(mut self, name: &'a str) -> Self {
        self.accessor = name;
        self
    }

    /// 使用一次联结查询批量加载模型；加载后可继续批量解析子路径。
    ///
    /// 查询约束可添加筛选及排序，应保留关联模型与中间表投影。两个模型
    /// 通过同一关联行加载时，各自拥有其行的 Pivot 值。
    pub async fn load<'q, M: HasRelationships>(
        &self,
        parents: &mut [M],
        relation: RelationLoad<'_>,
        context: &SaveContext<'_>,
        constrain: impl FnOnce(QueryBuilder<'q, 'q>) -> Result<QueryBuilder<'q, 'q>>,
    ) -> Result<()> {
        self.load_using(parents, relation, context, None, None, constrain)
            .await
    }

    /// 使用调用方已选择的数据库加载当前关系，例如使用模块固定物理表名。
    pub async fn load_on<'q, M: HasRelationships>(
        &self,
        parents: &mut [M],
        relation: RelationLoad<'_>,
        context: &SaveContext<'_>,
        database: &Db,
        constrain: impl FnOnce(QueryBuilder<'q, 'q>) -> Result<QueryBuilder<'q, 'q>>,
    ) -> Result<()> {
        self.load_using(parents, relation, context, Some(database), None, constrain)
            .await
    }

    /// 在同一已核验事务中加载中间表、关联模型及后续关系。
    ///
    /// 关联模型所选连接必须是该事务的来源，且关联模型的 retrieved 事件使用同一事务。
    pub async fn load_in_transaction<'q, M: HasRelationships>(
        &self,
        parents: &mut [M],
        relation: RelationLoad<'_>,
        context: &SaveContext<'_>,
        transaction: &TransactionContext<'q>,
        constrain: impl FnOnce(QueryBuilder<'q, 'q>) -> Result<QueryBuilder<'q, 'q>>,
    ) -> Result<()> {
        self.load_using(
            parents,
            relation,
            context,
            None,
            Some(transaction),
            constrain,
        )
        .await
    }

    async fn load_using<'q, M: HasRelationships>(
        &self,
        parents: &mut [M],
        relation: RelationLoad<'_>,
        context: &SaveContext<'_>,
        database: Option<&Db>,
        transaction: Option<&TransactionContext<'q>>,
        constrain: impl FnOnce(QueryBuilder<'q, 'q>) -> Result<QueryBuilder<'q, 'q>>,
    ) -> Result<()> {
        let mut keys = Vec::with_capacity(parents.len());
        let mut bindings = BTreeMap::new();
        let mut parent_metadata = BTreeMap::new();
        for parent in &*parents {
            let raw = parent
                .attributes()
                .get_attribute(parent.metadata(), self.keys.parent_key)?;
            let key = relation_key(raw.as_ref())?;
            if let (Some(key), Some(raw)) = (&key, raw)
                && let Entry::Vacant(entry) = bindings.entry(key.clone())
            {
                entry.insert(parent.bind_attribute(self.keys.parent_key, &raw)?);
                parent_metadata.insert(key.clone(), parent.metadata());
            }
            keys.push(key);
        }
        let mut grouped = HashMap::<String, Vec<Arc<R>>>::new();
        if !bindings.is_empty() {
            let fallback = parents
                .first()
                .and_then(|parent| parent.metadata().connection.as_deref());
            let metadata = self.related.metadata();
            if database.is_none()
                && metadata.connection.is_none()
                && parents
                    .iter()
                    .any(|parent| parent.metadata().connection.as_deref() != fallback)
            {
                return Err(Error::custom(
                    "model_relation_connection_mismatch",
                    "批量关系加载需要一致的父模型连接",
                ));
            }
            let db = match database {
                Some(database) => database.clone(),
                None => metadata.resolve_connection(context.databases, fallback)?,
            };
            let connection = metadata.connection.as_deref().or(fallback);
            if transaction.is_some_and(|transaction| !transaction.is_from(&db)) {
                return Err(Error::custom(
                    "model_relation_transaction_source_mismatch",
                    "关系查询事务不属于关联模型连接",
                ));
            }
            let database_transaction = transaction.map(TransactionContext::transaction);
            let query = constrain(self.query(&db, bindings.into_values().collect())?)?;
            let query = match database_transaction {
                Some(transaction) => query.using_transaction(transaction),
                None => query,
            };
            let rows = query.get().await?;
            let mut related = Vec::with_capacity(rows.len());
            let mut parent_keys = Vec::with_capacity(rows.len());
            for row in rows {
                let mut model =
                    hydrate_retrieved(self.related, context, row, connection, database_transaction)
                        .await?;
                parent_keys.push(self.install_pivot(&mut model, &parent_metadata)?);
                related.push(model);
            }
            load_requested(&mut related, relation.nested, context, transaction).await?;
            for (key, model) in parent_keys.into_iter().zip(related) {
                grouped.entry(key).or_default().push(Arc::new(model));
            }
        }
        for (parent, key) in parents.iter_mut().zip(keys) {
            let models = key
                .as_ref()
                .and_then(|key| grouped.get(key))
                .cloned()
                .unwrap_or_default();
            parent
                .relations_mut()
                .set_relation(relation.name, LoadedRelation::Related(Box::new(models)));
        }
        Ok(())
    }

    fn query<'q>(
        &self,
        db: &Db,
        bindings: Vec<crate::database::sea_orm::Value>,
    ) -> Result<QueryBuilder<'q, 'q>> {
        let backend = db.backend()?;
        let identifiers = DatabaseTableNames::default();
        let related_table = &self.related.metadata().table;
        let related = db.quoted_table_name(related_table)?;
        let pivot = db.quoted_table_name(self.keys.table)?;
        let foreign = identifiers.quote(self.keys.parent_foreign_key, backend)?;
        let placeholders = std::iter::repeat_n("?", bindings.len())
            .collect::<Vec<_>>()
            .join(", ");
        let mut query = db
            .table(related_table)?
            .join(
                self.keys.table,
                self.keys.related_key,
                self.keys.related_foreign_key,
                JoinKind::Inner,
            )?
            .where_raw(
                RawSql::new(format!("{pivot}.{foreign} IN ({placeholders})")),
                bindings,
            )
            .select_raw(RawSql::new(format!("{related}.*")), Vec::new())?;
        for column in &self.columns {
            let alias = identifiers.quote(&format!("pivot_{column}"), backend)?;
            let column = identifiers.quote(column, backend)?;
            query = query.select_raw(
                RawSql::new(format!("{pivot}.{column} AS {alias}")),
                Vec::new(),
            )?;
        }
        Ok(query)
    }

    fn install_pivot(
        &self,
        model: &mut R,
        parents: &BTreeMap<String, &ModelMetadata>,
    ) -> Result<String> {
        let mut raw = model.attributes().raw_attributes()?;
        let mut pivot = Map::new();
        for &column in &self.columns {
            let value = raw.remove(&format!("pivot_{column}")).ok_or_else(|| {
                Error::custom("model_pivot_column_missing", "查询缺少所需中间表字段")
            })?;
            pivot.insert(column.to_owned(), value);
        }
        let key = relation_key(pivot.get(self.keys.parent_foreign_key))?
            .ok_or_else(|| Error::custom("model_pivot_key_missing", "查询缺少中间表父键"))?;
        model.attributes().set_raw_attributes(raw)?;
        let pivot = if let Some(custom) = self.custom_pivot {
            let parent = parents.get(&key).ok_or_else(|| {
                Error::custom("model_pivot_parent_missing", "找不到中间表记录对应的父模型")
            })?;
            (custom.hydrate)(
                ModelAttributes::new(pivot),
                PivotSource {
                    keys: &self.keys,
                    parent,
                    timestamps: self.timestamps,
                },
            )?
        } else {
            LoadedRelation::Pivot(Box::new(Pivot(pivot)))
        };
        model.relations_mut().set_relation(self.accessor, pivot);
        Ok(key)
    }
}
