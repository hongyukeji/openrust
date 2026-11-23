use crate::{Builder, EngineManager, Result, ScoutFuture};
use openrust::{
    database::{
        Value,
        eloquent::{Builder as EloquentBuilder, Model},
    },
    foundation::Application,
};
use std::sync::Arc;
pub trait Searchable: Model + Clone {
    fn shouldBeSearchable(&self) -> bool {
        true
    }
    fn searchIndexShouldBeUpdated(&self) -> bool {
        true
    }
    fn wasSearchableBeforeUpdate(&self) -> bool {
        true
    }
    fn wasSearchableBeforeDelete(&self) -> bool {
        true
    }
    fn searchableAs(&self) -> Result<String> {
        let manager = Self::scoutManager()?;
        Ok(format!(
            "{}{}",
            manager
                .config
                .get("scout.prefix")
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
            self.getTable()
        ))
    }
    fn indexableAs(&self) -> Result<String> {
        self.searchableAs()
    }
    fn searchableEmbeddingColumn(&self) -> &str {
        "embedding"
    }
    fn toSearchableArray(&self) -> Result<serde_json::Map<String, serde_json::Value>> {
        Ok(self.toArray()?)
    }
    fn getScoutKey(&self) -> Option<Value> {
        self.getKey().cloned()
    }
    fn getScoutKeyType(&self) -> openrust::database::eloquent::ModelKeyType {
        self.getKeyType()
    }
    fn getScoutKeyName(&self) -> String {
        self.getKeyName().into()
    }
    fn scoutDriver(&self) -> Option<String> {
        None
    }
    fn scoutManager() -> Result<Arc<EngineManager>> {
        use openrust::support::facades::Facade;
        Ok(crate::facades::Scout::get_facade_root()?)
    }
    fn searchableUsing(&self) -> Result<Arc<dyn crate::engines::Engine<Self>>> {
        Self::scoutManager()?.engine::<Self>(self.scoutDriver().as_deref())
    }
    fn search(query: impl Into<String>) -> Result<Builder<Self>> {
        Ok(Builder::new(
            Self::from_state(Default::default()),
            query,
            Self::scoutManager()?,
        ))
    }
    fn bootSearchable(app: &Application) -> ScoutFuture<'_, ()> {
        Box::pin(async move {
            app.make::<EngineManager>()
                .await?
                .register::<Self>(app, std::any::type_name::<Self>())
                .await
        })
    }
    fn makeSearchableUsing(models: Vec<Self>) -> ScoutFuture<'static, Vec<Self>> {
        Box::pin(async move { Ok(models) })
    }
    fn makeAllSearchableUsing(
        query: EloquentBuilder<'static, Self>,
    ) -> Result<EloquentBuilder<'static, Self>> {
        Ok(query)
    }
    fn makeAllSearchable(chunk: Option<u64>) -> ScoutFuture<'static, u64> {
        Box::pin(async move { import::<Self>(&Self::scoutManager()?, chunk).await })
    }
    fn removeAllFromSearch() -> ScoutFuture<'static, ()> {
        Box::pin(async {
            let model = Self::from_state(Default::default());
            model.searchableUsing()?.flush(&model).await
        })
    }
    fn searchable(&self) -> ScoutFuture<'_, ()> {
        Box::pin(async move {
            Self::scoutManager()?
                .sync(std::slice::from_ref(self), false, false)
                .await
        })
    }
    fn unsearchable(&self) -> ScoutFuture<'_, ()> {
        Box::pin(async move {
            Self::scoutManager()?
                .sync(std::slice::from_ref(self), true, false)
                .await
        })
    }
    fn searchableSync(&self) -> ScoutFuture<'_, ()> {
        Box::pin(async move {
            Self::scoutManager()?
                .sync(std::slice::from_ref(self), false, true)
                .await
        })
    }
    fn unsearchableSync(&self) -> ScoutFuture<'_, ()> {
        Box::pin(async move {
            Self::scoutManager()?
                .sync(std::slice::from_ref(self), true, true)
                .await
        })
    }
    fn syncWithSearchUsing(&self, manager: &EngineManager) -> Option<String> {
        manager
            .config
            .get("scout.queue.connection")
            .or_else(|| manager.config.get("queue.default"))
            .and_then(|v| v.as_str().map(str::to_owned))
    }
    fn syncWithSearchUsingQueue(&self, manager: &EngineManager) -> Option<String> {
        manager
            .config
            .get("scout.queue.queue")
            .and_then(|v| v.as_str().map(str::to_owned))
    }
    fn queryScoutModelsByIds(
        &self,
        builder: &Builder<Self>,
        ids: Vec<Value>,
    ) -> Result<EloquentBuilder<'static, Self>> {
        let mut query = Self::queryOn(&self.getConnection()?);
        if self.getDeletedAtColumn().is_some() {
            query = query.withTrashed();
        }
        if let Some(callback) = &builder.queryCallback {
            query = callback(query)?;
        }
        let ids = normalizedKeys(self, ids);
        query.whereIn(&self.getScoutKeyName(), ids);
        Ok(query)
    }
    fn getScoutModelsByIds<'a>(
        &'a self,
        builder: &'a Builder<Self>,
        ids: Vec<Value>,
    ) -> ScoutFuture<'a, openrust::database::eloquent::Collection<Self>> {
        Box::pin(async move { Ok(self.queryScoutModelsByIds(builder, ids)?.get().await?) })
    }
    fn disableSearchSyncing() -> Result<()> {
        Self::scoutManager()?.disableSyncingFor::<Self>();
        Ok(())
    }
    fn enableSearchSyncing() -> Result<()> {
        Self::scoutManager()?.enableSyncingFor::<Self>();
        Ok(())
    }
    fn withoutSyncingToSearch<F: std::future::Future + Send>(
        future: F,
    ) -> impl std::future::Future<Output = Result<F::Output>> + Send {
        async move {
            let manager = Self::scoutManager()?;
            Ok(crate::model_observer::withoutSyncing::<Self, F>(&manager, future).await)
        }
    }
}
pub(crate) async fn import<M: Searchable>(
    manager: &Arc<EngineManager>,
    chunk: Option<u64>,
) -> Result<u64> {
    let chunk = chunk
        .or_else(|| {
            manager
                .config
                .get("scout.chunk.searchable")
                .and_then(|v| v.as_u64())
        })
        .unwrap_or(500);
    if chunk == 0 {
        return Err(crate::Error::InvalidConfiguration(
            "Searchable chunk size must be positive".into(),
        ));
    }
    let model = M::from_state(Default::default());
    let mut query = M::makeAllSearchableUsing(M::query()?)?;
    if manager.boolean("soft_delete") {
        query = query.withTrashed();
    }
    let count = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let observed = count.clone();
    let manager = manager.clone();
    query
        .chunkById(
            chunk,
            move |models, _| {
                let count = observed.clone();
                let manager = manager.clone();
                async move {
                    let models: Vec<_> = models
                        .into_iter()
                        .filter(|m| m.shouldBeSearchable())
                        .collect();
                    manager
                        .sync(&models, false, false)
                        .await
                        .map_err(crate::databaseError)?;
                    count.fetch_add(models.len() as u64, std::sync::atomic::Ordering::Relaxed);
                    Ok(true)
                }
            },
            Some(&model.getScoutKeyName()),
            None,
        )
        .await?;
    Ok(count.load(std::sync::atomic::Ordering::Relaxed))
}

pub(crate) fn normalizedKeys<M: Searchable>(model: &M, keys: Vec<Value>) -> Vec<Value> {
    if model.getScoutKeyType() == openrust::database::eloquent::ModelKeyType::Integer {
        keys.into_iter()
            .map(|key| match key {
                Value::Text(text) => Value::Integer(openrust::support::php_integer_cast(
                    &serde_json::Value::String(text),
                )),
                key => key,
            })
            .collect()
    } else {
        keys
    }
}
