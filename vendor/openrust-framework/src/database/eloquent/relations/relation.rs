use std::{collections::HashMap, marker::PhantomData, sync::Arc};

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Db, Transaction, Value, query,
};

use super::{
    super::{
        Attributes, Builder, Model, ModelState,
        concerns::{
            has_events::{ModelEvent, fire_with_transaction},
            has_timestamps::timestamp,
        },
        touches::TouchedOwners,
    },
    one_of_many::OneOfMany,
};

/// The typed result and batch matching contract shared by relationships.
pub trait Relation: Send + Sync + 'static {
    type Parent: Model;
    type Output: Send + Sync + 'static;

    /// Identifies an empty collection output without projecting its models.
    #[doc(hidden)]
    fn is_empty_collection(_output: &Self::Output) -> bool {
        false
    }

    /// Projects a loaded relationship into its JSON representation.
    fn serialize(_output: &Self::Output) -> DatabaseResult<Option<serde_json::Value>> {
        Ok(None)
    }

    /// Returns the loaded relationship paths shared by this relation's related models.
    fn queueable_relations(_output: &Self::Output) -> Vec<String> {
        Vec::new()
    }

    /// Associates model outputs with a shared automatic loading batch.
    #[doc(hidden)]
    fn attach_autoload(_outputs: &mut [Self::Output]) {}

    /// Copies the loaded output for automatic loading propagation.
    #[doc(hidden)]
    fn autoload_output(output: &Self::Output) -> Option<Self::Output> {
        let _ = output;
        None
    }

    /// Saves model snapshots in a loaded output and returns the replacement output.
    #[doc(hidden)]
    fn push_loaded<'a>(
        _output: &'a Self::Output,
        _context: &'a mut super::PushContext,
    ) -> DatabaseFuture<'a, (Option<Self::Output>, DatabaseResult<bool>)> {
        Box::pin(async { (None, Ok(true)) })
    }

    /// Returns one result per parent, in the original order.
    #[track_caller]
    fn match_results<'a>(
        &'a self,
        parents: &'a [&'a Self::Parent],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>>;

    /// Matches related rows while borrowing an explicit transaction.
    #[track_caller]
    fn match_results_in_transaction<'a>(
        &'a self,
        _parents: &'a [&'a Self::Parent],
        _transaction: &'a mut Transaction,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(async {
            Err(DatabaseError::InvalidQuery(
                "relation does not support transaction loading",
            ))
        })
    }

    #[track_caller]
    fn get_results<'a>(
        &'a self,
        parent: &'a Self::Parent,
    ) -> DatabaseFuture<'a, DatabaseResult<Self::Output>> {
        Box::pin(crate::database::events::with_query_caller(
            std::panic::Location::caller(),
            async move {
                let mut results = self.match_results(&[parent]).await?;
                if results.len() != 1 {
                    return Err(DatabaseError::InvalidQuery("relation result cardinality"));
                }
                Ok(results.pop().expect("checked result cardinality"))
            },
        ))
    }
}

/// A relationship whose related model can load nested paths.
pub trait NestedRelation: Relation {
    type Related: Model;

    #[doc(hidden)]
    fn with_nested(self, nested: super::super::EagerLoads<Self::Related>) -> Self;

    #[doc(hidden)]
    fn with_path_columns(self, _columns: Vec<String>) -> DatabaseResult<Self>
    where
        Self: Sized,
    {
        Err(DatabaseError::InvalidQuery(
            "relationship path does not support column selection",
        ))
    }

    #[doc(hidden)]
    fn with_path_constraint(
        self,
        _constraint: impl Fn(query::Conditions, &str) -> query::Conditions + Send + Sync + 'static,
    ) -> DatabaseResult<Self>
    where
        Self: Sized,
    {
        Err(DatabaseError::InvalidQuery(
            "relationship path does not support constraints",
        ))
    }
}

/// Builds a correlated subquery for filtering parent models by a relationship.
pub trait ExistenceRelation: Relation {
    fn get_relation_existence_query(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>>;
}

/// A typed relation whose related query accepts additional constraints.
pub trait ConstrainableRelation: ExistenceRelation {
    type Related: Model;

    fn constrain_query(
        self,
        configure: impl Fn(Builder<'static, Self::Related>) -> Builder<'static, Self::Related>
        + Send
        + Sync
        + 'static,
    ) -> Self;
}

type Configure<R> =
    dyn Fn(Builder<'static, R>) -> DatabaseResult<Builder<'static, R>> + Send + Sync;

pub(super) struct RelationKeys<P, R: Model> {
    pub parent: &'static str,
    pub related: &'static str,
    configure: Option<Box<Configure<R>>>,
    pub(super) morph_constraint: Option<Box<Configure<R>>>,
    pub selection: Option<OneOfMany>,
    pub(super) inverse: Option<&'static str>,
    pub discriminator: Option<(&'static str, &'static str)>,
    models: PhantomData<fn() -> (P, R)>,
}

impl<P: Model, R: Model> RelationKeys<P, R> {
    pub const fn new(parent: &'static str, related: &'static str) -> Self {
        Self {
            parent,
            related,
            configure: None,
            morph_constraint: None,
            discriminator: None,
            inverse: None,
            selection: None,
            models: PhantomData,
        }
    }

    pub(super) fn for_parent<M: Model>(self) -> RelationKeys<M, R> {
        RelationKeys {
            parent: self.parent,
            related: self.related,
            configure: self.configure,
            morph_constraint: self.morph_constraint,
            selection: self.selection,
            discriminator: self.discriminator,
            inverse: self.inverse,
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

    pub fn append_constraint(
        &mut self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) {
        self.append_query_configuration(move |query| Ok(configure(query)));
    }

    pub fn append_query_configuration(
        &mut self,
        configure: impl Fn(Builder<'static, R>) -> DatabaseResult<Builder<'static, R>>
        + Send
        + Sync
        + 'static,
    ) {
        let previous = self.configure.take();
        self.try_constrain(move |mut query| {
            if let Some(previous) = &previous {
                query = previous(query)?;
            }
            configure(query)
        });
    }

    pub fn with_nested(&mut self, nested: super::super::EagerLoads<R>) {
        self.append_constraint(move |query| query.with_eager_loads(nested.clone()));
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
        let query = match &self.morph_constraint {
            Some(constraint) => constraint(query)?,
            None => query,
        };
        Ok(if let Some((column, alias)) = self.discriminator {
            let column = query.qualify_column(column);
            query.require_relation_value(&column, alias)
        } else {
            query
        })
    }

    pub(super) fn owner_query(&self, child: &P) -> DatabaseResult<Option<Builder<'static, R>>> {
        let keys = ParentKeys::new(&[child], self.parent)?;
        if keys.bindings.is_empty() {
            return Ok(None);
        }
        let connection = self.connection(child)?;
        let query = self.configure(R::query_on(&connection))?;
        let owner = query.qualify_column(self.related);
        Ok(Some(query.restrict_relation(&owner, &keys.bindings)))
    }

    pub fn connection(&self, parent: &P) -> DatabaseResult<Connection> {
        let resolve = || {
            let related = R::from_state(Default::default());
            let name = related
                .resolve_connection_name()?
                .map(|name| name.into_owned());
            if let Some(name) = name {
                if let Some(connection) = parent.state().connection.as_ref() {
                    if connection.get_name() == name {
                        return Ok(connection.clone());
                    }
                    if let Some(container) = Connection::model_container(Some(connection)) {
                        use crate::contracts::container::Container as _;
                        return container
                            .get::<crate::database::DatabaseManager>("db")?
                            .connection(Some(&name));
                    }
                }
                return Db::connection(Some(&name));
            }
            parent.get_connection()
        };
        if let Some(owner) = parent
            .state()
            .connection
            .as_ref()
            .and_then(|connection| Connection::model_container(Some(connection)))
        {
            crate::support::facades::application_scope_sync(owner, resolve)
        } else {
            resolve()
        }
    }

    pub(super) fn shares_related_source(&self, parent: &P, related: &R) -> bool {
        if related.get_table() != R::metadata().table {
            return false;
        }
        if let (Some(_), Some(actual)) = (&parent.state().connection, &related.state().connection) {
            return self
                .connection(parent)
                .is_ok_and(|expected| expected.same_query_source(actual));
        }
        // Unsaved, unbound models compare declared identities without resolving a database.
        let name = R::metadata()
            .connection
            .or_else(|| parent.get_connection_name());
        related.get_connection_name() == name
    }

    pub(super) fn matches_identity(&self, parent: &P, related: &R) -> bool {
        self.shares_related_source(parent, related)
            && compare_relation_keys(
                parent.get_attribute(self.parent),
                related.get_attribute(self.related),
            )
    }

    pub(super) async fn matches_selected(&self, parent: &P, related: &R) -> DatabaseResult<bool> {
        if !self.matches_identity(parent, related) {
            return Ok(false);
        }
        if self.selection.is_none() {
            return Ok(true);
        }

        let (Some(parent_key), Some(related_key)) = (
            parent.get_attribute(self.parent),
            related.get_key().filter(|key| !matches!(key, Value::Null)),
        ) else {
            return Ok(false);
        };
        let connection = self.connection(parent)?;
        let query = self.select_one(self.configure(R::query_on(&connection))?)?;
        let foreign = query.qualify_column(self.related);
        let primary = query.qualify_column(R::metadata().primary_key);
        query
            .restrict_relation(&foreign, std::slice::from_ref(parent_key))
            .restrict_relation(&primary, std::slice::from_ref(related_key))
            .scoped()?
            .0
            .exists()
            .await
    }

    pub(super) async fn touch_owner(
        &self,
        model: &P,
        attributes: &Attributes,
        visited: &mut TouchedOwners,
        mut transaction: Option<&mut Transaction>,
    ) -> DatabaseResult<()> {
        let Some(foreign_key) = attributes
            .get(self.parent)
            .filter(|key| **key != Value::Null)
        else {
            return Ok(());
        };
        let connection = self.connection(model)?;
        let relation = self
            .configure(R::query_on(&connection))?
            .restrict_relation(self.related, std::slice::from_ref(foreign_key));
        let casts = relation.casts.clone();
        let query = relation.scoped()?.0.use_write_pdo();
        let row = match transaction.as_deref_mut() {
            Some(tx) => query.in_transaction(tx)?.first().await?,
            None => query.first().await?,
        };
        let Some(row) = row else {
            return Ok(());
        };
        let mut attributes: Attributes = row.into_columns().into_iter().collect();
        let metadata = R::metadata();
        let key = attributes
            .get(metadata.primary_key)
            .filter(|key| **key != Value::Null)
            .cloned()
            .ok_or(DatabaseError::InvalidQuery("touch owner primary key"))?;
        if !visited.visit::<R>(&connection, Some(&key))? {
            return Ok(());
        }
        if let Some(column) = metadata.updated_at
            && !R::is_ignoring_touch()
        {
            let time = timestamp(&connection, metadata.date_format)?;
            let query = self
                .configure(R::query_on(&connection))?
                .restrict_relation(self.related, std::slice::from_ref(foreign_key))
                .restrict_relation(metadata.primary_key, &[key])
                .scoped()?
                .0;
            let update = Attributes::from([(column.into(), time.clone())]);
            match transaction.as_deref_mut() {
                Some(tx) => query.in_transaction(tx)?.update(update).await?,
                None => query.update(update).await?,
            };
            attributes.insert(column.into(), time);
        }
        let mut owner = R::from_state(ModelState::hydrated_attributes(
            attributes.clone(),
            connection.clone(),
            casts,
        ));
        fire_with_transaction(
            &mut owner,
            &connection,
            ModelEvent::Saved,
            false,
            transaction.as_deref_mut(),
        )
        .await?;
        R::touches()
            .run(&owner, &attributes, visited, transaction)
            .await
    }

    pub fn existence_builder(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<Builder<'static, R>> {
        if R::metadata()
            .connection
            .is_some_and(|name| name != connection.get_name())
        {
            return Err(DatabaseError::InvalidQuery(
                "relationship existence requires one connection",
            ));
        }
        let mut query = R::query_on(connection);
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
        let related = query.qualify_column(self.related);
        let parent = format!("{parent_table}.{}", self.parent);
        Ok(query.scoped()?.0.where_column(&related, "=", &parent))
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

    pub(super) fn select_one(
        &self,
        query: Builder<'static, R>,
    ) -> DatabaseResult<Builder<'static, R>> {
        match &self.selection {
            Some(selection) => selection.apply(query, self.related, self.discriminator),
            None => Ok(query),
        }
    }

    fn inverse_parent(parent: &P) -> Arc<P> {
        let mut state = parent.state().clone();
        state.relations.clear();
        state.detached_autoload = None;
        Arc::new(P::from_state(state))
    }

    pub(super) fn hydrate_inverse(&self, parent: &P, model: &mut R) {
        if let Some(name) = self.inverse {
            self.set_inverse(name, Self::inverse_parent(parent), model);
        }
    }

    fn set_inverse(&self, name: &str, parent: Arc<P>, model: &mut R) {
        if let Some((_, alias)) = self.discriminator {
            model.set_relation::<super::MorphTo<R>>(
                name,
                Some(super::MorphTarget::from_model(alias, parent)),
            );
        } else {
            model.set_relation::<super::BelongsTo<R, P>>(name, Some(parent));
        }
    }

    pub async fn match_many(
        &self,
        parents: &[&P],
        transaction: Option<&mut Transaction>,
    ) -> DatabaseResult<Vec<Vec<Arc<R>>>> {
        self.match_many_with(parents, transaction, |query| query)
            .await
    }

    pub(super) async fn match_many_with(
        &self,
        parents: &[&P],
        mut transaction: Option<&mut Transaction>,
        prepare: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send,
    ) -> DatabaseResult<Vec<Vec<Arc<R>>>> {
        let keys = ParentKeys::new(parents, self.parent)?;
        let mut dictionary: HashMap<_, Vec<_>> = HashMap::new();
        if !keys.bindings.is_empty() {
            let connection = self.batch_connection(parents)?;
            // Bound the number of SQL parameters for SQLite as well as MySQL.
            for batch in keys.bindings.chunks(500) {
                let query = self
                    .select_one(self.configure(prepare(R::query_on(&connection)))?)?
                    .restrict_relation(self.related, batch);
                for model in query
                    .get_models_with_transaction(transaction.as_deref_mut())
                    .await?
                {
                    if let Some(key) = dictionary_key(model.get_attribute(self.related))? {
                        dictionary.entry(key).or_default().push(Arc::new(model));
                    }
                }
            }
        }
        let mut matches = keys.match_models(&dictionary);
        if let Some(name) = self.inverse {
            for (parent, children) in parents.iter().zip(&mut matches) {
                if !children.is_empty() {
                    let parent = Self::inverse_parent(parent);
                    for child in children {
                        let mut model = R::from_state(child.state().clone());
                        self.set_inverse(name, parent.clone(), &mut model);
                        *child = Arc::new(model);
                    }
                }
            }
        }
        Ok(matches)
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
            .map(|parent| dictionary_key(parent.get_attribute(column)))
            .collect::<DatabaseResult<Vec<_>>>()?;
        let mut values = indexmap::IndexMap::new();
        for (parent, key) in parents.iter().zip(&keys) {
            if let Some(key) = key {
                values.entry(key).or_insert_with(|| {
                    parent
                        .get_attribute(column)
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

fn compare_relation_keys(parent: Option<&Value>, related: Option<&Value>) -> bool {
    fn nonempty(key: &Value) -> bool {
        match key {
            Value::Null | Value::Bool(false) | Value::Integer(0) | Value::Unsigned(0) => false,
            Value::Real(value) => value.is_finite() && *value != 0.0,
            Value::Text(value) => !value.is_empty() && value != "0",
            Value::Bytes(value) => !value.is_empty() && value != b"0",
            _ => true,
        }
    }

    fn integer(key: &Value) -> Option<i128> {
        match key {
            Value::Integer(value) => Some((*value).into()),
            Value::Unsigned(value) => Some((*value).into()),
            Value::Text(value) => value.trim().parse().ok(),
            Value::Bytes(value) => std::str::from_utf8(value).ok()?.trim().parse().ok(),
            _ => None,
        }
    }

    let (Some(parent), Some(related)) = (
        parent.filter(|key| nonempty(key)),
        related.filter(|key| nonempty(key)),
    ) else {
        return false;
    };
    if matches!(parent, Value::Integer(_) | Value::Unsigned(_))
        || matches!(related, Value::Integer(_) | Value::Unsigned(_))
    {
        return integer(parent)
            .zip(integer(related))
            .is_some_and(|(parent, related)| parent == related);
    }
    parent == related
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
