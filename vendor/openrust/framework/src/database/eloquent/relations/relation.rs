use std::{collections::HashMap, marker::PhantomData, sync::Arc};

use crate::database::{
    Connection, DB, DatabaseError, DatabaseFuture, DatabaseResult, Value, query,
};

use super::{
    super::{Builder, Model},
    one_of_many::OneOfMany,
};

/// The typed result and batch matching contract shared by relationships.
#[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
pub trait Relation: Send + Sync + 'static {
    type Parent: Model;
    type Output: Send + Sync + 'static;

    /// Projects a loaded relationship into its JSON representation.
    /// Returns one result per parent, in the original order.
    fn matchResults<'a>(
        &'a self,
        parents: &'a [&'a Self::Parent],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>>;

    fn getResults<'a>(
        &'a self,
        parent: &'a Self::Parent,
    ) -> DatabaseFuture<'a, DatabaseResult<Self::Output>> {
        Box::pin(async move {
            let mut results = self.matchResults(&[parent]).await?;
            if results.len() != 1 {
                return Err(DatabaseError::InvalidQuery("relation result cardinality"));
            }
            Ok(results.pop().expect("checked result cardinality"))
        })
    }
}

/// Builds a correlated subquery for filtering parent models by a relationship.
#[allow(non_snake_case, reason = "Matches Laravel's relation query contract")]
pub trait ExistenceRelation: Relation {
    fn getRelationExistenceQuery(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>>;
}

type Configure<R> =
    dyn Fn(Builder<'static, R>) -> DatabaseResult<Builder<'static, R>> + Send + Sync;

pub(super) struct RelationKeys<P, R: Model> {
    pub parent: &'static str,
    pub related: &'static str,
    configure: Option<Box<Configure<R>>>,
    pub selection: Option<OneOfMany>,
    pub discriminator: Option<(&'static str, &'static str)>,
    models: PhantomData<fn() -> (P, R)>,
}

impl<P: Model, R: Model> RelationKeys<P, R> {
    pub const fn new(parent: &'static str, related: &'static str) -> Self {
        Self {
            parent,
            related,
            configure: None,
            discriminator: None,
            selection: None,
            models: PhantomData,
        }
    }

    pub const fn with_discriminator(mut self, column: &'static str, alias: &'static str) -> Self {
        self.discriminator = Some((column, alias));
        self
    }

    pub fn constrain(
        &mut self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) {
        self.try_constrain(move |query| Ok(configure(query)));
    }

    pub fn try_constrain(
        &mut self,
        configure: impl Fn(Builder<'static, R>) -> DatabaseResult<Builder<'static, R>>
        + Send
        + Sync
        + 'static,
    ) {
        self.configure = Some(Box::new(configure));
    }

    pub fn configure(&self, query: Builder<'static, R>) -> DatabaseResult<Builder<'static, R>> {
        let query = match &self.configure {
            Some(configure) => configure(query)?,
            None => query,
        };
        Ok(if let Some((column, alias)) = self.discriminator {
            let column = query.qualifyColumn(column);
            query.require_relation_value(&column, alias)
        } else {
            query
        })
    }

    pub fn connection(&self, parent: &P) -> DatabaseResult<Connection> {
        if let Some(name) = R::metadata().connection
            && parent.getConnectionName() != Some(name)
        {
            return DB::connection(Some(name));
        }
        match &parent.state().connection {
            Some(connection) => Ok(connection.clone()),
            None => DB::connection(R::metadata().connection.or(P::metadata().connection)),
        }
    }

    pub fn existence_builder(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<Builder<'static, R>> {
        if R::metadata()
            .connection
            .is_some_and(|name| name != connection.getName())
        {
            return Err(DatabaseError::InvalidQuery(
                "relationship existence requires one connection",
            ));
        }
        let mut query = R::queryOn(connection);
        if parent_table == R::metadata().table {
            let alias = if parent_table == "laravel_reserved_0" {
                "laravel_reserved_1"
            } else {
                "laravel_reserved_0"
            };
            query = query.alias_relation(alias);
        }
        self.select_one(self.configure(query)?)
    }

    pub fn existence_query(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>> {
        let query = self.existence_builder(connection, parent_table)?;
        let related = query.qualifyColumn(self.related);
        let parent = format!("{parent_table}.{}", self.parent);
        Ok(query.scoped()?.0.whereColumn(&related, "=", &parent))
    }

    pub fn batch_connection(&self, parents: &[&P]) -> DatabaseResult<Connection> {
        let first = parents
            .first()
            .ok_or(DatabaseError::InvalidQuery("empty relation batch"))?;
        let connection = self.connection(first)?;
        for parent in &parents[1..] {
            if let (Some(first), Some(other)) =
                (&first.state().connection, &parent.state().connection)
                && !first.same_query_source(other)
            {
                return Err(DatabaseError::InvalidQuery(
                    "mixed relationship connections",
                ));
            }
            if !connection.same_query_source(&self.connection(parent)?) {
                return Err(DatabaseError::InvalidQuery(
                    "mixed relationship connections",
                ));
            }
        }
        Ok(connection)
    }

    fn select_one(&self, query: Builder<'static, R>) -> DatabaseResult<Builder<'static, R>> {
        match &self.selection {
            Some(selection) => selection.apply(query, self.related, self.discriminator),
            None => Ok(query),
        }
    }

    pub async fn match_many(&self, parents: &[&P]) -> DatabaseResult<Vec<Vec<Arc<R>>>> {
        let keys = ParentKeys::new(parents, self.parent)?;
        let mut dictionary: HashMap<_, Vec<_>> = HashMap::new();
        if !keys.bindings.is_empty() {
            let connection = self.batch_connection(parents)?;
            // Bound the number of SQL parameters for SQLite as well as MySQL.
            for batch in keys.bindings.chunks(500) {
                let query = self
                    .select_one(self.configure(R::queryOn(&connection))?)?
                    .restrict_relation(self.related, batch);
                for model in query.get().await? {
                    if let Some(key) = dictionary_key(model.getAttribute(self.related))? {
                        dictionary.entry(key).or_default().push(Arc::new(model));
                    }
                }
            }
        }
        Ok(keys.match_models(&dictionary))
    }
}

pub(super) struct ParentKeys {
    keys: Vec<Option<Vec<u8>>>,
    pub bindings: Vec<Value>,
}

impl ParentKeys {
    pub fn new<P: Model>(parents: &[&P], column: &str) -> DatabaseResult<Self> {
        let keys = parents
            .iter()
            .map(|parent| dictionary_key(parent.getAttribute(column)))
            .collect::<DatabaseResult<Vec<_>>>()?;
        let mut values = indexmap::IndexMap::new();
        for (parent, key) in parents.iter().zip(&keys) {
            if let Some(key) = key {
                values.entry(key).or_insert_with(|| {
                    parent
                        .getAttribute(column)
                        .expect("key came from parent")
                        .clone()
                });
            }
        }
        let bindings = values.into_values().collect();
        Ok(Self { keys, bindings })
    }

    pub fn match_models<R>(&self, dictionary: &HashMap<Vec<u8>, Vec<Arc<R>>>) -> Vec<Vec<Arc<R>>> {
        self.keys
            .iter()
            .map(|key| {
                key.as_ref()
                    .and_then(|key| dictionary.get(key))
                    .cloned()
                    .unwrap_or_default()
            })
            .collect()
    }
}

pub(in crate::database::eloquent) fn dictionary_key(
    value: Option<&Value>,
) -> DatabaseResult<Option<Vec<u8>>> {
    let key = match value {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Integer(value)) => value.to_string().into_bytes(),
        Some(Value::Unsigned(value)) => value.to_string().into_bytes(),
        Some(Value::Bool(value)) => {
            if *value {
                b"1".to_vec()
            } else {
                b"0".to_vec()
            }
        }
        Some(Value::Real(value)) if value.is_finite() => value.to_string().into_bytes(),
        Some(Value::Text(value)) => value.as_bytes().to_vec(),
        Some(Value::Bytes(value)) => value.clone(),
        _ => return Err(DatabaseError::InvalidQuery("relationship key type")),
    };
    Ok(Some(key))
}
