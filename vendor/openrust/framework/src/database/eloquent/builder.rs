use std::{collections::HashSet, marker::PhantomData, mem};

use indexmap::IndexMap;

use crate::{
    database::{Connection, DatabaseError, DatabaseResult, Row, Value, query},
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
    removed_scopes: HashSet<String>,
    apply_global_scopes: bool,
    trashed: Trashed,
    model: PhantomData<M>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent Builder API")]
impl<M: Model> Builder<'static, M> {
    pub(super) fn new(connection: Connection) -> Self {
        Self {
            query: connection.table(M::metadata().table),
            connection,
            table_alias: None,
            pivot: None,
            existence_attributes: HashSet::new(),
            casts: CastOverrides::default(),
            predicates: query::Conditions::default(),
            pending_attributes: Attributes::new(),
            eager_load: EagerLoads::default(),
            default_counts: false,
            removed_scopes: HashSet::new(),
            apply_global_scopes: true,
            trashed: Trashed::default(),
            model: PhantomData,
        }
    }
    /// Filters by the existence of a registered relation or dot-separated path.
    pub fn whereHasPath(self, path: &str) -> DatabaseResult<Self> {
        self.hasPathCount(path, ">=", 1)
    }

    /// Filters by the absence of a complete registered relation path.
    pub fn whereDoesntHavePath(self, path: &str) -> DatabaseResult<Self> {
        self.hasPathCount(path, "<", 1)
    }

    /// Applies the count comparison at the final segment of the relation path.
    pub fn hasPathCount(mut self, path: &str, operator: &str, count: u64) -> DatabaseResult<Self> {
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
    pub async fn whereHasPathAll(self, path: &str) -> DatabaseResult<Self> {
        self.hasPathCountAll(path, ">=", 1).await
    }

    /// Discovers stored morph aliases before testing for a missing relation path.
    pub async fn whereDoesntHavePathAll(self, path: &str) -> DatabaseResult<Self> {
        self.hasPathCountAll(path, "<", 1).await
    }

    /// Discovers stored morph aliases and compares the final segment's count.
    pub async fn hasPathCountAll(
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

    pub fn whereRelationPath(
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

    pub fn orWhereRelationPath(
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

    pub fn whereDoesntHaveRelationPath(
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

    pub fn orWhereDoesntHaveRelationPath(
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
    pub fn whereRelationPathWith(
        self,
        path: &str,
        constraint: impl Fn(query::Conditions, &str) -> query::Conditions + Send + Sync + 'static,
    ) -> DatabaseResult<Self> {
        let constraint: super::named_relations::PathConstraint = std::sync::Arc::new(constraint);
        self.filter_path_with(path, ">=", false, &constraint, None)
    }

    pub fn withWhereRelationPath(
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
    pub fn withWhereRelationPathWith(
        self,
        path: &str,
        constraint: impl Fn(query::Conditions, &str) -> query::Conditions + Send + Sync + 'static,
    ) -> DatabaseResult<Self> {
        self.with_where_path(path, std::sync::Arc::new(constraint), None)
    }

    /// Filters the final relationship after discovering stored morph aliases.
    pub async fn whereRelationPathAll(
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
    pub async fn whereDoesntHaveRelationPathAll(
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
    pub async fn withWhereRelationPathAll(
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
    pub async fn withWhereRelationPathAllWith(
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

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent Builder API")]
impl<M: Model> Builder<'_, M> {
    pub fn r#where(&mut self, column: &str, operator: &str, value: impl Into<Value>) -> &mut Self {
        self.predicates = mem::take(&mut self.predicates).r#where(column, operator, value);
        self
    }

    pub fn orWhere(&mut self, column: &str, operator: &str, value: impl Into<Value>) -> &mut Self {
        self.predicates = mem::take(&mut self.predicates).orWhere(column, operator, value);
        self
    }

    pub fn whereIn(&mut self, column: &str, values: Vec<Value>) -> &mut Self {
        self.predicates = mem::take(&mut self.predicates).whereIn(column, values);
        self
    }

    pub fn whereNull(&mut self, column: &str) -> &mut Self {
        self.predicates = mem::take(&mut self.predicates).whereNull(column);
        self
    }

    pub fn whereNested(
        &mut self,
        configure: impl FnOnce(query::Conditions) -> query::Conditions,
    ) -> &mut Self {
        self.predicates = mem::take(&mut self.predicates).whereNested(configure);
        self
    }

    pub fn whereFullText(&mut self, columns: &[&str], value: &str) -> &mut Self {
        self.predicates = mem::take(&mut self.predicates).whereFullText(columns, value);
        self
    }

    pub fn orWhereFullText(&mut self, columns: &[&str], value: &str) -> &mut Self {
        self.predicates = mem::take(&mut self.predicates).orWhereFullText(columns, value);
        self
    }

    pub fn whereFullTextWith(
        &mut self,
        columns: &[&str],
        value: &str,
        options: query::FullTextOptions,
    ) -> &mut Self {
        self.predicates =
            mem::take(&mut self.predicates).whereFullTextWith(columns, value, options);
        self
    }

    pub fn orWhereFullTextWith(
        &mut self,
        columns: &[&str],
        value: &str,
        options: query::FullTextOptions,
    ) -> &mut Self {
        self.predicates =
            mem::take(&mut self.predicates).orWhereFullTextWith(columns, value, options);
        self
    }

    pub fn whereKey(&mut self, value: impl Into<Value>) -> &mut Self {
        self.r#where(M::metadata().primary_key, "=", value)
    }

    pub fn qualifyColumn(&self, column: &str) -> String {
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

    /// Applies attribute casts to models retrieved or created by this query.
    #[must_use]
    pub fn withCasts(
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
    pub fn orderBy(mut self, column: &str, direction: &str) -> Self {
        self.query = self.query.orderBy(column, direction);
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

    pub fn withScope(&mut self, scope: &dyn Scope<M>) -> DatabaseResult<&mut Self> {
        let previous = mem::take(&mut self.predicates);
        if let Err(error) = scope.apply(self) {
            self.predicates = previous;
            return Err(error);
        }
        let scope = mem::take(&mut self.predicates);
        self.predicates = previous.whereNested(|_| scope);
        Ok(self)
    }

    #[must_use]
    pub fn withoutGlobalScope(mut self, name: &str) -> Self {
        self.removed_scopes.insert(name.into());
        self
    }

    #[must_use]
    pub fn withoutGlobalScopes(mut self) -> Self {
        self.apply_global_scopes = false;
        self.withTrashed()
    }

    #[must_use]
    pub fn withTrashed(mut self) -> Self {
        self.trashed = Trashed::Include;
        self
    }

    #[must_use]
    pub fn withoutTrashed(mut self) -> Self {
        self.trashed = Trashed::Without;
        self
    }

    #[must_use]
    pub fn onlyTrashed(mut self) -> Self {
        self.trashed = Trashed::Only;
        self
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent Builder API")]
impl<M: Model> Builder<'static, M> {
    pub(super) fn enable_default_counts(mut self) -> Self {
        self.default_counts = true;
        self
    }

    pub(super) fn apply_default_counts(mut self) -> DatabaseResult<Self> {
        if mem::take(&mut self.default_counts) {
            return M::withDefaultCounts(self);
        }
        Ok(self)
    }

    pub(super) fn scoped(mut self) -> DatabaseResult<(query::Builder<'static>, Connection)> {
        let mut conditions = mem::take(&mut self.predicates);
        if self.apply_global_scopes {
            // Keep each registered scope in its own AND group so an OR in the
            // caller's predicates or another scope cannot bypass its constraint.
            conditions = query::Conditions::default().whereNested(|_| conditions);
            for (name, scope) in M::globalScopes() {
                if !self.removed_scopes.contains(*name) {
                    scope.apply(&mut self)?;
                    conditions = conditions.whereNested(|_| mem::take(&mut self.predicates));
                }
            }
        }
        let deleted_at = M::metadata()
            .deleted_at
            .map(|column| self.qualifyColumn(column));
        let query = self.query.whereNested(|_| conditions);
        Ok((
            self.trashed.apply(query, deleted_at.as_deref())?,
            self.connection,
        ))
    }

    #[cfg(feature = "sanctum")]
    pub(crate) fn require_relation_conditions(
        mut self,
        configure: impl FnOnce(query::Conditions) -> query::Conditions,
    ) -> Self {
        self.query = self.query.whereNested(configure);
        self
    }

    #[cfg(feature = "sanctum")]
    pub(crate) fn require_mysql_morph_alias(self, alias: &str) -> Self {
        if self.connection.is_sqlite() {
            return self;
        }
        self.require_relation_conditions(|conditions| {
            conditions.whereRaw("BINARY tokenable_type = BINARY ?", vec![alias.into()])
        })
    }

    pub(crate) fn require_relation_value(mut self, column: &str, value: impl Into<Value>) -> Self {
        self.query = self.query.r#where(column, "=", value);
        self
    }

    pub(super) fn require_relation_subquery(
        mut self,
        column: &str,
        query: &query::Builder<'_>,
    ) -> DatabaseResult<Self> {
        self.query = self.query.whereSub(column, "=", query)?;
        Ok(self)
    }

    pub(super) fn relation_candidate(&self, alias: &str) -> Self {
        self.unconstrained_relation_query().alias_relation(alias)
    }

    pub(super) fn unconstrained_relation_query(&self) -> Self {
        let mut candidate = Self::new(self.connection.clone());
        candidate.removed_scopes.clone_from(&self.removed_scopes);
        candidate.apply_global_scopes = self.apply_global_scopes;
        candidate.trashed = self.trashed;
        candidate
    }

    pub(super) fn restrict_relation_attributes(mut self, attributes: &Attributes) -> Self {
        for (column, value) in attributes {
            let column = self.qualifyColumn(column);
            self.query = if matches!(value, Value::Null) {
                self.query.whereNull(&column)
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
        self.query = self.query.whereIn(column, keys.to_vec());
        if keys.len() > 1 {
            self.query = self.query.group_limit(column);
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
        self.query = self.query.lockForUpdate();
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
        self.whereHas(relation)
    }

    pub fn whereHas(self, relation: &impl ExistenceRelation<Parent = M>) -> DatabaseResult<Self> {
        self.relation_exists(relation, false, false)
    }

    pub fn orHas(self, relation: &impl ExistenceRelation<Parent = M>) -> DatabaseResult<Self> {
        self.orWhereHas(relation)
    }

    pub fn orWhereHas(self, relation: &impl ExistenceRelation<Parent = M>) -> DatabaseResult<Self> {
        self.relation_exists(relation, true, false)
    }

    fn relation_exists(
        mut self,
        relation: &impl ExistenceRelation<Parent = M>,
        or: bool,
        negated: bool,
    ) -> DatabaseResult<Self> {
        let related = relation.getRelationExistenceQuery(&self.connection, self.table_name())?;
        self.predicates
            .raw(or, self.query.exists_expression(&related, negated)?);
        Ok(self)
    }

    pub fn hasCount(
        mut self,
        relation: &impl ExistenceRelation<Parent = M>,
        operator: &str,
        count: u64,
    ) -> DatabaseResult<Self> {
        if count == 1 {
            match operator {
                ">=" => return self.whereHas(relation),
                "<" => return self.whereDoesntHave(relation),
                _ => (),
            }
        }
        let related = relation.getRelationExistenceQuery(&self.connection, self.table_name())?;
        let related = related.select_related_aggregate("count", "*")?;
        let expression = self
            .query
            .related_count_expression(&related, operator, count)?;
        self.predicates.raw(false, expression);
        Ok(self)
    }

    pub fn whereDoesntHave(
        self,
        relation: &impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<Self> {
        self.relation_exists(relation, false, true)
    }

    pub fn doesntHave(self, relation: &impl ExistenceRelation<Parent = M>) -> DatabaseResult<Self> {
        self.whereDoesntHave(relation)
    }

    pub fn orDoesntHave(
        self,
        relation: &impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<Self> {
        self.orWhereDoesntHave(relation)
    }

    pub fn orWhereDoesntHave(
        self,
        relation: &impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<Self> {
        self.relation_exists(relation, true, true)
    }

    pub fn whereRelation(
        self,
        relation: impl ConstrainableRelation<Parent = M>,
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.whereHas(&Self::constrain_relation(
            relation,
            column,
            operator,
            value.into(),
        ))
    }

    pub fn orWhereRelation(
        self,
        relation: impl ConstrainableRelation<Parent = M>,
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.orWhereHas(&Self::constrain_relation(
            relation,
            column,
            operator,
            value.into(),
        ))
    }

    pub fn whereDoesntHaveRelation(
        self,
        relation: impl ConstrainableRelation<Parent = M>,
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.whereDoesntHave(&Self::constrain_relation(
            relation,
            column,
            operator,
            value.into(),
        ))
    }

    pub fn orWhereDoesntHaveRelation(
        self,
        relation: impl ConstrainableRelation<Parent = M>,
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.orWhereDoesntHave(&Self::constrain_relation(
            relation,
            column,
            operator,
            value.into(),
        ))
    }

    pub fn withWhereRelation(
        self,
        name: impl Into<String>,
        relation: impl ConstrainableRelation<Parent = M>,
        column: &str,
        operator: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        self.withWhereHas(
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
            let column = query.qualifyColumn(&column);
            let existing = mem::take(&mut query.predicates);
            query.predicates = query::Conditions::default()
                .whereNested(|_| existing)
                .r#where(&column, &operator, value.clone());
            query
        })
    }

    pub fn whereHasMorph(self, relation: &MorphTo<M>, aliases: &[&str]) -> DatabaseResult<Self> {
        self.hasMorphCount(relation, aliases, ">=", 1)
    }

    /// Filters by the foreign key and the owners' configured owner key.
    pub fn whereBelongsTo<R: Model>(
        self,
        relation: &BelongsTo<M, R>,
        owners: &[R],
    ) -> DatabaseResult<Self> {
        self.where_belongs_to(relation, owners, false)
    }

    pub fn orWhereBelongsTo<R: Model>(
        self,
        relation: &BelongsTo<M, R>,
        owners: &[R],
    ) -> DatabaseResult<Self> {
        self.where_belongs_to(relation, owners, true)
    }

    fn where_belongs_to<R: Model>(
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
                    .getAttribute(owner_key)
                    .cloned()
                    .unwrap_or(Value::Null)
            })
            .collect();
        let foreign_key = self.qualifyColumn(foreign_key);
        self.predicates = if or {
            self.predicates.orWhereIn(&foreign_key, keys)
        } else {
            self.predicates.whereIn(&foreign_key, keys)
        };
        Ok(self)
    }

    /// Filters by related models linked through the intermediate table.
    /// A polymorphic descriptor dereferences to its discriminator-aware relation.
    pub fn whereAttachedTo<R: Model>(
        self,
        relation: &BelongsToMany<M, R>,
        related: &[R],
    ) -> DatabaseResult<Self> {
        self.where_attached_to(relation, related, false)
    }

    pub fn orWhereAttachedTo<R: Model>(
        self,
        relation: &BelongsToMany<M, R>,
        related: &[R],
    ) -> DatabaseResult<Self> {
        self.where_attached_to(relation, related, true)
    }

    fn where_attached_to<R: Model>(
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
            .map(|model| model.getKey().cloned().unwrap_or(Value::Null))
            .collect();
        let query = relation.existence_query(&self.connection, self.table_name(), Some(keys))?;
        self.predicates
            .raw(or, self.query.exists_expression(&query, false)?);
        Ok(self)
    }

    pub fn whereMorphedTo(
        self,
        relation: &MorphTo<M>,
        identities: &[MorphIdentity],
    ) -> DatabaseResult<Self> {
        self.where_morph_identity(relation, identities, false, false)
    }

    pub fn whereNotMorphedTo(
        self,
        relation: &MorphTo<M>,
        identities: &[MorphIdentity],
    ) -> DatabaseResult<Self> {
        self.where_morph_identity(relation, identities, false, true)
    }

    pub fn orWhereMorphedTo(
        self,
        relation: &MorphTo<M>,
        identities: &[MorphIdentity],
    ) -> DatabaseResult<Self> {
        self.where_morph_identity(relation, identities, true, false)
    }

    pub fn orWhereNotMorphedTo(
        self,
        relation: &MorphTo<M>,
        identities: &[MorphIdentity],
    ) -> DatabaseResult<Self> {
        self.where_morph_identity(relation, identities, true, true)
    }

    /// Filters by a stored morph alias or by a null type.
    #[must_use]
    pub fn whereMorphType(mut self, relation: &MorphTo<M>, alias: Option<&str>) -> Self {
        let column = self.qualifyColumn(relation.type_column());
        let value = alias.map_or(Value::Null, Value::from);
        self.predicates = self.predicates.whereNullSafeEquals(&column, value);
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
        let morph_type = self.qualifyColumn(relation.type_column());
        let morph_id = self.qualifyColumn(relation.id_column());
        let configure = |conditions: query::Conditions| {
            groups
                .into_iter()
                .fold(conditions, |conditions, (alias, keys)| {
                    conditions.orWhereNested(|group| {
                        group
                            .whereNullSafeEquals(&morph_type, alias)
                            .whereIn(&morph_id, keys)
                    })
                })
        };
        self.predicates = match (or, negated) {
            (false, false) => self.predicates.whereNested(configure),
            (false, true) => self.predicates.whereNot(configure),
            (true, false) => self.predicates.orWhereNested(configure),
            (true, true) => self.predicates.orWhereNot(configure),
        };
        Ok(self)
    }

    pub fn whereDoesntHaveMorph(
        self,
        relation: &MorphTo<M>,
        aliases: &[&str],
    ) -> DatabaseResult<Self> {
        self.hasMorphCount(relation, aliases, "<", 1)
    }

    pub fn hasMorphCount(
        self,
        relation: &MorphTo<M>,
        aliases: &[&str],
        operator: &str,
        count: u64,
    ) -> DatabaseResult<Self> {
        self.has_morph_count(relation, aliases, operator, count, false)
    }

    /// Discovers the stored morph types before adding an existence predicate.
    pub async fn whereHasMorphAll(self, relation: &MorphTo<M>) -> DatabaseResult<Self> {
        self.hasMorphCountAll(relation, ">=", 1).await
    }

    /// Discovers the stored morph types before adding an absence predicate.
    pub async fn whereDoesntHaveMorphAll(self, relation: &MorphTo<M>) -> DatabaseResult<Self> {
        self.hasMorphCountAll(relation, "<", 1).await
    }

    /// The asynchronous counterpart of Laravel's wildcard morph query.
    pub async fn hasMorphCountAll(
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
        self.has_morph_count(relation, &aliases, operator, count, include_null)
    }

    fn has_morph_count(
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
        let column = self.qualifyColumn(relation.type_column());
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

    pub fn hasMorph(self, relation: &MorphTo<M>, aliases: &[&str]) -> DatabaseResult<Self> {
        self.whereHasMorph(relation, aliases)
    }

    pub fn doesntHaveMorph(self, relation: &MorphTo<M>, aliases: &[&str]) -> DatabaseResult<Self> {
        self.whereDoesntHaveMorph(relation, aliases)
    }

    pub fn orHasMorph(self, relation: &MorphTo<M>, aliases: &[&str]) -> DatabaseResult<Self> {
        self.orWhereHasMorph(relation, aliases)
    }

    pub fn orDoesntHaveMorph(
        self,
        relation: &MorphTo<M>,
        aliases: &[&str],
    ) -> DatabaseResult<Self> {
        self.orWhereDoesntHaveMorph(relation, aliases)
    }

    pub fn orWhereHasMorph(self, relation: &MorphTo<M>, aliases: &[&str]) -> DatabaseResult<Self> {
        let queries = relation.existence_queries(&self.connection, self.table_name(), aliases)?;
        self.morph_predicate(relation, queries, (">=", 1), false, true)
    }

    pub fn orWhereDoesntHaveMorph(
        self,
        relation: &MorphTo<M>,
        aliases: &[&str],
    ) -> DatabaseResult<Self> {
        let queries = relation.existence_queries(&self.connection, self.table_name(), aliases)?;
        self.morph_predicate(relation, queries, ("<", 1), false, true)
    }

    pub fn whereMorphRelation(
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

    pub fn orWhereMorphRelation(
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

    pub fn whereMorphDoesntHaveRelation(
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

    pub fn orWhereMorphDoesntHaveRelation(
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

    pub fn withWhereHas(
        self,
        name: impl Into<String>,
        relation: impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<Self> {
        Ok(self.whereHas(&relation)?.with(name, relation))
    }

    pub(super) fn take_existence_attributes(&mut self) -> HashSet<String> {
        mem::take(&mut self.existence_attributes)
    }

    pub fn withAggregate(
        mut self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
        aggregate: RelationAggregate<'_>,
    ) -> DatabaseResult<Self> {
        let alias = aggregate.alias(name)?;
        let related = relation.getRelationExistenceQuery(&self.connection, self.table_name())?;
        let projection = self.qualifyColumn("*");
        let parent = self.query.select_default(&projection);
        self.query = if matches!(aggregate, RelationAggregate::Exists) {
            let related = related.reorder().select([query::Expression::new("1")]);
            self.existence_attributes.insert(alias.clone());
            parent.select_exists(&related, &alias)?
        } else {
            let (function, column) = aggregate.function();
            let related = related.select_related_aggregate(function, column)?;
            parent.selectSub(&related, &alias)?
        };
        Ok(self)
    }

    pub fn withCount(
        self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<Self> {
        self.withAggregate(name, relation, RelationAggregate::Count)
    }

    pub fn withExists(
        self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<Self> {
        self.withAggregate(name, relation, RelationAggregate::Exists)
    }

    pub fn withSum(
        self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
        column: &str,
    ) -> DatabaseResult<Self> {
        self.withAggregate(name, relation, RelationAggregate::Sum(column))
    }

    pub fn withAvg(
        self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
        column: &str,
    ) -> DatabaseResult<Self> {
        self.withAggregate(name, relation, RelationAggregate::Avg(column))
    }

    pub fn withMin(
        self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
        column: &str,
    ) -> DatabaseResult<Self> {
        self.withAggregate(name, relation, RelationAggregate::Min(column))
    }

    pub fn withMax(
        self,
        name: &str,
        relation: &impl ExistenceRelation<Parent = M>,
        column: &str,
    ) -> DatabaseResult<Self> {
        self.withAggregate(name, relation, RelationAggregate::Max(column))
    }

    #[must_use]
    pub fn with(mut self, name: impl Into<String>, relation: impl Relation<Parent = M>) -> Self {
        self.eager_load = self.eager_load.with(name, relation);
        self
    }

    /// Adds eager loads by their registered names or dot-separated paths.
    pub fn withPaths(self, paths: &[&str]) -> DatabaseResult<Self> {
        Ok(self.with_eager_loads(super::NamedRelations::<M>::prepare(paths)?))
    }

    pub(super) fn select_path_columns(self, columns: &[String]) -> Self {
        let columns = columns
            .iter()
            .map(|column| self.qualifyColumn(column))
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
            .whereNested(|_| existing)
            .whereNested(|conditions| constraint(conditions, &qualifier));
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
    pub fn withOnly(mut self, configure: impl FnOnce(EagerLoads<M>) -> EagerLoads<M>) -> Self {
        self.eager_load = configure(EagerLoads::default());
        self
    }

    /// Supplies model defaults and optionally adds matching query conditions.
    #[must_use]
    pub fn withAttributes(mut self, attributes: Attributes, as_conditions: bool) -> Self {
        if as_conditions {
            for (column, value) in &attributes {
                let column = self.qualifyColumn(column);
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
            return M::newInstanceOn(&self.connection, attributes);
        }
        let mut model = M::newInstanceOn(&self.connection, Attributes::new())?;
        model.state_mut().casts.merge_from(&self.casts);
        model.fill(attributes)?;
        Ok(model)
    }

    pub async fn create(self, attributes: Attributes) -> DatabaseResult<M> {
        let mut model = self.makeAsync(attributes).await?;
        model.save().await?;
        Ok(model)
    }

    /// Prepares insert rows with model defaults, casts, unique IDs and timestamps.
    pub async fn fillForInsert(&self, rows: Vec<Attributes>) -> DatabaseResult<Vec<Attributes>> {
        let mut prepared = Vec::with_capacity(rows.len());
        for row in rows {
            let mut model = M::newInstanceOn(&self.connection, Attributes::new())?;
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
            model.setUniqueIds().await?;
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
    pub async fn fillAndInsert(self, rows: Vec<Attributes>) -> DatabaseResult<bool> {
        let rows = self.fillForInsert(rows).await?;
        self.scoped()?.0.insert(&rows).await
    }

    pub async fn fillAndInsertOrIgnore(self, rows: Vec<Attributes>) -> DatabaseResult<u64> {
        let rows = self.fillForInsert(rows).await?;
        self.scoped()?.0.insertOrIgnore(&rows).await
    }

    pub async fn fillAndInsertGetId(self, row: Attributes) -> DatabaseResult<Value> {
        let row = self
            .fillForInsert(vec![row])
            .await?
            .pop()
            .expect("one prepared insert row");
        self.scoped()?.0.insertGetId(row).await
    }

    pub async fn makeAsync(&self, attributes: Attributes) -> DatabaseResult<M> {
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
        model.fillAsync(attributes).await?;
        Ok(model)
    }

    /// Reads this query from the write connection.
    #[must_use]
    pub fn useWritePdo(mut self) -> Self {
        self.query = self.query.useWritePdo();
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
                .map(M::newCollection)
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
        self.paginateWith(PageOptions {
            perPage: Some(per_page),
            ..PageOptions::default()
        })
        .await
    }

    pub async fn paginateWith(
        self,
        options: PageOptions<'_>,
    ) -> DatabaseResult<LengthAwarePaginator<M>> {
        let key = self.qualifyColumn(M::metadata().primary_key);
        let (query, hydration) = self.hydration_query()?;
        let mut page = query
            .order_for_pagination(&key)
            .paginateWith(options)
            .await?
            .map_items(|rows| hydration.rows(rows));
        super::concerns::has_events::retrieved(page.items_mut(), &hydration.connection).await?;
        load_eager(hydration.eager, page.items_mut()).await?;
        Ok(page)
    }

    pub async fn simplePaginate(self, per_page: u64) -> DatabaseResult<Paginator<M>> {
        self.simplePaginateWith(PageOptions {
            perPage: Some(per_page),
            ..PageOptions::default()
        })
        .await
    }

    pub async fn simplePaginateWith(
        self,
        options: PageOptions<'_>,
    ) -> DatabaseResult<Paginator<M>> {
        let key = self.qualifyColumn(M::metadata().primary_key);
        let (query, hydration) = self.hydration_query()?;
        let mut page = query
            .order_for_pagination(&key)
            .simplePaginateWith(options)
            .await?
            .map_items(|rows| hydration.rows(rows));
        super::concerns::has_events::retrieved(page.items_mut(), &hydration.connection).await?;
        load_eager(hydration.eager, page.items_mut()).await?;
        Ok(page)
    }

    pub async fn cursorPaginate(self, per_page: u64) -> DatabaseResult<CursorPaginator<M>> {
        self.cursorPaginateWith(CursorOptions {
            perPage: Some(per_page),
            ..CursorOptions::default()
        })
        .await
    }

    pub async fn cursorPaginateWith(
        self,
        options: CursorOptions<'_>,
    ) -> DatabaseResult<CursorPaginator<M>> {
        let key = self.qualifyColumn(M::metadata().primary_key);
        let (query, hydration) = self.hydration_query()?;
        let mut page = query
            .order_for_pagination(&key)
            .cursorPaginateWith(options)
            .await?
            .map_items(|rows| hydration.rows(rows));
        super::concerns::has_events::retrieved(page.items_mut(), &hydration.connection).await?;
        load_eager(hydration.eager, page.items_mut()).await?;
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

    pub async fn first(self) -> DatabaseResult<Option<M>> {
        Ok(self.limit(1).get().await?.into_iter().next())
    }

    pub async fn firstOrFail(self) -> DatabaseResult<M> {
        self.first()
            .await?
            .ok_or(DatabaseError::ModelNotFound(M::metadata().table))
    }

    pub async fn find(mut self, key: impl Into<Value>) -> DatabaseResult<Option<M>> {
        self.whereKey(key);
        self.first().await
    }

    pub async fn findOrFail(mut self, key: impl Into<Value>) -> DatabaseResult<M> {
        self.whereKey(key);
        self.firstOrFail().await
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
        let ids = M::uniqueIds();
        let needs_ids = ids
            .iter()
            .any(|&column| rows.iter().any(|row| !row.contains_key(column)));
        let model = needs_ids
            .then(|| M::newInstanceOn(&self.connection, Attributes::new()))
            .transpose()?;
        for row in &mut rows {
            if let Some(model) = &model {
                for &column in ids {
                    if !row.contains_key(column) {
                        let id = model.newUniqueId()?;
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
        self.incrementEach(Attributes::from([(column.into(), amount.into())]), extra)
            .await
    }

    pub async fn incrementEach(
        self,
        columns: Attributes,
        mut extra: Attributes,
    ) -> DatabaseResult<u64> {
        self.add_updated_at(&mut extra)?;
        self.scoped()?.0.incrementEach(columns, extra).await
    }

    pub async fn decrement(
        self,
        column: &str,
        amount: impl Into<Value>,
        extra: Attributes,
    ) -> DatabaseResult<u64> {
        self.decrementEach(Attributes::from([(column.into(), amount.into())]), extra)
            .await
    }

    pub async fn decrementEach(
        self,
        columns: Attributes,
        mut extra: Attributes,
    ) -> DatabaseResult<u64> {
        self.add_updated_at(&mut extra)?;
        self.scoped()?.0.decrementEach(columns, extra).await
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
            self = self.withTrashed();
        }
        self.update(Attributes::from([(column.into(), Value::Null)]))
            .await
    }

    pub async fn forceDelete(mut self) -> DatabaseResult<u64> {
        self.apply_global_scopes = false;
        if matches!(self.trashed, Trashed::Exclude) {
            self = self.withTrashed();
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
        rows.into_iter()
            .map(|row| {
                let mut attributes = row.into_columns().into_iter().collect();
                cast_existence(&mut attributes, &self.existence);
                let mut state = ModelState::hydrated_attributes(
                    attributes,
                    self.connection.clone(),
                    self.casts.clone(),
                );
                state.pivot_identity = self.pivot;
                M::from_state(state)
            })
            .collect()
    }
}
