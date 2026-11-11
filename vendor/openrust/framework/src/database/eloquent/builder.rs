use std::{collections::HashSet, marker::PhantomData, mem};

use indexmap::IndexMap;

use crate::database::{Connection, DatabaseError, DatabaseResult, Value, query};

use super::concerns::queries_relationships::cast_existence;
use super::{
    Attributes, Collection, EagerLoads, Model, ModelState, RelationAggregate, Scope,
    concerns::{has_relationships::load_eager, has_timestamps::timestamp},
    relations::{BelongsTo, BelongsToMany, ExistenceRelation, MorphIdentity, MorphTo, Relation},
    soft_deletes,
    soft_deleting_scope::Trashed,
};

/// Builds model queries and hydrates results from the selected connection.
pub struct Builder<'connection, M: Model> {
    query: query::Builder<'connection>,
    connection: Connection,
    table_alias: Option<String>,
    existence_attributes: HashSet<String>,
    predicates: query::Conditions,
    eager_load: EagerLoads<M>,
    default_counts: bool,
    removed_scopes: HashSet<String>,
    apply_global_scopes: bool,
    trashed: Trashed,
    model: PhantomData<M>,
}

impl<M: Model> Builder<'static, M> {
    pub(super) fn new(connection: Connection) -> Self {
        Self {
            query: connection.table(M::metadata().table),
            connection,
            table_alias: None,
            existence_attributes: HashSet::new(),
            predicates: query::Conditions::default(),
            eager_load: EagerLoads::default(),
            default_counts: false,
            removed_scopes: HashSet::new(),
            apply_global_scopes: true,
            trashed: Trashed::default(),
            model: PhantomData,
        }
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

    pub(super) fn require_relation_value(mut self, column: &str, value: impl Into<Value>) -> Self {
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
        let mut candidate = Self::new(self.connection.clone()).alias_relation(alias);
        candidate.removed_scopes.clone_from(&self.removed_scopes);
        candidate.apply_global_scopes = self.apply_global_scopes;
        candidate.trashed = self.trashed;
        candidate
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

    pub fn whereHas(
        mut self,
        relation: &impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<Self> {
        let related = relation.getRelationExistenceQuery(&self.connection, self.table_name())?;
        self.query = self.query.whereExists(&related)?;
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
        self.query = self.query.where_related_count(&related, operator, count)?;
        Ok(self)
    }

    pub fn whereDoesntHave(
        mut self,
        relation: &impl ExistenceRelation<Parent = M>,
    ) -> DatabaseResult<Self> {
        let related = relation.getRelationExistenceQuery(&self.connection, self.table_name())?;
        self.query = self.query.whereNotExists(&related)?;
        Ok(self)
    }

    pub fn whereHasMorph(self, relation: &MorphTo<M>, aliases: &[&str]) -> DatabaseResult<Self> {
        self.hasMorphCount(relation, aliases, ">=", 1)
    }

    /// Filters by the foreign key and the owners' configured owner key.
    pub fn whereBelongsTo<R: Model>(
        mut self,
        relation: &BelongsTo<M, R>,
        owners: &[R],
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
        self.whereIn(&foreign_key, keys);
        Ok(self)
    }

    /// Filters by related models linked through the intermediate table.
    pub fn whereAttachedTo<R: Model>(
        mut self,
        relation: &BelongsToMany<M, R>,
        related: &[R],
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
        self.query = self.query.whereExists(&query)?;
        Ok(self)
    }

    pub fn whereMorphedTo(
        self,
        relation: &MorphTo<M>,
        identities: &[MorphIdentity],
    ) -> DatabaseResult<Self> {
        self.where_morph_identity(relation, identities, false)
    }

    pub fn whereNotMorphedTo(
        self,
        relation: &MorphTo<M>,
        identities: &[MorphIdentity],
    ) -> DatabaseResult<Self> {
        self.where_morph_identity(relation, identities, true)
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
        self.predicates = if negated {
            self.predicates.whereNot(configure)
        } else {
            self.predicates.whereNested(configure)
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
        mut self,
        relation: &MorphTo<M>,
        aliases: &[&str],
        operator: &str,
        count: u64,
        include_null: bool,
    ) -> DatabaseResult<Self> {
        let queries = relation.existence_queries(&self.connection, self.table_name(), aliases)?;
        let column = self.qualifyColumn(relation.type_column());
        self.query =
            self.query
                .where_morph_count(&column, queries, operator, count, include_null)?;
        Ok(self)
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

    pub fn make(&self, attributes: Attributes) -> DatabaseResult<M> {
        M::newInstanceOn(&self.connection, attributes)
    }

    pub async fn create(self, attributes: Attributes) -> DatabaseResult<M> {
        let mut model = self.make(attributes)?;
        model.save().await?;
        Ok(model)
    }

    pub async fn get(self) -> DatabaseResult<Collection<M>> {
        let mut query = self.apply_default_counts()?;
        let eager_load = query.take_eager_load();
        let existence = query.take_existence_attributes();
        let (query, connection) = query.scoped()?;
        let mut models: Vec<_> = query
            .get()
            .await?
            .into_iter()
            .map(|row| {
                let mut attributes = row.into_columns().into_iter().collect();
                cast_existence(&mut attributes, &existence);
                M::from_state(ModelState::hydrated_attributes(
                    attributes,
                    connection.clone(),
                ))
            })
            .collect();
        load_eager(eager_load, &mut models).await?;
        Ok(Collection::new(models))
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
        if let Some(column) = M::metadata().updated_at
            && !attributes.contains_key(column)
        {
            attributes.insert(column.into(), timestamp());
        }
        self.scoped()?.0.update(attributes).await
    }

    pub async fn delete(self) -> DatabaseResult<u64> {
        if let Some(column) = M::metadata().deleted_at {
            let attributes = soft_deletes::deletion_attributes(M::metadata(), column);
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
