use crate::{EngineManager, Error, Result, Searchable};
use openrust::{
    contracts::queue::JobHandler,
    database::Value,
    queue::{JobContext, Payload, QueueFuture, QueueResult},
};
use std::{marker::PhantomData, sync::Arc};
pub(crate) fn jobName<M: Searchable>(delete: bool) -> String {
    format!(
        "openrust_scout::jobs::{}<{}>",
        if delete {
            "RemoveFromSearch"
        } else {
            "MakeSearchable"
        },
        std::any::type_name::<M>()
    )
}
pub(crate) fn keyToJson(key: Value) -> Result<serde_json::Value> {
    match key {
        Value::Integer(n) => Ok(n.into()),
        Value::Unsigned(n) => Ok(n.into()),
        Value::Text(s) => Ok(s.into()),
        _ => Err(Error::InvalidConfiguration(
            "Scout keys must be strings or integers".into(),
        )),
    }
}
fn keyFromJson(value: &serde_json::Value) -> Result<Value> {
    match value {
        serde_json::Value::String(s) => Ok(s.clone().into()),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(Value::Integer)
            .or_else(|| n.as_u64().map(Value::Unsigned))
            .ok_or_else(|| {
                Error::InvalidConfiguration("Scout keys must be strings or integers".into())
            }),
        _ => Err(Error::InvalidConfiguration(
            "Scout keys must be strings or integers".into(),
        )),
    }
}
pub struct MakeSearchable<M: Searchable> {
    manager: Arc<EngineManager>,
    model: PhantomData<M>,
}
pub struct RemoveFromSearch<M: Searchable> {
    manager: Arc<EngineManager>,
    model: PhantomData<M>,
}
impl<M: Searchable> MakeSearchable<M> {
    pub fn new(manager: Arc<EngineManager>) -> Self {
        Self {
            manager,
            model: PhantomData,
        }
    }
}
impl<M: Searchable> RemoveFromSearch<M> {
    pub fn new(manager: Arc<EngineManager>) -> Self {
        Self {
            manager,
            model: PhantomData,
        }
    }
}
async fn restore<M: Searchable>(
    payload: &Payload,
    delete: bool,
    manager: &EngineManager,
) -> Result<Vec<M>> {
    use openrust::support::facades::DB;
    let keys = payload.data()["keys"]
        .as_array()
        .ok_or_else(|| Error::InvalidConfiguration("Scout job keys must be an array".into()))?
        .iter()
        .map(keyFromJson)
        .collect::<Result<Vec<_>>>()?;
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let connection = DB::connection(payload.data()["connection"].as_str())?;
    let model = M::from_state(Default::default());
    if delete {
        return keys
            .into_iter()
            .map(|key| {
                let mut tombstone = M::newInstanceOn(&connection, Default::default())?;
                tombstone.forceFill([(model.getScoutKeyName(), key)].into())?;
                Ok(tombstone)
            })
            .collect();
    }
    let mut query = M::queryOn(&connection).withoutGlobalScopes();
    if manager.boolean("soft_delete") {
        query = query.withTrashed();
    }
    query.whereIn(&model.getScoutKeyName(), keys);
    Ok(query.get().await?.into_iter().collect())
}
fn failed(e: Error) -> openrust::queue::QueueError {
    openrust::queue::QueueError::Execution {
        source: Box::new(e),
    }
}
impl<M: Searchable> JobHandler for MakeSearchable<M> {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let models = restore::<M>(payload, false, &self.manager)
                .await
                .map_err(failed)?;
            self.manager
                .sync(&models, false, true)
                .await
                .map_err(failed)
        })
    }
}
impl<M: Searchable> JobHandler for RemoveFromSearch<M> {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let models = restore::<M>(payload, true, &self.manager)
                .await
                .map_err(failed)?;
            self.manager.sync(&models, true, true).await.map_err(failed)
        })
    }
}
