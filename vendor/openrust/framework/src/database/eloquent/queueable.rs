use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    marker::PhantomData,
};

use serde::{Deserialize, Serialize};

use crate::database::{DB, DatabaseError, DatabaseResult, Value, query::Conditions};

use super::{
    Builder, Collection, Model, ModelState,
    relations::{custom_pivot::PivotIdentity, relation::dictionary_key},
};

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
    id: IdentifierKey,
    connection: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pivot: Option<PivotDefinition>,
    relations: Vec<String>,
    #[serde(skip)]
    model: PhantomData<fn() -> M>,
}

/// The ordered identities and shared loaded relation paths of a queued collection.
#[derive(Serialize, Deserialize)]
#[serde(bound = "")]
pub struct ModelCollectionIdentifier<M: Model> {
    ids: Vec<IdentifierKey>,
    connection: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pivot: Option<PivotDefinition>,
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

/// A compiled relationship definition permitted to restore a custom pivot.
pub struct QueueablePivot(pub(super) PivotIdentity);

#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct PivotDefinition {
    table: String,
    columns: Vec<String>,
    timestamps: (Option<String>, Option<String>),
    morph_alias: Option<String>,
}

impl From<PivotIdentity> for PivotDefinition {
    fn from(identity: PivotIdentity) -> Self {
        Self {
            table: identity.table.into(),
            morph_alias: identity.morph.map(|(_, alias)| alias.to_owned()),
            columns: identity.columns().map(str::to_owned).collect(),
            timestamps: (
                identity.timestamps.0.map(str::to_owned),
                identity.timestamps.1.map(str::to_owned),
            ),
        }
    }
}

impl PivotDefinition {
    fn resolve<M: Model>(&self) -> DatabaseResult<PivotIdentity> {
        M::queueablePivots()
            .into_iter()
            .map(|pivot| pivot.0)
            .find(|identity| Self::from(*identity) == *self)
            .ok_or(DatabaseError::InvalidQuery("unregistered queued pivot"))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum IdentifierKey {
    Scalar(ModelKey),
    Composite(Vec<ModelKey>),
}

impl IdentifierKey {
    fn capture<M: Model>(model: &M) -> DatabaseResult<Self> {
        let key = model.getQueueableId().cloned().unwrap_or(Value::Null);
        if let Some(pivot) = model.state().pivot_identity
            && key == Value::Null
        {
            return pivot
                .columns()
                .map(|column| {
                    model
                        .getAttribute(column)
                        .filter(|value| **value != Value::Null)
                        .ok_or(DatabaseError::InvalidQuery("missing pivot identity"))?
                        .clone()
                        .try_into()
                })
                .collect::<DatabaseResult<_>>()
                .map(Self::Composite);
        }
        key.try_into().map(Self::Scalar)
    }

    fn predicates(
        self,
        conditions: Conditions,
        primary: &str,
        pivot: PivotIdentity,
    ) -> DatabaseResult<Conditions> {
        let conditions = match pivot.morph {
            Some((column, alias)) => conditions.r#where(column, "=", alias),
            None => conditions,
        };
        match self {
            Self::Scalar(ModelKey::Null) => Err(DatabaseError::InvalidQuery("missing pivot key")),
            Self::Scalar(key) => Ok(conditions.r#where(primary, "=", Value::from(key))),
            Self::Composite(keys) => {
                if keys.len() != pivot.columns().count()
                    || keys.iter().any(|key| matches!(key, ModelKey::Null))
                {
                    return Err(DatabaseError::InvalidQuery("invalid queued pivot key"));
                }
                Ok(pivot
                    .columns()
                    .zip(keys)
                    .fold(conditions, |query, (column, key)| {
                        query.r#where(column, "=", Value::from(key))
                    }))
            }
        }
    }

    fn scalar(self) -> DatabaseResult<Value> {
        match self {
            Self::Scalar(key) => Ok(key.into()),
            Self::Composite(_) => Err(DatabaseError::InvalidQuery(
                "queued composite key requires pivot definition",
            )),
        }
    }
}

fn restoration_query<M: Model>(connection: Option<&str>) -> DatabaseResult<Builder<'static, M>> {
    let connection = DB::connection(connection.or(M::metadata().connection))?;
    Ok(M::queryOn(&connection).withoutGlobalScopes().useWritePdo())
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<M: Model> ModelIdentifier<M> {
    pub fn capture(model: &M) -> DatabaseResult<Self> {
        Ok(Self {
            id: IdentifierKey::capture(model)?,
            connection: model.getQueueableConnection().map(str::to_owned),
            pivot: model.state().pivot_identity.map(Into::into),
            relations: model.getQueueableRelations(),
            model: PhantomData,
        })
    }

    #[must_use]
    pub fn withoutRelations(mut self) -> Self {
        self.relations.clear();
        self
    }

    /// Restores a job argument and preserves its missing-model outcome for queue settlement.
    pub async fn restoreForJob(self) -> crate::queue::QueueResult<M> {
        use crate::queue::QueueError;

        self.restore().await.map_err(|source| match source {
            DatabaseError::ModelNotFound(table) => QueueError::MissingModel(table.into()),
            source => QueueError::Execution {
                source: Box::new(source),
            },
        })
    }

    /// Resolves the model using the current application's database configuration.
    pub async fn restore(self) -> DatabaseResult<M> {
        let mut model = if let Some(definition) = self.pivot {
            let pivot = definition.resolve::<M>()?;
            let predicates =
                self.id
                    .predicates(Conditions::default(), M::metadata().primary_key, pivot)?;
            let mut query = restoration_query::<M>(self.connection.as_deref())?.for_pivot(pivot);
            query.whereNested(|_| predicates);
            query
                .first()
                .await?
                .ok_or(DatabaseError::ModelNotFound(pivot.table))?
        } else {
            let key = self.id.scalar()?;
            if key == Value::Null {
                return Err(DatabaseError::ModelNotFound(M::metadata().table));
            }
            restoration_query::<M>(self.connection.as_deref())?
                .findOrFail(key)
                .await?
        };
        let paths: Vec<_> = self.relations.iter().map(String::as_str).collect();
        model.load_missing_paths(&paths).await?;
        Ok(model)
    }
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<M: Model> ModelCollectionIdentifier<M> {
    pub fn capture(models: &Collection<M>) -> DatabaseResult<Self> {
        let connection = models.getQueueableConnection()?.map(str::to_owned);
        let identity = models
            .first()
            .and_then(|model| model.state().pivot_identity);
        if models
            .iter()
            .any(|model| model.state().pivot_identity != identity)
        {
            return Err(DatabaseError::InvalidQuery(
                "queued collection has multiple pivot definitions",
            ));
        }
        Ok(Self {
            ids: models
                .iter()
                .map(IdentifierKey::capture)
                .collect::<DatabaseResult<_>>()?,
            connection,
            pivot: identity.map(Into::into),
            relations: models.getQueueableRelations(),
            model: PhantomData,
        })
    }

    #[must_use]
    pub fn withoutRelations(mut self) -> Self {
        self.relations.clear();
        self
    }

    /// Restores existing models and omits missing rows. Ordinary models follow payload
    /// order, while custom pivots are returned once each in database result order.
    pub async fn restore(self) -> DatabaseResult<Collection<M>> {
        if let Some(definition) = self.pivot {
            let pivot = definition.resolve::<M>()?;
            let mut models = Vec::new();
            let mut seen = HashSet::new();
            let mut ids = self.ids.into_iter();
            loop {
                // Keep each OR query below driver parameter and expression limits.
                let batch: Vec<_> = ids.by_ref().take(200).collect();
                if batch.is_empty() {
                    break;
                }
                let mut predicates = Conditions::default();
                for id in batch {
                    let branch =
                        id.predicates(Conditions::default(), M::metadata().primary_key, pivot)?;
                    predicates = predicates.orWhereNested(|_| branch);
                }
                let mut query =
                    restoration_query::<M>(self.connection.as_deref())?.for_pivot(pivot);
                query.whereNested(|_| predicates);
                for model in query.get_models_with_transaction(None).await? {
                    let columns = M::metadata().primary_key;
                    let key = std::iter::once(columns)
                        .chain(pivot.columns())
                        .map(|column| dictionary_key(model.getAttribute(column)))
                        .collect::<DatabaseResult<Vec<_>>>()?;
                    if seen.insert(key) {
                        models.push(model);
                    }
                }
            }
            return Ok(M::newCollection(models));
        }
        let mut keys = Vec::with_capacity(self.ids.len());
        let mut bindings = indexmap::IndexMap::new();
        for id in self.ids {
            let value = id.scalar()?;
            if let Some(key) = dictionary_key(Some(&value))? {
                bindings.entry(key.clone()).or_insert(value);
                keys.push(key);
            }
        }
        if keys.is_empty() {
            return Ok(M::newCollection(Vec::new()));
        }
        let mut found = HashMap::new();
        let bindings: Vec<_> = bindings.into_values().collect();
        for batch in bindings.chunks(500) {
            let mut query = restoration_query::<M>(self.connection.as_deref())?;
            query.where_in(M::metadata().primary_key, batch.to_vec());
            for model in query.get_models_with_transaction(None).await? {
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
        let mut collection = M::newCollection(models);
        if !collection.is_empty() {
            let paths: Vec<_> = self.relations.iter().map(String::as_str).collect();
            collection.load_missing_paths(&paths).await?;
        }
        Ok(collection)
    }
}
