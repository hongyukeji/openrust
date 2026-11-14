use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{Attributes, Model, ModelState, concerns::has_relationships::LoadedRelations},
};

/// The relationship identity carried by a hydrated intermediate model.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::database::eloquent) struct PivotIdentity {
    pub table: &'static str,
    pub foreign: &'static str,
    pub related: &'static str,
    pub morph: Option<(&'static str, &'static str)>,
    pub timestamps: (Option<&'static str>, Option<&'static str>),
}

impl PivotIdentity {
    pub(in crate::database::eloquent) fn columns(self) -> impl Iterator<Item = &'static str> {
        [self.foreign, self.related]
            .into_iter()
            .chain(self.morph.map(|(column, _)| column))
    }

    pub(in crate::database::eloquent) fn constrain<'a>(
        self,
        mut query: crate::database::query::Builder<'a>,
        original: &Attributes,
    ) -> DatabaseResult<crate::database::query::Builder<'a>> {
        for column in self.columns() {
            let value = original
                .get(column)
                .filter(|value| **value != Value::Null)
                .ok_or(DatabaseError::InvalidQuery("missing pivot identity"))?;
            query = query.r#where(column, "=", value.clone());
        }
        Ok(query)
    }
}

type Hydrate = fn(
    &mut LoadedRelations,
    &str,
    Attributes,
    &Connection,
    PivotIdentity,
    LoadedRelations,
) -> DatabaseResult<()>;
type Insert = for<'a> fn(
    &'a mut Transaction,
    &'a Connection,
    PivotIdentity,
    Attributes,
) -> DatabaseFuture<'a, DatabaseResult<bool>>;
type Update = for<'a> fn(
    &'a mut Transaction,
    &'a Connection,
    PivotIdentity,
    Attributes,
    Attributes,
) -> DatabaseFuture<'a, DatabaseResult<bool>>;
type Delete = for<'a> fn(
    &'a mut Transaction,
    &'a Connection,
    PivotIdentity,
    Attributes,
) -> DatabaseFuture<'a, DatabaseResult<bool>>;

#[derive(Clone, Copy)]
pub(super) struct CustomPivot {
    pub inverse_relations: &'static [&'static str],
    pub primary_key: Option<&'static str>,
    pub hydrate: Hydrate,
    pub insert: Insert,
    pub update: Update,
    pub delete: Delete,
}

impl CustomPivot {
    pub fn new<M: Model>() -> Self {
        Self {
            inverse_relations: M::pivotInverseRelations(),
            primary_key: M::getIncrementing().then_some(M::metadata().primary_key),
            hydrate: |relations, accessor, attributes, connection, identity, inverses| {
                let mut model = hydrated::<M>(attributes, connection, identity)?;
                model.state_mut().relations = inverses;
                relations.insert_model(accessor.into(), model, |model| {
                    model.toArray().map(serde_json::Value::Object).map(Some)
                });
                Ok(())
            },
            insert: |transaction, connection, identity, attributes| {
                Box::pin(async move {
                    let mut model = hydrated::<M>(attributes.clone(), connection, identity)?;
                    model.state_mut().exists = false;
                    model.state_mut().attributes.clear();
                    for (column, value) in attributes {
                        if identity.columns().any(|key| key == column) {
                            model.state_mut().setAttribute(column, value);
                        } else {
                            model.setAttributeValueAsync(&column, value).await?;
                        }
                    }
                    model.saveInTransaction(transaction).await
                })
            },
            update: |transaction, connection, identity, raw, attributes| {
                Box::pin(async move {
                    let mut model = hydrated::<M>(raw, connection, identity)?;
                    model.fillAsync(attributes).await?;
                    if model.getDirty()?.is_empty() {
                        return Ok(false);
                    }
                    model.saveInTransaction(transaction).await
                })
            },
            delete: |transaction, connection, identity, raw| {
                Box::pin(async move {
                    hydrated::<M>(raw, connection, identity)?
                        .deleteInTransaction(transaction)
                        .await
                })
            },
        }
    }
}

fn hydrated<M: Model>(
    attributes: Attributes,
    connection: &Connection,
    identity: PivotIdentity,
) -> DatabaseResult<M> {
    if M::metadata().deleted_at.is_some() {
        return Err(DatabaseError::InvalidQuery(
            "custom pivot models cannot use soft deletes",
        ));
    }
    let mut state =
        ModelState::hydrated_attributes(attributes, connection.clone(), Default::default());
    state.pivot_identity = Some(identity);
    Ok(M::from_state(state))
}
