use std::{collections::HashSet, marker::PhantomData, mem};

use futures_util::TryStreamExt as _;
use indexmap::IndexMap;

use crate::{
    database::{Connection, DatabaseCursor, DatabaseError, DatabaseResult, Row, Value, query},
    pagination::{CursorOptions, CursorPaginator, LengthAwarePaginator, PageOptions, Paginator},
};

use super::concerns::queries_relationships::cast_existence;
use super::{
    Attributes, Collection, EagerLoads, Model, ModelState, RelationAggregate, Scope,
    casts::{AttributeCast, CastOverrides},
    concerns::{has_relationships::load_eager, has_timestamps::timestamp},
    relations::{
        BelongsTo, BelongsToMany, ConstrainableRelation, ExistenceRelation, MorphIdentity, MorphTo,
        Relation,
    },
    soft_deletes,
    soft_deleting_scope::Trashed,
};

/// 主键条件支持单值或列表；模型参数请显式传入 getKey() 的结果。
#[derive(Clone, Debug)]
pub enum Key {
    Scalar(Value),
    Many(Vec<Value>),
}

impl<T: Into<Value>> From<T> for Key {
    fn from(value: T) -> Self {
        Self::Scalar(value.into())
    }
}
impl<T: Into<Value>> From<Vec<T>> for Key {
    fn from(values: Vec<T>) -> Self {
        Self::Many(values.into_iter().map(Into::into).collect())
    }
}
impl<T: Into<Value>, const N: usize> From<[T; N]> for Key {
    fn from(values: [T; N]) -> Self {
        Self::Many(values.into_iter().map(Into::into).collect())
    }
}

fn integer_key(value: Value) -> i64 {
    use serde_json::Value as Json;
    let value = match value {
        Value::Integer(value) => return value,
        Value::Unsigned(value) => return i64::from_ne_bytes(value.to_ne_bytes()),
        Value::Real(value) => serde_json::Number::from_f64(value).map_or(Json::Null, Json::Number),
        Value::Text(value) => Json::String(value),
        Value::Bytes(value) => Json::String(String::from_utf8_lossy(&value).into_owned()),
        Value::Bool(value) => Json::Bool(value),
        Value::Json(value) => value,
        Value::Null => Json::Null,
    };
    crate::support::php_integer_cast(&value)
}

fn string_key(value: Value) -> Value {
    match value {
        Value::Integer(value) => value.to_string().into(),
        Value::Unsigned(value) => value.to_string().into(),
        Value::Real(value) => crate::support::php_float_string(value).into(),
        Value::Bool(value) => if value { "1" } else { "" }.into(),
        Value::Json(_) => "Array".into(),
        value => value,
    }
}

/// Builds model queries and hydrates results from the selected connection.
pub struct Builder<'connection, M: Model> {
    query: query::Builder<'connection>,
    pub(super) connection: Connection,
    table_alias: Option<String>,
    pivot: Option<super::relations::custom_pivot::PivotIdentity>,
    existence_attributes: HashSet<String>,
    pub(super) casts: CastOverrides,
    predicates: query::Conditions,
    pub(super) pending_attributes: Attributes,
    eager_load: EagerLoads<M>,
    default_counts: bool,
    removed_scopes: Vec<String>,
    scopes: Box<IndexMap<String, super::scope::ScopeEntry<M>>>,
    trashed: Trashed,
    model: PhantomData<M>,
}

impl<M: Model> Builder<'static, M> {
    pub(super) fn new(connection: Connection) -> Self {
        M::new_eloquent_builder(connection.table(M::metadata().table))
    }

    /// 用指定底层查询构造模型查询，供模型工厂定制默认行为。
    pub fn from_query(query: query::Builder<'static>) -> Self {
        Self {
            connection: query.get_connection().clone(),
            query,
            table_alias: None,
            pivot: None,
            existence_attributes: HashSet::new(),
            casts: CastOverrides::default(),
            predicates: query::Conditions::default(),
            pending_attributes: Attributes::new(),
            eager_load: EagerLoads::default(),
            default_counts: false,
            removed_scopes: Vec::new(),
            scopes: Box::new(
                M::metadata()
                    .deleted_at
                    .map(|_| {
                        (
                            std::any::type_name::<super::SoftDeletingScope>().into(),
                            super::scope::ScopeEntry::Static(
                                &super::SoftDeletingScope as &'static dyn Scope<M>,
                            ),
                        )
                    })
                    .into_iter()
                    .chain(M::global_scopes().iter().map(|(name, scope)| {
                        ((*name).into(), super::scope::ScopeEntry::Static(*scope))
                    }))
                    .collect(),
            ),
            trashed: Trashed::default(),
            model: PhantomData,
        }
    }
    /// Filters by the existence of a registered relation or dot-separated path.
    pub fn where_has_path(self, path: &str) -> DatabaseResult<Self> {
        self.has_path_count(path, ">=", 1)
    }

    /// Filters by the absence of a complete registered relation path.
    pub fn where_doesnt_have_path(self, path: &str) -> DatabaseResult<Self> {
        self.has_path_count(path, "<", 1)
    }

    /// Applies the count comparison at the final segment of the relation path.
    pub fn has_path_count(
        mut self,
        path: &str,
        operator: &str,
        count: u64,
    ) -> DatabaseResult<Self> {
        Self::validate_path(path)?;
        let expression = super::NamedRelations::<M>::filter(
            &self.connection,
            &self.query,
            path,
            (operator, count),
            None,
            None,
        )?;
        self.predicates.raw(false, expression);
        Ok(self)
    }

    /// Discovers stored morph aliases before filtering by a named relation path.
    pub async fn where_has_path_all(self, path: &str) -> DatabaseResult<Self> {
        self.has_path_count_all(path, ">=", 1).await
    }

    /// Discovers stored morph aliases before testing for a missing relation path.
    pub async fn where_doesnt_have_path_all(self, path: &str) -> DatabaseResult<Self> {
        self.has_path_count_all(path, "<", 1).await
    }

    /// Discovers stored morph aliases and compares the final segment's count.
    pub async fn has_path_count_all(
        mut self,
        path: &str,
        operator: &str,
        count: u64,
    ) -> DatabaseResult<Self> {
        let found = self.discover_path(path).await?;
        let expression = super::NamedRelations::<M>::filter(
            &self.connection,
            &self.query,
            path,
            (operator, count),
            Some(&found),
            None,
        )?;
        self.predicates.raw(false, expression);
        Ok(self)
    }

    pub fn where_relation_path(
        self,
        path: &str,
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.filter_path_with(
            path,
            ">=",
            false,
            &Self::path_column(column, operator, value.into()),
            None,
        )
    }

    pub fn or_where_relation_path(
        self,
        path: &str,
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.filter_path_with(
            path,
            ">=",
            true,
            &Self::path_column(column, operator, value.into()),
            None,
        )
    }

    pub fn where_doesnt_have_relation_path(
        self,
        path: &str,
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.filter_path_with(
            path,
            "<",
            false,
            &Self::path_column(column, operator, value.into()),
            None,
        )
    }

    pub fn or_where_doesnt_have_relation_path(
        self,
        path: &str,
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.filter_path_with(
            path,
            "<",
            true,
            &Self::path_column(column, operator, value.into()),
            None,
        )
    }

    /// Filters using a grouped predicate on the final relationship in the path.
    pub fn where_relation_path_with(
        self,
        path: &str,
        constraint: impl Fn(query::Conditions, &str) -> query::Conditions + Send + Sync + 'static,
    ) -> DatabaseResult<Self> {
        let constraint: super::named_relations::PathConstraint = std::sync::Arc::new(constraint);
        self.filter_path_with(path, ">=", false, &constraint, None)
    }

    pub fn with_where_relation_path(
        self,
        path: &str,
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.with_where_path(
            path,
            Self::path_column(column, operator, value.into()),
            None,
        )
    }

    /// Applies the same grouped predicate when filtering and loading the path.
    pub fn with_where_relation_path_with(
        self,
        path: &str,
        constraint: impl Fn(query::Conditions, &str) -> query::Conditions + Send + Sync + 'static,
    ) -> DatabaseResult<Self> {
        self.with_where_path(path, std::sync::Arc::new(constraint), None)
    }

    /// Filters the final relationship after discovering stored morph aliases.
    pub async fn where_relation_path_all(
        self,
        path: &str,
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        let constraint = Self::path_column(column, operator, value.into());
        let found = self.discover_path(path).await?;
        self.filter_path_with(path, ">=", false, &constraint, Some(&found))
    }

    /// Tests for the absence of a matching relationship after morph alias discovery.
    pub async fn where_doesnt_have_relation_path_all(
        self,
        path: &str,
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        let constraint = Self::path_column(column, operator, value.into());
        let found = self.discover_path(path).await?;
        self.filter_path_with(path, "<", false, &constraint, Some(&found))
    }

    /// Applies the final column predicate using the same aliases for filtering and loading.
    pub async fn with_where_relation_path_all(
        self,
        path: &str,
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        let constraint = Self::path_column(column, operator, value.into());
        let found = self.discover_path(path).await?;
        self.with_where_path(path, constraint, Some(&found))
    }

    /// Applies a grouped final predicate using the same aliases for filtering and loading.
    pub async fn with_where_relation_path_all_with(
        self,
        path: &str,
        constraint: impl Fn(query::Conditions, &str) -> query::Conditions + Send + Sync + 'static,
    ) -> DatabaseResult<Self> {
        let found = self.discover_path(path).await?;
        self.with_where_path(path, std::sync::Arc::new(constraint), Some(&found))
    }

    async fn discover_path(
        &self,
        path: &str,
    ) -> DatabaseResult<super::named_relations::DiscoveredMorphs> {
        Self::validate_path(path)?;
        let mut found = super::named_relations::DiscoveredMorphs::default();
        super::NamedRelations::<M>::discover(&self.connection, path, &mut found).await?;
        Ok(found)
    }

    fn with_where_path(
        self,
        path: &str,
        constraint: super::named_relations::PathConstraint,
        discovered: Option<&super::named_relations::DiscoveredMorphs>,
    ) -> DatabaseResult<Self> {
        let query = self.filter_path_with(path, ">=", false, &constraint, discovered)?;
        Ok(
            query.with_eager_loads(super::NamedRelations::<M>::prepare_constrained(
                &[path],
                Some(&constraint),
                discovered,
            )?),
        )
    }

    fn filter_path_with(
        mut self,
        path: &str,
        operator: &str,
        or: bool,
        constraint: &super::named_relations::PathConstraint,
        discovered: Option<&super::named_relations::DiscoveredMorphs>,
    ) -> DatabaseResult<Self> {
        Self::validate_path(path)?;
        let expression = super::NamedRelations::<M>::filter(
            &self.connection,
            &self.query,
            path,
            (operator, 1),
            discovered,
            Some(constraint),
        )?;
        self.predicates.raw(or, expression);
        Ok(self)
    }

    fn path_column(
        column: &str,
        operator: &str,
        value: Value,
    ) -> super::named_relations::PathConstraint {
        let column = column.to_owned();
        let operator = operator.to_owned();
        std::sync::Arc::new(move |conditions, qualifier| {
            let qualified;
            let column = if column.contains('.') {
                &column
            } else {
                qualified = format!("{qualifier}.{column}");
                &qualified
            };
            conditions.r#where(column, &operator, value.clone())
        })
    }

    fn validate_path(path: &str) -> DatabaseResult<()> {
        if path.split('.').any(str::is_empty) {
            return Err(DatabaseError::InvalidQuery("invalid relationship path"));
        }
        Ok(())
    }
}

impl<'connection, M: Model> Builder<'connection, M> {
    pub fn r#where(&mut self, column: &str, operator: &str, value: impl Into<Value>) -> &mut Self {
        self.predicates = mem::take(&mut self.predicates).r#where(column, operator, value);
        self
    }

    pub fn or_where(&mut self, column: &str, operator: &str, value: impl Into<Value>) -> &mut Self {
        self.predicates = mem::take(&mut self.predicates).or_where(column, operator, value);
        self
    }

    pub fn where_in(&mut self, column: &str, values: Vec<Value>) -> &mut Self {
        self.predicates = mem::take(&mut self.predicates).where_in(column, values);
        self
    }

    pub fn where_null(&mut self, column: &str) -> &mut Self {
        self.predicates = mem::take(&mut self.predicates).where_null(column);
        self
    }

    pub fn where_nested(
        &mut self,
        configure: impl FnOnce(query::Conditions) -> query::Conditions,
    ) -> &mut Self {
        self.predicates = mem::take(&mut self.predicates).where_nested(configure);
        self
    }

    pub fn where_full_text(&mut self, columns: &[&str], value: &str) -> &mut Self {
        self.predicates = mem::take(&mut self.predicates).where_full_text(columns, value);
        self
    }

    pub fn or_where_full_text(&mut self, columns: &[&str], value: &str) -> &mut Self {
        self.predicates = mem::take(&mut self.predicates).or_where_full_text(columns, value);
        self
    }

    pub fn where_full_text_with(
        &mut self,
        columns: &[&str],
        value: &str,
        options: query::FullTextOptions,
    ) -> &mut Self {
        self.predicates =
            mem::take(&mut self.predicates).where_full_text_with(columns, value, options);
        self
    }

    pub fn or_where_full_text_with(
        &mut self,
        columns: &[&str],
        value: &str,
        options: query::FullTextOptions,
    ) -> &mut Self {
        self.predicates =
            mem::take(&mut self.predicates).or_where_full_text_with(columns, value, options);
        self
    }

    pub fn where_vector_distance_less_than(
        &mut self,
        column: impl Into<query::Column>,
        vector: &[f64],
        max_distance: f64,
    ) -> &mut Self {
        self.predicates = mem::take(&mut self.predicates).where_vector_distance_less_than(
            column,
            vector,
            max_distance,
        );
        self
    }

    pub fn or_where_vector_distance_less_than(
        &mut self,
        column: impl Into<query::Column>,
        vector: &[f64],
        max_distance: f64,
    ) -> &mut Self {
        self.predicates = mem::take(&mut self.predicates).or_where_vector_distance_less_than(
            column,
            vector,
            max_distance,
        );
        self
    }

    pub fn where_vector_similar_to(
        mut self,
        column: impl Into<query::Column>,
        vector: &[f64],
        min_similarity: f64,
        order: bool,
    ) -> DatabaseResult<Self> {
        let max_distance = query::vector::similarity_max_distance(min_similarity)?;
        let column = column.into();
        self.where_vector_distance_less_than(column.clone(), vector, max_distance);
        if order {
            self.order_by_vector_distance(column, vector)
        } else {
            Ok(self)
        }
    }

    pub async fn where_vector_similar_to_text(
        self,
        column: impl Into<query::Column> + Send,
        text: &str,
        min_similarity: f64,
        order: bool,
    ) -> DatabaseResult<Self> {
        query::vector::similarity_max_distance(min_similarity)?;
        let vector = self.query.text_vector(text).await?;
        self.where_vector_similar_to(column, &vector, min_similarity, order)
    }

    pub async fn where_vector_distance_less_than_text(
        &mut self,
        column: impl Into<query::Column> + Send,
        text: &str,
        max_distance: f64,
    ) -> DatabaseResult<&mut Self> {
        query::vector::validate_max_distance(max_distance)?;
        let vector = self.query.text_vector(text).await?;
        Ok(self.where_vector_distance_less_than(column, &vector, max_distance))
    }

    pub async fn or_where_vector_distance_less_than_text(
        &mut self,
        column: impl Into<query::Column> + Send,
        text: &str,
        max_distance: f64,
    ) -> DatabaseResult<&mut Self> {
        query::vector::validate_max_distance(max_distance)?;
        let vector = self.query.text_vector(text).await?;
        Ok(self.or_where_vector_distance_less_than(column, &vector, max_distance))
    }

    pub fn where_key(&mut self, value: impl Into<Key>) -> &mut Self {
        self.key_condition(value.into(), false, false)
    }

    pub fn where_key_not(&mut self, value: impl Into<Key>) -> &mut Self {
        self.key_condition(value.into(), false, true)
    }

    pub fn or_where_key(&mut self, value: impl Into<Key>) -> &mut Self {
        self.key_condition(value.into(), true, false)
    }

    pub fn or_where_key_not(&mut self, value: impl Into<Key>) -> &mut Self {
        self.key_condition(value.into(), true, true)
    }

    fn key_condition(&mut self, value: Key, or: bool, not: bool) -> &mut Self {
        let column = self.qualify_column(M::metadata().primary_key);
        let integer =
            M::from_state(ModelState::default()).get_key_type() == super::ModelKeyType::Integer;
        let configure = |group: query::Conditions| match value {
            Key::Scalar(value) => group.r#where(
                &column,
                if not { "!=" } else { "=" },
                if integer { value } else { string_key(value) },
            ),
            Key::Many(values) if integer => {
                let values = values.into_iter().map(integer_key).collect();
                if not {
                    group.where_integer_not_in_raw(&column, values)
                } else {
                    group.where_integer_in_raw(&column, values)
                }
            }
            Key::Many(values) => {
                if not {
                    group.where_not_in(&column, values)
                } else {
                    group.where_in(&column, values)
                }
            }
        };
        let predicates = mem::take(&mut self.predicates);
        self.predicates = if or {
            predicates.or_where_nested(configure)
        } else {
            configure(predicates)
        };
        self
    }

    pub fn qualify_column(&self, column: &str) -> String {
        if column.contains('.') {
            return column.to_owned();
        }
        format!("{}.{column}", self.table_name())
    }

    pub(super) fn table_name(&self) -> &str {
        self.table_alias.as_deref().unwrap_or(M::metadata().table)
    }

    #[must_use]
    pub fn select(mut self, columns: impl IntoIterator<Item = impl Into<query::Column>>) -> Self {
        self.default_counts = false;
        self.select_relation_columns(columns)
    }

    pub fn select_vector_distance(
        mut self,
        column: impl Into<query::Column>,
        vector: &[f64],
        alias: Option<&str>,
    ) -> DatabaseResult<Self> {
        self.query = self.query.select_vector_distance(column, vector, alias)?;
        Ok(self)
    }

    pub async fn select_vector_distance_text(
        mut self,
        column: impl Into<query::Column> + Send,
        text: &str,
        alias: Option<&str>,
    ) -> DatabaseResult<Self> {
        self.query = self
            .query
            .select_vector_distance_text(column, text, alias)
            .await?;
        Ok(self)
    }

    /// Applies attribute casts to models retrieved or created by this query.
    #[must_use]
    pub fn with_casts(
        mut self,
        casts: impl IntoIterator<Item = (impl Into<String>, AttributeCast)>,
    ) -> Self {
        self.casts.merge(casts);
        self
    }

    pub(super) fn select_relation_columns(
        mut self,
        columns: impl IntoIterator<Item = impl Into<query::Column>>,
    ) -> Self {
        self.query = self.query.select(columns);
        self.existence_attributes.clear();
        self
    }

    #[must_use]
    pub fn order_by(mut self, column: &str, direction: &str) -> Self {
        self.query = self.query.order_by(column, direction);
        self
    }

    /// Orders models by a supplied sequence of values using the native query grammar.
    #[must_use]
    pub fn in_order_of(mut self, column: &str, values: Vec<Value>) -> Self {
        self.query = self.query.in_order_of(column, values);
        self
    }

    #[must_use]
    pub fn reorder(mut self) -> Self {
        self.query = self.query.reorder();
        self
    }

    pub fn order_by_vector_distance(
        mut self,
        column: impl Into<query::Column>,
        vector: &[f64],
    ) -> DatabaseResult<Self> {
        self.query = self.query.order_by_vector_distance(column, vector)?;
        Ok(self)
    }

    pub async fn order_by_vector_distance_text(
        mut self,
        column: impl Into<query::Column> + Send,
        text: &str,
    ) -> DatabaseResult<Self> {
        self.query = self
            .query
            .order_by_vector_distance_text(column, text)
            .await?;
        Ok(self)
    }

    pub(super) fn order_by_full_text(
        mut self,
        columns: Vec<String>,
        term: String,
        options: query::FullTextOptions,
    ) -> Self {
        self.query = self.query.order_by_full_text(columns, term, options);
        self
    }

    #[must_use]
    pub fn limit(mut self, limit: u64) -> Self {
        self.query = self.query.limit(limit);
        self
    }

    #[must_use]
    pub fn offset(mut self, offset: u64) -> Self {
        self.query = self.query.offset(offset);
        self
    }

    pub fn with_scope(&mut self, scope: &dyn Scope<M>) -> DatabaseResult<&mut Self> {
        let previous = mem::take(&mut self.predicates);
        if let Err(error) = scope.apply(self) {
            self.predicates = previous;
            return Err(error);
        }
        let scope = mem::take(&mut self.predicates);
        self.predicates = previous.where_nested(|_| scope);
        Ok(self)
    }

    /// 注册或替换本查询的具名全局作用域，并执行其装配钩子。
    pub fn with_global_scope(
        mut self,
        identifier: impl Into<String>,
        scope: impl Scope<M> + 'static,
    ) -> DatabaseResult<Self> {
        let scope: std::sync::Arc<dyn Scope<M>> = std::sync::Arc::new(scope);
        self.scopes.insert(
            identifier.into(),
            super::scope::ScopeEntry::Owned(scope.clone()),
        );
        scope.extend(&mut self)?;
        Ok(self)
    }

    #[must_use]
    pub fn without_global_scopes_except(mut self, scopes: &[&str]) -> Self {
        let removed: Vec<_> = self
            .scopes
            .keys()
            .filter(|name| !scopes.contains(&name.as_str()))
            .cloned()
            .collect();
        for name in removed {
            self = self.without_global_scope(&name);
        }
        self
    }

    pub fn removed_scopes(&self) -> &[String] {
        &self.removed_scopes
    }

    /// 取得当前底层查询的所有权，不自动应用尚未执行的全局作用域。
    pub fn get_query(self) -> query::Builder<'connection> {
        self.query.where_nested(|_| self.predicates)
    }

    #[must_use]
    pub fn set_query(mut self, query: query::Builder<'connection>) -> Self {
        self.connection = query.get_connection().clone();
        self.query = query;
        self.predicates = query::Conditions::default();
        self
    }

    pub fn get_eager_loads(&self) -> &EagerLoads<M> {
        &self.eager_load
    }

    #[must_use]
    pub fn set_eager_loads(mut self, eager_loads: EagerLoads<M>) -> Self {
        self.eager_load = eager_loads;
        self
    }

    #[must_use]
    pub fn without_global_scope(mut self, name: &str) -> Self {
        self.scopes.shift_remove(name);
        self.removed_scopes.push(name.into());
        self
    }

    #[must_use]
    pub fn without_global_scopes(mut self) -> Self {
        let names: Vec<_> = self.scopes.keys().cloned().collect();
        for name in names {
            self = self.without_global_scope(&name);
        }
        self
    }

    #[must_use]
    pub fn with_trashed(mut self) -> Self {
        self.trashed = Trashed::Include;
        self.without_global_scope(std::any::type_name::<super::SoftDeletingScope>())
    }

    #[must_use]
    pub fn without_trashed(mut self) -> Self {
        self = self.with_trashed();
        self.trashed = Trashed::Without;
        if let Some(column) = M::metadata().deleted_at {
            self.where_null(&self.qualify_column(column));
        }
        self
    }

    #[must_use]
    pub fn only_trashed(mut self) -> Self {
        self = self.with_trashed();
        self.trashed = Trashed::Only;
        // 不支持软删除的模型仍在执行时显式报错。
        if let Some(column) = M::metadata().deleted_at {
            let column = self.qualify_column(column);
            self.predicates = mem::take(&mut self.predicates).where_not_null(&column);
        }
        self
    }
}

impl<M: Model> Builder<'static, M> {
    pub(super) fn enable_default_counts(mut self) -> Self {
        self.default_counts = true;
        self
    }

    pub(super) fn apply_default_counts(mut self) -> DatabaseResult<Self> {
        if mem::take(&mut self.default_counts) {
            return M::with_default_counts(self);
        }
        Ok(self)
    }

    /// 应用当前作用域并移交查询所有权；保留注册项，重复调用会再次应用。
    pub fn apply_scopes(mut self) -> DatabaseResult<Self> {
        let scopes = self.scopes.clone();
        let mut conditions = mem::take(&mut self.predicates);
        if !scopes.is_empty() {
            self.query = self.query.group_predicates();
            conditions = query::Conditions::default().where_nested(|_| conditions);
            for (name, scope) in *scopes {
                if self.scopes.contains_key(&name) {
                    scope.get().apply(&mut self)?;
                    conditions = conditions.where_nested(|_| mem::take(&mut self.predicates));
                }
            }
        }
        self.predicates = conditions;
        if matches!(self.trashed, Trashed::Only) && M::metadata().deleted_at.is_none() {
            return Err(soft_deletes::unsupported());
        }
        Ok(self)
    }

    pub(super) fn scoped(self) -> DatabaseResult<(query::Builder<'static>, Connection)> {
        let query = self.apply_scopes()?;
        let connection = query.connection.clone();
        Ok((query.get_query(), connection))
    }

    #[cfg(feature = "sanctum")]
    pub(crate) fn require_relation_conditions(
        mut self,
        configure: impl FnOnce(query::Conditions) -> query::Conditions,
    ) -> Self {
        self.query = self.query.where_nested(configure);
        self
    }

    #[cfg(feature = "sanctum")]
    pub(crate) fn require_mysql_morph_alias(self, alias: &str) -> Self {
        if self.connection.is_sqlite() {
            return self;
        }
        self.require_relation_conditions(|conditions| {
            conditions.where_raw("BINARY tokenable_type = BINARY ?", vec![alias.into()])
        })
    }

    pub(crate) fn require_relation_value(mut self, column: &str, value: impl Into<Value>) -> Self {
        self.query = self.query.r#where(column, "=", value);
        self
    }

    pub(super) fn require_relation_subquery(
        mut self,
        column: &str,
        query: &mut query::Builder<'_>,
    ) -> DatabaseResult<Self> {
        self.query = self.query.where_sub(column, "=", query)?;
        Ok(self)
    }

    pub(super) fn relation_candidate(&self, alias: &str) -> Self {
        self.unconstrained_relation_query().alias_relation(alias)
    }

    pub(super) fn unconstrained_relation_query(&self) -> Self {
        let mut candidate = Self::new(self.connection.clone());
        candidate.removed_scopes.clone_from(&self.removed_scopes);
        candidate.scopes.clone_from(&self.scopes);
        candidate.trashed = self.trashed;
        candidate
    }

    pub(super) fn restrict_relation_attributes(mut self, attributes: &Attributes) -> Self {
        for (column, value) in attributes {
            let column = self.qualify_column(column);
            self.query = if matches!(value, Value::Null) {
                self.query.where_null(&column)
            } else {
                self.query.r#where(&column, "=", value.clone())
            };
        }
        self
    }

    pub(super) fn map_relation_query(
        mut self,
        configure: impl FnOnce(query::Builder<'static>) -> query::Builder<'static>,
    ) -> Self {
        self.query = configure(self.query);
        self
    }

    pub(super) fn restrict_relation(mut self, column: &str, keys: &[Value]) -> Self {
        // Keep relation keys in the base query so an OR inside a related
        // query constraint cannot admit a different parent's records.
        self.query = self.query.where_in(column, keys.to_vec());
        if keys.len() > 1 {
            self.query = self.query.configure_relation_group_limit(column);
        }
        self
    }

    pub(super) fn take_eager_load(&mut self) -> EagerLoads<M> {
        mem::take(&mut self.eager_load)
    }

    pub(super) fn for_pivot(
        mut self,
        pivot: super::relations::custom_pivot::PivotIdentity,
    ) -> Self {
        self.query = self.query.from(pivot.table);
        self.table_alias = Some(pivot.table.into());
        self.pivot = Some(pivot);
        self
    }

    pub(super) fn lock_for_update(mut self) -> Self {
        self.query = self.query.lock_for_update();
        self
    }

    pub(super) fn for_persisted_model(mut self, model: &M) -> DatabaseResult<Self> {
        if let Some(identity) = model.state().pivot_identity {
            self = self.for_pivot(identity);
        }
        self.query = super::persistence::scoped_model(model, self.query)?;
        Ok(self)
    }

    pub(super) fn alias_relation(mut self, alias: &str) -> Self {
        self.query = self
            .query
            .from(&format!("{} as {alias}", M::metadata().table));
        self.table_alias = Some(alias.to_owned());
        self
    }

    pub fn has(self, relation: &impl ExistenceRelation<Parent = M>) -> DatabaseResult<Self> {
        self.where_has(relation)
    }

    pub fn where_has(self, relation: &impl ExistenceRelation<Parent = M>) -> DatabaseResult<Self> {
        self.relation_exists(relation, false, false)
    }

    pub fn or_has(self, relation: &impl ExistenceRelation<Parent = M>) -> DatabaseResult<Self> {
        self.or_where_has(relation)
    }

    pub fn or_where_has(
        self,
        relation: &impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<Self> {
        self.relation_exists(relation, true, false)
    }

    fn relation_exists(
        mut self,
        relation: &impl ExistenceRelation<Parent = M>,
        or: bool,
        negated: bool,
    ) -> DatabaseResult<Self> {
        let mut related =
            relation.get_relation_existence_query(&self.connection, self.table_name())?;
        self.predicates
            .raw(or, self.query.exists_expression(&mut related, negated)?);
        Ok(self)
    }

    pub fn has_count(
        mut self,
        relation: &impl ExistenceRelation<Parent = M>,
        operator: &str,
        count: u64,
    ) -> DatabaseResult<Self> {
        if count == 1 {
            match operator {
                ">=" => return self.where_has(relation),
                "<" => return self.where_doesnt_have(relation),
                _ => (),
            }
        }
        let related = relation.get_relation_existence_query(&self.connection, self.table_name())?;
        let mut related = related.select_related_aggregate("count", "*")?;
        let expression = self
            .query
            .related_count_expression(&mut related, operator, count)?;
        self.predicates.raw(false, expression);
        Ok(self)
    }

    pub fn where_doesnt_have(
        self,
        relation: &impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<Self> {
        self.relation_exists(relation, false, true)
    }

    pub fn doesnt_have(
        self,
        relation: &impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<Self> {
        self.where_doesnt_have(relation)
    }

    pub fn or_doesnt_have(
        self,
        relation: &impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<Self> {
        self.or_where_doesnt_have(relation)
    }

    pub fn or_where_doesnt_have(
        self,
        relation: &impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<Self> {
        self.relation_exists(relation, true, true)
    }

    pub fn where_relation(
        self,
        relation: impl ConstrainableRelation<Parent = M>,
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.where_has(&Self::constrain_relation(
            relation,
            column,
            operator,
            value.into(),
        ))
    }

    pub fn or_where_relation(
        self,
        relation: impl ConstrainableRelation<Parent = M>,
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.or_where_has(&Self::constrain_relation(
            relation,
            column,
            operator,
            value.into(),
        ))
    }

    pub fn where_doesnt_have_relation(
        self,
        relation: impl ConstrainableRelation<Parent = M>,
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.where_doesnt_have(&Self::constrain_relation(
            relation,
            column,
            operator,
            value.into(),
        ))
    }

    pub fn or_where_doesnt_have_relation(
        self,
        relation: impl ConstrainableRelation<Parent = M>,
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.or_where_doesnt_have(&Self::constrain_relation(
            relation,
            column,
            operator,
            value.into(),
        ))
    }

    pub fn with_where_relation(
        self,
        name: impl Into<String>,
        relation: impl ConstrainableRelation<Parent = M>,
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.with_where_has(
            name,
            Self::constrain_relation(relation, column, operator, value.into()),
        )
    }

    fn constrain_relation<R: ConstrainableRelation<Parent = M>>(
        relation: R,
        column: &str,
        operator: &str,
        value: Value,
    ) -> R {
        let column = column.to_owned();
        let operator = operator.to_owned();
        relation.constrain_query(move |mut query| {
            let column = query.qualify_column(&column);
            let existing = mem::take(&mut query.predicates);
            query.predicates = query::Conditions::default()
                .where_nested(|_| existing)
                .r#where(&column, &operator, value.clone());
            query
        })
    }

    pub fn where_has_morph(self, relation: &MorphTo<M>, aliases: &[&str]) -> DatabaseResult<Self> {
        self.has_morph_count(relation, aliases, ">=", 1)
    }

    /// Filters by the foreign key and the owners' configured owner key.
    pub fn where_belongs_to<R: Model>(
        self,
        relation: &BelongsTo<M, R>,
        owners: &[R],
    ) -> DatabaseResult<Self> {
        self.where_belongs_to_boolean(relation, owners, false)
    }

    pub fn or_where_belongs_to<R: Model>(
        self,
        relation: &BelongsTo<M, R>,
        owners: &[R],
    ) -> DatabaseResult<Self> {
        self.where_belongs_to_boolean(relation, owners, true)
    }

    fn where_belongs_to_boolean<R: Model>(
        mut self,
        relation: &BelongsTo<M, R>,
        owners: &[R],
        or: bool,
    ) -> DatabaseResult<Self> {
        if owners.is_empty() {
            return Err(DatabaseError::InvalidQuery(
                "empty relationship identity list",
            ));
        }
        let (foreign_key, owner_key) = relation.identity_columns();
        let keys = owners
            .iter()
            .map(|model| {
                model
                    .get_attribute(owner_key)
                    .cloned()
                    .unwrap_or(Value::Null)
            })
            .collect();
        let foreign_key = self.qualify_column(foreign_key);
        self.predicates = if or {
            self.predicates.or_where_in(&foreign_key, keys)
        } else {
            self.predicates.where_in(&foreign_key, keys)
        };
        Ok(self)
    }

    /// Filters by related models linked through the intermediate table.
    /// A polymorphic descriptor dereferences to its discriminator-aware relation.
    pub fn where_attached_to<R: Model>(
        self,
        relation: &BelongsToMany<M, R>,
        related: &[R],
    ) -> DatabaseResult<Self> {
        self.where_attached_to_boolean(relation, related, false)
    }

    pub fn or_where_attached_to<R: Model>(
        self,
        relation: &BelongsToMany<M, R>,
        related: &[R],
    ) -> DatabaseResult<Self> {
        self.where_attached_to_boolean(relation, related, true)
    }

    fn where_attached_to_boolean<R: Model>(
        mut self,
        relation: &BelongsToMany<M, R>,
        related: &[R],
        or: bool,
    ) -> DatabaseResult<Self> {
        if related.is_empty() {
            return Err(DatabaseError::InvalidQuery(
                "empty relationship identity list",
            ));
        }
        let keys = related
            .iter()
            .map(|model| model.get_key().cloned().unwrap_or(Value::Null))
            .collect();
        let mut query =
            relation.existence_query(&self.connection, self.table_name(), Some(keys))?;
        self.predicates
            .raw(or, self.query.exists_expression(&mut query, false)?);
        Ok(self)
    }

    pub fn where_morphed_to(
        self,
        relation: &MorphTo<M>,
        identities: &[MorphIdentity],
    ) -> DatabaseResult<Self> {
        self.where_morph_identity(relation, identities, false, false)
    }

    pub fn where_not_morphed_to(
        self,
        relation: &MorphTo<M>,
        identities: &[MorphIdentity],
    ) -> DatabaseResult<Self> {
        self.where_morph_identity(relation, identities, false, true)
    }

    pub fn or_where_morphed_to(
        self,
        relation: &MorphTo<M>,
        identities: &[MorphIdentity],
    ) -> DatabaseResult<Self> {
        self.where_morph_identity(relation, identities, true, false)
    }

    pub fn or_where_not_morphed_to(
        self,
        relation: &MorphTo<M>,
        identities: &[MorphIdentity],
    ) -> DatabaseResult<Self> {
        self.where_morph_identity(relation, identities, true, true)
    }

    /// Filters by a stored morph alias or by a null type.
    #[must_use]
    pub fn where_morph_type(mut self, relation: &MorphTo<M>, alias: Option<&str>) -> Self {
        let column = self.qualify_column(relation.type_column());
        let value = alias.map_or(Value::Null, Value::from);
        self.predicates = self.predicates.where_null_safe_equals(&column, value);
        self
    }

    fn where_morph_identity(
        mut self,
        relation: &MorphTo<M>,
        identities: &[MorphIdentity],
        or: bool,
        negated: bool,
    ) -> DatabaseResult<Self> {
        if identities.is_empty() {
            return Err(DatabaseError::InvalidQuery(
                "empty relationship identity list",
            ));
        }
        let mut groups = IndexMap::<&str, Vec<Value>>::new();
        for identity in identities {
            relation.validate_identity(identity)?;
            groups
                .entry(identity.alias)
                .or_default()
                .push(identity.key.clone());
        }
        let morph_type = self.qualify_column(relation.type_column());
        let morph_id = self.qualify_column(relation.id_column());
        let configure = |conditions: query::Conditions| {
            groups
                .into_iter()
                .fold(conditions, |conditions, (alias, keys)| {
                    conditions.or_where_nested(|group| {
                        group
                            .where_null_safe_equals(&morph_type, alias)
                            .where_in(&morph_id, keys)
                    })
                })
        };
        self.predicates = match (or, negated) {
            (false, false) => self.predicates.where_nested(configure),
            (false, true) => self.predicates.where_not(configure),
            (true, false) => self.predicates.or_where_nested(configure),
            (true, true) => self.predicates.or_where_not(configure),
        };
        Ok(self)
    }

    pub fn where_doesnt_have_morph(
        self,
        relation: &MorphTo<M>,
        aliases: &[&str],
    ) -> DatabaseResult<Self> {
        self.has_morph_count(relation, aliases, "<", 1)
    }

    pub fn has_morph_count(
        self,
        relation: &MorphTo<M>,
        aliases: &[&str],
        operator: &str,
        count: u64,
    ) -> DatabaseResult<Self> {
        self.has_morph_count_with_null(relation, aliases, operator, count, false)
    }

    /// Discovers the stored morph types before adding an existence predicate.
    pub async fn where_has_morph_all(self, relation: &MorphTo<M>) -> DatabaseResult<Self> {
        self.has_morph_count_all(relation, ">=", 1).await
    }

    /// Discovers the stored morph types before adding an absence predicate.
    pub async fn where_doesnt_have_morph_all(self, relation: &MorphTo<M>) -> DatabaseResult<Self> {
        self.has_morph_count_all(relation, "<", 1).await
    }

    /// The asynchronous counterpart of wildcard morph query.
    pub async fn has_morph_count_all(
        self,
        relation: &MorphTo<M>,
        operator: &str,
        count: u64,
    ) -> DatabaseResult<Self> {
        let aliases = relation.discover_types(&self.connection).await?;
        let include_null = match operator {
            "<" | "!=" => count >= 1,
            "<=" => true,
            "=" => count == 0,
            _ => false,
        };
        self.has_morph_count_with_null(relation, &aliases, operator, count, include_null)
    }

    fn has_morph_count_with_null(
        self,
        relation: &MorphTo<M>,
        aliases: &[&str],
        operator: &str,
        count: u64,
        include_null: bool,
    ) -> DatabaseResult<Self> {
        let queries = relation.existence_queries(&self.connection, self.table_name(), aliases)?;
        self.morph_predicate(relation, queries, (operator, count), include_null, false)
    }

    fn morph_predicate(
        mut self,
        relation: &MorphTo<M>,
        queries: Vec<(&str, query::Builder<'_>)>,
        (operator, count): (&str, u64),
        include_null: bool,
        or: bool,
    ) -> DatabaseResult<Self> {
        let column = self.qualify_column(relation.type_column());
        let expression = self.query.morph_count_expression(
            &column,
            queries,
            operator,
            count,
            include_null,
            false,
        )?;
        self.predicates.raw(or, expression);
        Ok(self)
    }

    pub fn has_morph(self, relation: &MorphTo<M>, aliases: &[&str]) -> DatabaseResult<Self> {
        self.where_has_morph(relation, aliases)
    }

    pub fn doesnt_have_morph(
        self,
        relation: &MorphTo<M>,
        aliases: &[&str],
    ) -> DatabaseResult<Self> {
        self.where_doesnt_have_morph(relation, aliases)
    }

    pub fn or_has_morph(self, relation: &MorphTo<M>, aliases: &[&str]) -> DatabaseResult<Self> {
        self.or_where_has_morph(relation, aliases)
    }

    pub fn or_doesnt_have_morph(
        self,
        relation: &MorphTo<M>,
        aliases: &[&str],
    ) -> DatabaseResult<Self> {
        self.or_where_doesnt_have_morph(relation, aliases)
    }

    pub fn or_where_has_morph(
        self,
        relation: &MorphTo<M>,
        aliases: &[&str],
    ) -> DatabaseResult<Self> {
        let queries = relation.existence_queries(&self.connection, self.table_name(), aliases)?;
        self.morph_predicate(relation, queries, (">=", 1), false, true)
    }

    pub fn or_where_doesnt_have_morph(
        self,
        relation: &MorphTo<M>,
        aliases: &[&str],
    ) -> DatabaseResult<Self> {
        let queries = relation.existence_queries(&self.connection, self.table_name(), aliases)?;
        self.morph_predicate(relation, queries, ("<", 1), false, true)
    }

    pub fn where_morph_relation(
        self,
        relation: &MorphTo<M>,
        aliases: &[&str],
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.constrained_morph(
            relation,
            aliases,
            (column, operator, value.into()),
            false,
            false,
        )
    }

    pub fn or_where_morph_relation(
        self,
        relation: &MorphTo<M>,
        aliases: &[&str],
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.constrained_morph(
            relation,
            aliases,
            (column, operator, value.into()),
            false,
            true,
        )
    }

    pub fn where_morph_doesnt_have_relation(
        self,
        relation: &MorphTo<M>,
        aliases: &[&str],
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.constrained_morph(
            relation,
            aliases,
            (column, operator, value.into()),
            true,
            false,
        )
    }

    pub fn or_where_morph_doesnt_have_relation(
        self,
        relation: &MorphTo<M>,
        aliases: &[&str],
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.constrained_morph(
            relation,
            aliases,
            (column, operator, value.into()),
            true,
            true,
        )
    }

    fn constrained_morph(
        self,
        relation: &MorphTo<M>,
        aliases: &[&str],
        (column, operator, value): (&str, &str, Value),
        negated: bool,
        or: bool,
    ) -> DatabaseResult<Self> {
        let queries = relation
            .existence_queries(&self.connection, self.table_name(), aliases)?
            .into_iter()
            .map(|(alias, query)| {
                let qualified = if column.contains('.') {
                    column.to_owned()
                } else {
                    format!("{}.{column}", query.relation_qualifier())
                };
                (alias, query.r#where(&qualified, operator, value.clone()))
            })
            .collect();
        self.morph_predicate(
            relation,
            queries,
            (if negated { "<" } else { ">=" }, 1),
            false,
            or,
        )
    }

    pub fn with_where_has(
        self,
        name: impl Into<String>,
        relation: impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<Self> {
        Ok(self.where_has(&relation)?.with(name, relation))
    }

    pub(super) fn take_existence_attributes(&mut self) -> HashSet<String> {
        mem::take(&mut self.existence_attributes)
    }

    pub fn with_aggregate(
        mut self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
        aggregate: RelationAggregate<'_>,
    ) -> DatabaseResult<Self> {
        let alias = aggregate.alias(name)?;
        let related = relation.get_relation_existence_query(&self.connection, self.table_name())?;
        let projection = self.qualify_column("*");
        let parent = self.query.select_default(&projection);
        self.query = if matches!(aggregate, RelationAggregate::Exists) {
            let mut related = related.reorder().select([query::Expression::new("1")]);
            self.existence_attributes.insert(alias.clone());
            parent.select_exists(&mut related, &alias)?
        } else {
            let (function, column) = aggregate.function();
            let mut related = related.select_related_aggregate(function, column)?;
            parent.select_sub(&mut related, &alias)?
        };
        Ok(self)
    }

    pub fn with_count(
        self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<Self> {
        self.with_aggregate(name, relation, RelationAggregate::Count)
    }

    pub fn with_exists(
        self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<Self> {
        self.with_aggregate(name, relation, RelationAggregate::Exists)
    }

    pub fn with_sum(
        self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
        column: &str,
    ) -> DatabaseResult<Self> {
        self.with_aggregate(name, relation, RelationAggregate::Sum(column))
    }

    pub fn with_avg(
        self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
        column: &str,
    ) -> DatabaseResult<Self> {
        self.with_aggregate(name, relation, RelationAggregate::Avg(column))
    }

    pub fn with_min(
        self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
        column: &str,
    ) -> DatabaseResult<Self> {
        self.with_aggregate(name, relation, RelationAggregate::Min(column))
    }

    pub fn with_max(
        self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
        column: &str,
    ) -> DatabaseResult<Self> {
        self.with_aggregate(name, relation, RelationAggregate::Max(column))
    }

    #[must_use]
    pub fn with(mut self, name: impl Into<String>, relation: impl Relation<Parent = M>) -> Self {
        self.eager_load = self.eager_load.with(name, relation);
        self
    }

    /// Adds eager loads by their registered names or dot-separated paths.
    pub fn with_paths(self, paths: &[&str]) -> DatabaseResult<Self> {
        Ok(self.with_eager_loads(super::NamedRelations::<M>::prepare(paths)?))
    }

    pub(super) fn select_path_columns(self, columns: &[String]) -> Self {
        let columns = columns
            .iter()
            .map(|column| self.qualify_column(column))
            .collect::<Vec<_>>();
        self.select(columns.iter().map(String::as_str))
    }

    pub(super) fn with_path_constraint(
        mut self,
        constraint: &(impl Fn(query::Conditions, &str) -> query::Conditions + ?Sized),
    ) -> Self {
        let qualifier = self.table_name().to_owned();
        let existing = mem::take(&mut self.predicates);
        self.predicates = query::Conditions::default()
            .where_nested(|_| existing)
            .where_nested(|conditions| constraint(conditions, &qualifier));
        self
    }

    pub(super) fn with_eager_loads(mut self, loads: EagerLoads<M>) -> Self {
        self.eager_load.extend(loads);
        self
    }

    /// Removes named eager loads from this query.
    #[must_use]
    pub fn without(mut self, names: &[&str]) -> Self {
        self.eager_load = self.eager_load.without(names);
        self
    }

    /// Replaces this query's eager loads with the supplied relationships.
    #[must_use]
    pub fn with_only(mut self, configure: impl FnOnce(EagerLoads<M>) -> EagerLoads<M>) -> Self {
        self.eager_load = configure(EagerLoads::default());
        self
    }

    /// Supplies model defaults and optionally adds matching query conditions.
    #[must_use]
    pub fn with_attributes(mut self, attributes: Attributes, as_conditions: bool) -> Self {
        if as_conditions {
            for (column, value) in &attributes {
                let column = self.qualify_column(column);
                self.r#where(&column, "=", value.clone());
            }
        }
        self.pending_attributes.extend(attributes);
        self
    }

    pub fn make(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.make_with_attributes(self.creation_attributes(attributes))
    }

    pub(super) fn creation_attributes(&self, attributes: Attributes) -> Attributes {
        let mut pending = self.pending_attributes.clone();
        pending.extend(attributes);
        pending
    }

    fn make_with_attributes(&self, attributes: Attributes) -> DatabaseResult<M> {
        if self.casts.is_empty() {
            return M::new_instance_on(&self.connection, attributes);
        }
        let mut model = M::new_instance_on(&self.connection, Attributes::new())?;
        model.state_mut().casts.merge_from(&self.casts);
        model.fill(attributes)?;
        Ok(model)
    }

    pub async fn create(self, attributes: Attributes) -> DatabaseResult<M> {
        let mut model = self.make_async(attributes).await?;
        model.save().await?;
        Ok(model)
    }

    /// Prepares insert rows with model defaults, casts, unique IDs and timestamps.
    pub async fn fill_for_insert(&self, rows: Vec<Attributes>) -> DatabaseResult<Vec<Attributes>> {
        let mut prepared = Vec::with_capacity(rows.len());
        for row in rows {
            let mut model = M::new_instance_on(&self.connection, Attributes::new())?;
            let state = model.state_mut();
            state.casts.merge_from(&self.casts);
            let mut attributes = state.attributes.clone();
            for (name, value) in self.creation_attributes(row) {
                super::casts::write_async::<M>(
                    &mut attributes,
                    name,
                    value,
                    Some(&self.connection),
                    &state.casts,
                )
                .await?;
            }
            model.state_mut().replace_attributes(attributes);
            prepared.push(model);
        }
        self.prepare_insert_models(prepared).await
    }

    async fn prepare_insert_models(&self, models: Vec<M>) -> DatabaseResult<Vec<Attributes>> {
        if models.is_empty() {
            return Ok(Vec::new());
        }
        let metadata = M::metadata();
        let time = if metadata.created_at.is_some() || metadata.updated_at.is_some() {
            Some(timestamp(&self.connection, metadata.date_format)?)
        } else {
            None
        };
        let mut prepared = Vec::with_capacity(models.len());
        for mut model in models {
            model.set_unique_ids().await?;
            let mut attributes = mem::take(&mut model.state_mut().attributes);
            if let Some(time) = &time {
                for column in [metadata.created_at, metadata.updated_at]
                    .into_iter()
                    .flatten()
                {
                    attributes
                        .entry(column.into())
                        .or_insert_with(|| time.clone());
                }
            }
            prepared.push(attributes);
        }
        Ok(prepared)
    }

    /// Inserts prepared rows without dispatching model persistence events.
    pub async fn fill_and_insert(self, rows: Vec<Attributes>) -> DatabaseResult<bool> {
        let rows = self.fill_for_insert(rows).await?;
        self.scoped()?.0.insert(&rows).await
    }

    pub async fn fill_and_insert_or_ignore(self, rows: Vec<Attributes>) -> DatabaseResult<u64> {
        let rows = self.fill_for_insert(rows).await?;
        self.scoped()?.0.insert_or_ignore(&rows).await
    }

    pub async fn fill_and_insert_get_id(self, row: Attributes) -> DatabaseResult<Value> {
        let row = self
            .fill_for_insert(vec![row])
            .await?
            .pop()
            .expect("one prepared insert row");
        self.scoped()?
            .0
            .insert_get_id_for(row, M::metadata().primary_key)
            .await
    }

    pub async fn make_async(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.make_with_attributes_async(self.creation_attributes(attributes))
            .await
    }

    pub(super) async fn make_with_attributes_async(
        &self,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        if !attributes
            .keys()
            .any(|name| super::casts::needs_async::<M>(name, &self.casts))
        {
            return self.make_with_attributes(attributes);
        }
        let mut model = self.make_with_attributes(Attributes::new())?;
        model.fill_async(attributes).await?;
        Ok(model)
    }

    /// Reads this query from the write connection.
    #[must_use]
    pub fn use_write_pdo(mut self) -> Self {
        self.query = self.query.use_write_pdo();
        self
    }

    pub async fn get(self) -> DatabaseResult<Collection<M>> {
        self.get_with_transaction(None).await
    }

    pub(super) fn get_with_transaction<'a>(
        self,
        transaction: Option<&'a mut crate::database::Transaction>,
    ) -> crate::database::DatabaseFuture<'a, DatabaseResult<Collection<M>>> {
        Box::pin(async move {
            self.get_models_with_transaction(transaction)
                .await
                .map(M::new_collection)
        })
    }

    pub(super) async fn get_models_with_transaction(
        self,
        mut transaction: Option<&mut crate::database::Transaction>,
    ) -> DatabaseResult<Vec<M>> {
        let (query, hydration) = self.hydration_query()?;
        let mut models = hydration.rows(
            query
                .get_with_transaction(transaction.as_deref_mut())
                .await?,
        );
        let source = transaction
            .as_deref_mut()
            .filter(|tx| tx.belongs_to(&hydration.connection));
        super::concerns::has_events::retrieved_with_transaction(
            &mut models,
            &hydration.connection,
            source,
        )
        .await?;
        super::concerns::has_relationships::load_eager_with_transaction(
            hydration.eager,
            &mut models,
            transaction,
        )
        .await?;
        Ok(models)
    }

    pub async fn paginate(self, per_page: u64) -> DatabaseResult<LengthAwarePaginator<M>> {
        self.paginate_with(PageOptions {
            per_page: Some(per_page),
            ..PageOptions::default()
        })
        .await
    }

    pub async fn paginate_with(
        self,
        options: PageOptions<'_>,
    ) -> DatabaseResult<LengthAwarePaginator<M>> {
        self.paginate_hydrated(options, |_, _| Ok(())).await
    }

    pub(super) async fn paginate_hydrated(
        self,
        mut options: PageOptions<'_>,
        hydrate: impl FnOnce(&mut [M], &Connection) -> DatabaseResult<()>,
    ) -> DatabaseResult<LengthAwarePaginator<M>> {
        options.per_page.get_or_insert(M::metadata().per_page);
        let key = self.qualify_column(M::metadata().primary_key);
        let (query, hydration) = self.hydration_query()?;
        let mut page = query
            .order_for_pagination(&key)
            .paginate_with(options)
            .await?
            .map_items(|rows| hydration.rows(rows));
        hydrate(page.items_mut(), &hydration.connection)?;
        super::concerns::has_events::retrieved(page.items_mut(), &hydration.connection).await?;
        load_eager(hydration.eager, page.items_mut()).await?;
        page.apply_model_collection();
        Ok(page)
    }

    pub async fn simple_paginate(self, per_page: u64) -> DatabaseResult<Paginator<M>> {
        self.simple_paginate_with(PageOptions {
            per_page: Some(per_page),
            ..PageOptions::default()
        })
        .await
    }

    pub async fn simple_paginate_with(
        self,
        options: PageOptions<'_>,
    ) -> DatabaseResult<Paginator<M>> {
        self.simple_paginate_hydrated(options, |_, _| Ok(())).await
    }

    pub(super) async fn simple_paginate_hydrated(
        self,
        mut options: PageOptions<'_>,
        hydrate: impl FnOnce(&mut [M], &Connection) -> DatabaseResult<()>,
    ) -> DatabaseResult<Paginator<M>> {
        options.per_page.get_or_insert(M::metadata().per_page);
        let key = self.qualify_column(M::metadata().primary_key);
        let (query, hydration) = self.hydration_query()?;
        let mut page = query
            .order_for_pagination(&key)
            .simple_paginate_with(options)
            .await?
            .map_items(|rows| hydration.rows(rows));
        hydrate(page.items_mut(), &hydration.connection)?;
        super::concerns::has_events::retrieved(page.items_mut(), &hydration.connection).await?;
        load_eager(hydration.eager, page.items_mut()).await?;
        page.apply_model_collection();
        Ok(page)
    }

    /// Streams hydrated models one at a time from separately fetched offset batches.
    pub fn lazy(self, chunk_size: u64) -> DatabaseCursor<'static, M> {
        query::flatten_batches(
            self.chunk_collections(chunk_size, None, |_, _| Ok(())),
            |models| models,
        )
    }

    pub fn lazy_by_id(
        self,
        chunk_size: u64,
        column: Option<&str>,
        alias: Option<&str>,
    ) -> DatabaseCursor<'static, M> {
        self.lazy_by_key(chunk_size, column, alias, false)
    }

    pub fn lazy_by_id_desc(
        self,
        chunk_size: u64,
        column: Option<&str>,
        alias: Option<&str>,
    ) -> DatabaseCursor<'static, M> {
        self.lazy_by_key(chunk_size, column, alias, true)
    }

    pub(super) fn lazy_by_key(
        self,
        chunk_size: u64,
        column: Option<&str>,
        alias: Option<&str>,
        descending: bool,
    ) -> DatabaseCursor<'static, M> {
        let mut key = self.chunk_key(column, alias, descending);
        key.lazy = true;
        query::flatten_batches(
            self.chunk_collections(chunk_size, Some(key), |_, _| Ok(())),
            |models| models,
        )
    }

    pub async fn chunk_map<T, Fut>(
        self,
        callback: impl FnMut(M) -> Fut,
        count: u64,
    ) -> DatabaseResult<crate::support::Collection<T>>
    where
        Fut: Future<Output = DatabaseResult<T>>,
    {
        query::map_chunk_rows(self.lazy(count), callback).await
    }

    /// Visits models with their keys from each hydrated batch.
    pub async fn each<Fut>(
        self,
        callback: impl FnMut(M, crate::support::ArrayKey) -> Fut,
        count: u64,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        let batches = self
            .chunk_collections(count, None, |_, _| Ok(()))
            .map_ok(Collection::to_base);
        query::visit_batches(batches, |_, key| Ok(key), callback).await
    }

    /// Visits models in ascending key order, with indices spanning all batches.
    pub async fn each_by_id<Fut>(
        self,
        callback: impl FnMut(M, i64) -> Fut,
        count: u64,
        column: Option<&str>,
        alias: Option<&str>,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        let key = self.chunk_key(column, alias, false);
        let batches = self
            .chunk_collections(count, Some(key), |_, _| Ok(()))
            .map_ok(Collection::to_base);
        query::visit_batches(
            batches,
            |batch, key| query::collection_chunk_index(batch, count, key),
            callback,
        )
        .await
    }

    /// Fetches model collections in separate queries, stopping when the callback returns false.
    pub async fn chunk<Fut>(
        self,
        count: u64,
        callback: impl FnMut(Collection<M>, u64) -> Fut,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        self.chunk_hydrated(count, None, |_, _| Ok(()), callback)
            .await
    }

    /// Processes successive batches ordered by the model key or a projected key.
    pub async fn chunk_by_id<Fut>(
        self,
        count: u64,
        callback: impl FnMut(Collection<M>, u64) -> Fut,
        column: Option<&str>,
        alias: Option<&str>,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        self.ordered_chunk_by_id(count, callback, column, alias, false)
            .await
    }

    pub async fn chunk_by_id_desc<Fut>(
        self,
        count: u64,
        callback: impl FnMut(Collection<M>, u64) -> Fut,
        column: Option<&str>,
        alias: Option<&str>,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        self.ordered_chunk_by_id(count, callback, column, alias, true)
            .await
    }

    pub async fn ordered_chunk_by_id<Fut>(
        self,
        count: u64,
        callback: impl FnMut(Collection<M>, u64) -> Fut,
        column: Option<&str>,
        alias: Option<&str>,
        descending: bool,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        let key = self.chunk_key(column, alias, descending);
        self.chunk_hydrated(count, Some(key), |_, _| Ok(()), callback)
            .await
    }

    pub(super) fn chunk_key(
        &self,
        column: Option<&str>,
        alias: Option<&str>,
        descending: bool,
    ) -> query::ChunkKey {
        let primary_key = M::metadata().primary_key;
        query::ChunkKey {
            column: column.map_or_else(|| self.qualify_column(primary_key), str::to_owned),
            alias: alias.or(column).unwrap_or(primary_key).into(),
            descending,
            lazy: false,
        }
    }

    pub(super) async fn chunk_hydrated<Fut>(
        self,
        count: u64,
        key: Option<query::ChunkKey>,
        hydrate: impl FnMut(&mut [M], &Connection) -> DatabaseResult<()> + Send,
        mut callback: impl FnMut(Collection<M>, u64) -> Fut,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        let mut batches = self.chunk_collections(count, key, hydrate);
        let mut page = 1;
        while let Some(models) = batches.try_next().await? {
            if !callback(models, page).await? {
                return Ok(false);
            }
            page += 1;
        }
        Ok(true)
    }

    pub(super) fn chunk_collections<'a>(
        self,
        count: u64,
        key: Option<query::ChunkKey>,
        mut hydrate: impl FnMut(&mut [M], &Connection) -> DatabaseResult<()> + Send + 'a,
    ) -> DatabaseCursor<'a, Collection<M>> {
        Box::pin(async_stream::try_stream! {
            let primary_key = self.qualify_column(M::metadata().primary_key);
            let (mut query, hydration) = self.hydration_query()?;
            let mut pages = if let Some(key) = key {
                query.chunk_id_pages(count, key)
            } else {
                query = query.order_for_pagination(&primary_key);
                query.chunk_pages(count)
            };
            while let Some(rows) = pages.try_next().await? {
                let mut models = hydration.rows(rows);
                hydrate(&mut models, &hydration.connection)?;
                super::concerns::has_events::retrieved(&mut models, &hydration.connection).await?;
                load_eager(hydration.eager.clone(), &mut models).await?;
                yield M::new_collection(models);
            }
        })
    }

    /// Hydrates models as the query yields rows. Eager relations require a batched query.
    pub fn cursor(self) -> DatabaseCursor<'static, M> {
        self.cursor_hydrated(|_, _| Ok(()))
    }

    pub(super) fn cursor_hydrated<'a>(
        self,
        mut hydrate: impl FnMut(&mut M, &Connection) -> DatabaseResult<()> + Send + 'a,
    ) -> DatabaseCursor<'a, M> {
        Box::pin(async_stream::try_stream! {
            let (query, hydration) = self.hydration_query()?;
            let mut rows = query.cursor();
            while let Some(row) = rows.try_next().await? {
                let mut model = hydration.row(row);
                hydrate(&mut model, &hydration.connection)?;
                super::concerns::has_events::retrieved(
                    std::slice::from_mut(&mut model),
                    &hydration.connection,
                ).await?;
                if let Some(model) = M::new_collection(vec![model]).into_iter().next() {
                    yield model;
                }
            }
        })
    }

    pub async fn cursor_paginate(self, per_page: u64) -> DatabaseResult<CursorPaginator<M>> {
        self.cursor_paginate_with(CursorOptions {
            per_page: Some(per_page),
            ..CursorOptions::default()
        })
        .await
    }

    pub async fn cursor_paginate_with(
        self,
        options: CursorOptions<'_>,
    ) -> DatabaseResult<CursorPaginator<M>> {
        self.cursor_paginate_hydrated(options, |_, _| Ok(())).await
    }

    pub(super) async fn cursor_paginate_hydrated(
        self,
        mut options: CursorOptions<'_>,
        hydrate: impl FnOnce(&mut [M], &Connection) -> DatabaseResult<()>,
    ) -> DatabaseResult<CursorPaginator<M>> {
        options.per_page.get_or_insert(M::metadata().per_page);
        let key = self.qualify_column(M::metadata().primary_key);
        let (query, hydration) = self.hydration_query()?;
        let mut page = query
            .order_for_pagination(&key)
            .cursor_paginate_with(options)
            .await?
            .map_items(|rows| hydration.rows(rows));
        hydrate(page.items_mut(), &hydration.connection)?;
        super::concerns::has_events::retrieved(page.items_mut(), &hydration.connection).await?;
        load_eager(hydration.eager, page.items_mut()).await?;
        page.apply_model_collection();
        Ok(page)
    }

    fn hydration_query(self) -> DatabaseResult<(query::Builder<'static>, Hydration<M>)> {
        let mut query = self.apply_default_counts()?;
        let eager = query.take_eager_load();
        let existence = query.take_existence_attributes();
        let casts = mem::take(&mut query.casts);
        let pivot = query.pivot;
        let (query, connection) = query.scoped()?;
        Ok((
            query,
            Hydration {
                connection,
                casts,
                eager,
                existence,
                pivot,
            },
        ))
    }

    /// 通过模型取值，保留访问器、类型转换、检索事件及全局作用域。
    pub async fn value(self, column: &str) -> DatabaseResult<Value> {
        self.value_query(column)
            .first()
            .await?
            .map(|model| model.get_attribute_value(column.rsplit('.').next().unwrap_or(column)))
            .transpose()
            .map(|value| value.flatten().unwrap_or(Value::Null))
    }

    pub async fn sole(self) -> DatabaseResult<M> {
        super::relations::sole_match(self.limit(2).get().await?.into_iter().collect())
    }

    pub async fn sole_value(self, column: &str) -> DatabaseResult<Value> {
        self.value_query(column)
            .sole()
            .await?
            .get_attribute_value(column.rsplit('.').next().unwrap_or(column))
            .map(|value| value.unwrap_or(Value::Null))
    }

    pub async fn value_or_fail(self, column: &str) -> DatabaseResult<Value> {
        self.value_query(column)
            .first_or_fail()
            .await?
            .get_attribute_value(column.rsplit('.').next().unwrap_or(column))
            .map(|value| value.unwrap_or(Value::Null))
    }

    fn value_query(self, column: &str) -> Self {
        if self.query.has_columns() {
            self
        } else {
            self.select([column])
        }
    }

    pub async fn first(self) -> DatabaseResult<Option<M>> {
        Ok(self.limit(1).get().await?.into_iter().next())
    }

    pub async fn first_or_fail(self) -> DatabaseResult<M> {
        self.first()
            .await?
            .ok_or(DatabaseError::ModelNotFound(M::metadata().table))
    }

    pub async fn find(mut self, key: impl Into<Value>) -> DatabaseResult<Option<M>> {
        self.where_key(key);
        self.first().await
    }

    pub async fn find_or_fail(mut self, key: impl Into<Value>) -> DatabaseResult<M> {
        self.where_key(key);
        self.first_or_fail().await
    }

    pub async fn update(self, mut attributes: Attributes) -> DatabaseResult<u64> {
        self.add_updated_at(&mut attributes)?;
        self.scoped()?.0.update(attributes).await
    }

    /// Writes multiple rows without hydrating or saving individual models.
    pub async fn upsert(
        self,
        mut rows: Vec<Attributes>,
        unique_by: &[&str],
        update: Option<&[&str]>,
    ) -> DatabaseResult<u64> {
        let Some(first) = rows.first() else {
            return Ok(0);
        };
        let mut columns: Vec<String> = update.map_or_else(
            || first.keys().cloned().collect(),
            |columns| columns.iter().map(|column| (*column).into()).collect(),
        );
        let metadata = M::metadata();
        let timestamp = if metadata.created_at.is_some() || metadata.updated_at.is_some() {
            Some(timestamp(&self.connection, metadata.date_format)?)
        } else {
            None
        };
        if let Some(column) = metadata.updated_at
            && !columns.iter().any(|candidate| candidate == column)
        {
            columns.push(column.into());
        }
        let ids = M::unique_ids();
        let needs_ids = ids
            .iter()
            .any(|&column| rows.iter().any(|row| !row.contains_key(column)));
        let model = needs_ids
            .then(|| M::new_instance_on(&self.connection, Attributes::new()))
            .transpose()?;
        for row in &mut rows {
            if let Some(model) = &model {
                for &column in ids {
                    if !row.contains_key(column) {
                        let id = model.new_unique_id()?;
                        row.insert(column.into(), id.map_or(Value::Null, Value::Text));
                    }
                }
            }
            if let Some(time) = &timestamp {
                for column in [metadata.created_at, metadata.updated_at]
                    .into_iter()
                    .flatten()
                {
                    row.entry(column.into()).or_insert_with(|| time.clone());
                }
            }
        }
        let columns: Vec<&str> = columns.iter().map(String::as_str).collect();
        self.scoped()?
            .0
            .upsert(&rows, unique_by, Some(&columns))
            .await
    }

    pub async fn increment(
        self,
        column: &str,
        amount: impl Into<Value>,
        extra: Attributes,
    ) -> DatabaseResult<u64> {
        self.increment_each(Attributes::from([(column.into(), amount.into())]), extra)
            .await
    }

    pub async fn increment_each(
        self,
        columns: Attributes,
        mut extra: Attributes,
    ) -> DatabaseResult<u64> {
        self.add_updated_at(&mut extra)?;
        self.scoped()?.0.increment_each(columns, extra).await
    }

    pub async fn decrement(
        self,
        column: &str,
        amount: impl Into<Value>,
        extra: Attributes,
    ) -> DatabaseResult<u64> {
        self.decrement_each(Attributes::from([(column.into(), amount.into())]), extra)
            .await
    }

    pub async fn decrement_each(
        self,
        columns: Attributes,
        mut extra: Attributes,
    ) -> DatabaseResult<u64> {
        self.add_updated_at(&mut extra)?;
        self.scoped()?.0.decrement_each(columns, extra).await
    }

    fn add_updated_at(&self, attributes: &mut Attributes) -> DatabaseResult<()> {
        if let Some(column) = M::metadata().updated_at
            && !attributes.contains_key(column)
        {
            attributes.insert(
                column.into(),
                timestamp(&self.connection, M::metadata().date_format)?,
            );
        }
        Ok(())
    }

    pub async fn delete(self) -> DatabaseResult<u64> {
        if let Some(column) = M::metadata().deleted_at {
            let attributes =
                soft_deletes::deletion_attributes(M::metadata(), column, &self.connection)?;
            return self.scoped()?.0.update(attributes).await;
        }
        self.scoped()?.0.delete().await
    }

    pub async fn restore(mut self) -> DatabaseResult<u64> {
        let column = M::metadata()
            .deleted_at
            .ok_or_else(soft_deletes::unsupported)?;
        if matches!(self.trashed, Trashed::Exclude) {
            self = self.with_trashed();
        }
        self.update(Attributes::from([(column.into(), Value::Null)]))
            .await
    }

    pub async fn force_delete(mut self) -> DatabaseResult<u64> {
        self.scopes.clear();
        if matches!(self.trashed, Trashed::Exclude) {
            self = self.with_trashed();
        }
        self.scoped()?.0.delete().await
    }
}

struct Hydration<M: Model> {
    connection: Connection,
    casts: CastOverrides,
    eager: EagerLoads<M>,
    existence: HashSet<String>,
    pivot: Option<super::relations::custom_pivot::PivotIdentity>,
}

impl<M: Model> Hydration<M> {
    fn rows(&self, rows: Vec<Row>) -> Vec<M> {
        rows.into_iter().map(|row| self.row(row)).collect()
    }

    fn row(&self, row: Row) -> M {
        let mut attributes = row.into_columns().into_iter().collect();
        cast_existence(&mut attributes, &self.existence);
        let mut state = ModelState::hydrated_attributes(
            attributes,
            self.connection.clone(),
            self.casts.clone(),
        );
        state.pivot_identity = self.pivot;
        M::from_state(state)
    }
}
