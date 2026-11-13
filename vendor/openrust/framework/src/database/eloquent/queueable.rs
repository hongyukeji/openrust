use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    marker::PhantomData,
};

use serde::{Deserialize, Serialize};

use crate::database::{DB, DatabaseError, DatabaseResult, Value};

use super::{Builder, Collection, Model, ModelState, relations::relation::dictionary_key};

thread_local! {
    static VISITING: RefCell<HashSet<usize>> = RefCell::default();
}

struct RelationGuard(usize);

impl Drop for RelationGuard {
    fn drop(&mut self) {
        VISITING.with(|models| models.borrow_mut().remove(&self.0));
    }
}

pub(super) fn model_relations(state: &ModelState) -> Vec<String> {
    let identity = std::ptr::from_ref(state).addr();
    if !VISITING.with(|models| models.borrow_mut().insert(identity)) {
        return Vec::new();
    }
    let _guard = RelationGuard(identity);
    match &state.detached_autoload {
        Some(link) => link.visible_relations(state, |_| true).queueable_paths(),
        None => state.relations.queueable_paths(),
    }
}

pub(super) fn common_relations<'a, M: Model + 'a>(
    mut models: impl Iterator<Item = &'a M>,
) -> Vec<String> {
    let Some(first) = models.next() else {
        return Vec::new();
    };
    let mut paths = first.getQueueableRelations();
    for model in models {
        if paths.is_empty() {
            break;
        }
        let loaded: HashSet<_> = model.getQueueableRelations().into_iter().collect();
        paths.retain(|path| loaded.contains(path));
    }
    paths
}

/// A queued model's identity and loaded relation paths. The job declares the model type.
#[derive(Serialize, Deserialize)]
#[serde(bound = "")]
pub struct ModelIdentifier<M: Model> {
    id: ModelKey,
    connection: Option<String>,
    relations: Vec<String>,
    #[serde(skip)]
    model: PhantomData<fn() -> M>,
}

/// The ordered identities and shared loaded relation paths of a queued collection.
#[derive(Serialize, Deserialize)]
#[serde(bound = "")]
pub struct ModelCollectionIdentifier<M: Model> {
    ids: Vec<ModelKey>,
    connection: Option<String>,
    relations: Vec<String>,
    #[serde(skip)]
    model: PhantomData<fn() -> M>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum ModelKey {
    Null,
    Bool(bool),
    Integer(i64),
    Unsigned(u64),
    Real(f64),
    Text(String),
    Bytes(Vec<u8>),
}

impl TryFrom<Value> for ModelKey {
    type Error = DatabaseError;

    fn try_from(value: Value) -> DatabaseResult<Self> {
        Ok(match value {
            Value::Null => Self::Null,
            Value::Bool(value) => Self::Bool(value),
            Value::Integer(value) => Self::Integer(value),
            Value::Unsigned(value) => Self::Unsigned(value),
            Value::Real(value) if value.is_finite() => Self::Real(value),
            Value::Text(value) => Self::Text(value),
            Value::Bytes(value) => Self::Bytes(value),
            _ => return Err(DatabaseError::InvalidQuery("queued model key type")),
        })
    }
}

impl From<ModelKey> for Value {
    fn from(key: ModelKey) -> Self {
        match key {
            ModelKey::Null => Self::Null,
            ModelKey::Bool(value) => Self::Bool(value),
            ModelKey::Integer(value) => Self::Integer(value),
            ModelKey::Unsigned(value) => Self::Unsigned(value),
            ModelKey::Real(value) => Self::Real(value),
            ModelKey::Text(value) => Self::Text(value),
            ModelKey::Bytes(value) => Self::Bytes(value),
        }
    }
}

fn restoration_query<M: Model>(connection: Option<&str>) -> DatabaseResult<Builder<'static, M>> {
    let connection = DB::connection(connection.or(M::metadata().connection))?;
    Ok(M::queryOn(&connection).withoutGlobalScopes().useWritePdo())
}

#[allow(non_snake_case, reason = "Matches Laravel's queue naming")]
impl<M: Model> ModelIdentifier<M> {
    pub fn capture(model: &M) -> DatabaseResult<Self> {
        Ok(Self {
            id: model
                .getQueueableId()
                .cloned()
                .unwrap_or(Value::Null)
                .try_into()?,
            connection: model.getQueueableConnection().map(str::to_owned),
            relations: model.getQueueableRelations(),
            model: PhantomData,
        })
    }

    #[must_use]
    pub fn withoutRelations(mut self) -> Self {
        self.relations.clear();
        self
    }

    /// Resolves the model using the current application's database configuration.
    pub async fn restore(self) -> DatabaseResult<M> {
        if matches!(self.id, ModelKey::Null) {
            return Err(DatabaseError::ModelNotFound(M::metadata().table));
        }
        let mut model = restoration_query::<M>(self.connection.as_deref())?
            .findOrFail(Value::from(self.id))
            .await?;
        let paths: Vec<_> = self.relations.iter().map(String::as_str).collect();
        model.loadMissingPaths(&paths).await?;
        Ok(model)
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's queue naming")]
impl<M: Model> ModelCollectionIdentifier<M> {
    pub fn capture(models: &Collection<M>) -> DatabaseResult<Self> {
        let connection = models.getQueueableConnection()?.map(str::to_owned);
        Ok(Self {
            ids: models
                .getQueueableIds()
                .into_iter()
                .map(ModelKey::try_from)
                .collect::<DatabaseResult<_>>()?,
            connection,
            relations: models.getQueueableRelations(),
            model: PhantomData,
        })
    }

    #[must_use]
    pub fn withoutRelations(mut self) -> Self {
        self.relations.clear();
        self
    }

    /// Restores existing models in payload order and omits missing rows.
    pub async fn restore(self) -> DatabaseResult<Collection<M>> {
        let mut keys = Vec::with_capacity(self.ids.len());
        let mut bindings = indexmap::IndexMap::new();
        for id in self.ids {
            let value = Value::from(id);
            if let Some(key) = dictionary_key(Some(&value))? {
                bindings.entry(key.clone()).or_insert(value);
                keys.push(key);
            }
        }
        if keys.is_empty() {
            return Ok(Collection::new(Vec::new()));
        }
        let mut found = HashMap::new();
        let bindings: Vec<_> = bindings.into_values().collect();
        for batch in bindings.chunks(500) {
            let mut query = restoration_query::<M>(self.connection.as_deref())?;
            query.whereIn(M::metadata().primary_key, batch.to_vec());
            for model in query.get().await? {
                if let Some(key) = dictionary_key(model.getKey())? {
                    found.insert(key, model);
                }
            }
        }
        let models = keys
            .iter()
            .filter_map(|key| found.get(key))
            .map(|model| M::from_state(model.state().clone()))
            .collect();
        let mut collection = Collection::new(models);
        if !collection.is_empty() {
            let paths: Vec<_> = self.relations.iter().map(String::as_str).collect();
            collection.loadMissingPaths(&paths).await?;
        }
        Ok(collection)
    }
}
