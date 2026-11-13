use std::any::TypeId;

use crate::database::{Connection, DatabaseFuture, DatabaseResult, Transaction, Value};

use super::{
    Attributes, Model,
    relations::{BelongsTo, MorphTo, relation::dictionary_key},
};

trait Touch<M: Model>: Send + Sync {
    fn touch<'a>(
        &'a self,
        model: &'a M,
        attributes: &'a Attributes,
        visited: &'a mut TouchedOwners,
        transaction: Option<&'a mut Transaction>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;
}

impl<M: Model, R: Model> Touch<M> for BelongsTo<M, R> {
    fn touch<'a>(
        &'a self,
        model: &'a M,
        attributes: &'a Attributes,
        visited: &'a mut TouchedOwners,
        transaction: Option<&'a mut Transaction>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(self.touch_owner(model, attributes, visited, transaction))
    }
}

impl<M: Model> Touch<M> for MorphTo<M> {
    fn touch<'a>(
        &'a self,
        model: &'a M,
        attributes: &'a Attributes,
        visited: &'a mut TouchedOwners,
        transaction: Option<&'a mut Transaction>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(self.touch_owner(model, attributes, visited, transaction))
    }
}

/// Typed relationships corresponding to Laravel's model `$touches` list.
pub struct Touches<M: Model>(Vec<Box<dyn Touch<M>>>);

impl<M: Model> Default for Touches<M> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<M: Model> Touches<M> {
    /// Adds a belongs-to relationship to touch after a dirty save.
    #[must_use]
    #[allow(non_snake_case, reason = "Matches Laravel's relationship name")]
    pub fn belongsTo<R: Model>(mut self, relation: BelongsTo<M, R>) -> Self {
        self.0.push(Box::new(relation));
        self
    }

    /// Adds a polymorphic belongs-to relationship to touch after a dirty save.
    #[must_use]
    #[allow(non_snake_case, reason = "Matches Laravel's relationship name")]
    pub fn morphTo(mut self, relation: MorphTo<M>) -> Self {
        self.0.push(Box::new(relation));
        self
    }

    pub(super) async fn run(
        self,
        model: &M,
        attributes: &Attributes,
        visited: &mut TouchedOwners,
        mut transaction: Option<&mut Transaction>,
    ) -> DatabaseResult<()> {
        for touch in self.0 {
            touch
                .touch(model, attributes, visited, transaction.as_deref_mut())
                .await?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct TouchedOwners(Vec<(TypeId, Connection, Vec<u8>)>);

impl TouchedOwners {
    pub(super) fn visit<M: Model>(
        &mut self,
        connection: &Connection,
        key: Option<&Value>,
    ) -> DatabaseResult<bool> {
        let Some(key) = dictionary_key(key)? else {
            return Ok(true);
        };
        let model = TypeId::of::<M>();
        if self.0.iter().any(|(kind, source, id)| {
            *kind == model && *id == key && source.same_query_source(connection)
        }) {
            return Ok(false);
        }
        self.0.push((model, connection.clone(), key));
        Ok(true)
    }
}

pub(super) async fn touch_owners<M: Model>(
    model: &M,
    persisted: &Attributes,
    connection: &Connection,
    transaction: Option<&mut Transaction>,
) -> DatabaseResult<()> {
    let touches = M::touches();
    if touches.0.is_empty() || *persisted == model.state().original {
        return Ok(());
    }
    let mut visited = TouchedOwners::default();
    visited.visit::<M>(connection, persisted.get(M::metadata().primary_key))?;
    touches
        .run(model, persisted, &mut visited, transaction)
        .await
}
