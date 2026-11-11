use std::{collections::HashSet, marker::PhantomData, mem};

use crate::database::{Connection, DatabaseError, DatabaseResult, Value, query};

use super::{
    Attributes, Collection, Model, ModelState, Scope, concerns::has_timestamps::timestamp,
    soft_deletes, soft_deleting_scope::Trashed,
};

/// Builds model queries and hydrates results from the selected connection.
pub struct Builder<'connection, M: Model> {
    query: query::Builder<'connection>,
    connection: Connection,
    predicates: query::Conditions,
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
            predicates: query::Conditions::default(),
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
        format!("{}.{column}", M::metadata().table)
    }

    #[must_use]
    pub fn select(mut self, columns: impl IntoIterator<Item = impl Into<query::Column>>) -> Self {
        self.query = self.query.select(columns);
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

    pub fn make(&self, attributes: Attributes) -> DatabaseResult<M> {
        M::newInstanceOn(&self.connection, attributes)
    }

    pub async fn create(self, attributes: Attributes) -> DatabaseResult<M> {
        let mut model = self.make(attributes)?;
        model.save().await?;
        Ok(model)
    }

    pub async fn get(self) -> DatabaseResult<Collection<M>> {
        let (query, connection) = self.scoped()?;
        let models = query
            .get()
            .await?
            .into_iter()
            .map(|row| {
                M::from_state(ModelState::hydrated_attributes(
                    row.into_columns().into_iter().collect(),
                    connection.clone(),
                ))
            })
            .collect();
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
