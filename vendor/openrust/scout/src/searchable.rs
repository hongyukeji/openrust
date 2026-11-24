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
    fn should_be_searchable(&self) -> bool {
        true
    }
    fn search_index_should_be_updated(&self) -> bool {
        true
    }
    fn was_searchable_before_update(&self) -> bool {
        true
    }
    fn was_searchable_before_delete(&self) -> bool {
        true
    }
    fn searchable_as(&self) -> Result<String> {
        let manager = Self::scout_manager()?;
        Ok(format!(
            "{}{}",
            manager
                .config
                .get("scout.prefix")
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
            self.get_table()
        ))
    }
    fn indexable_as(&self) -> Result<String> {
        self.searchable_as()
    }
    fn searchable_embedding_column(&self) -> &str {
        "embedding"
    }
    fn to_searchable_array(&self) -> Result<serde_json::Map<String, serde_json::Value>> {
        Ok(self.to_array()?)
    }
    fn get_scout_key(&self) -> Option<Value> {
        self.get_key().cloned()
    }
    fn get_scout_key_type(&self) -> openrust::database::eloquent::ModelKeyType {
        self.get_key_type()
    }
    fn get_scout_key_name(&self) -> String {
        self.get_key_name().into()
    }
    fn scout_driver(&self) -> Option<String> {
        None
    }
    fn scout_manager() -> Result<Arc<EngineManager>> {
        use openrust::support::facades::Facade;
        Ok(crate::facades::Scout::get_facade_root()?)
    }
    fn searchable_using(&self) -> Result<Arc<dyn crate::engines::Engine<Self>>> {
        Self::scout_manager()?.engine::<Self>(self.scout_driver().as_deref())
    }
    fn search(query: impl Into<String>) -> Result<Builder<Self>> {
        Ok(Builder::new(
            Self::from_state(Default::default()),
            query,
            Self::scout_manager()?,
        ))
    }
    fn boot_searchable(app: &Application) -> ScoutFuture<'_, ()> {
        Box::pin(async move {
            app.make::<EngineManager>()
                .await?
                .register::<Self>(app, std::any::type_name::<Self>())
                .await
        })
    }
    fn make_searchable_using(models: Vec<Self>) -> ScoutFuture<'static, Vec<Self>> {
        Box::pin(async move { Ok(models) })
    }
    fn make_all_searchable_using(
        query: EloquentBuilder<'static, Self>,
    ) -> Result<EloquentBuilder<'static, Self>> {
        Ok(query)
    }
    fn make_all_searchable(chunk: Option<u64>) -> ScoutFuture<'static, u64> {
        Box::pin(
            async move { crate::importer::import::<Self>(&Self::scout_manager()?, chunk).await },
        )
    }
    fn remove_all_from_search() -> ScoutFuture<'static, ()> {
        Box::pin(async {
            let model = Self::from_state(Default::default());
            model.searchable_using()?.flush(&model).await
        })
    }
    fn searchable(&self) -> ScoutFuture<'_, ()> {
        Box::pin(async move {
            Self::scout_manager()?
                .sync(std::slice::from_ref(self), false, false)
                .await
        })
    }
    fn unsearchable(&self) -> ScoutFuture<'_, ()> {
        Box::pin(async move {
            Self::scout_manager()?
                .sync(std::slice::from_ref(self), true, false)
                .await
        })
    }
    fn searchable_sync(&self) -> ScoutFuture<'_, ()> {
        Box::pin(async move {
            Self::scout_manager()?
                .sync(std::slice::from_ref(self), false, true)
                .await
        })
    }
    fn unsearchable_sync(&self) -> ScoutFuture<'_, ()> {
        Box::pin(async move {
            Self::scout_manager()?
                .sync(std::slice::from_ref(self), true, true)
                .await
        })
    }
    fn sync_with_search_using(&self, manager: &EngineManager) -> Option<String> {
        manager
            .config
            .get("scout.queue.connection")
            .or_else(|| manager.config.get("queue.default"))
            .and_then(|v| v.as_str().map(str::to_owned))
    }
    fn sync_with_search_using_queue(&self, manager: &EngineManager) -> Option<String> {
        manager
            .config
            .get("scout.queue.queue")
            .and_then(|v| v.as_str().map(str::to_owned))
    }
    fn query_scout_models_by_ids(
        &self,
        builder: &Builder<Self>,
        ids: Vec<Value>,
    ) -> Result<EloquentBuilder<'static, Self>> {
        let mut query = Self::query_on(&self.get_connection()?);
        if self.get_deleted_at_column().is_some() {
            query = query.with_trashed();
        }
        if let Some(callback) = &builder.query_callback {
            query = callback(query)?;
        }
        let ids = normalized_keys(self, ids);
        query.where_in(&self.get_scout_key_name(), ids);
        Ok(query)
    }
    fn get_scout_models_by_ids<'a>(
        &'a self,
        builder: &'a Builder<Self>,
        ids: Vec<Value>,
    ) -> ScoutFuture<'a, openrust::database::eloquent::Collection<Self>> {
        Box::pin(async move { Ok(self.query_scout_models_by_ids(builder, ids)?.get().await?) })
    }
    fn disable_search_syncing() -> Result<()> {
        Self::scout_manager()?.disable_syncing_for::<Self>();
        Ok(())
    }
    fn enable_search_syncing() -> Result<()> {
        Self::scout_manager()?.enable_syncing_for::<Self>();
        Ok(())
    }
    fn without_syncing_to_search<F: std::future::Future + Send>(
        future: F,
    ) -> impl std::future::Future<Output = Result<F::Output>> + Send {
        async move {
            let manager = Self::scout_manager()?;
            Ok(crate::model_observer::without_syncing::<Self, F>(&manager, future).await)
        }
    }
}
pub(crate) fn normalized_keys<M: Searchable>(model: &M, keys: Vec<Value>) -> Vec<Value> {
    if model.get_scout_key_type() == openrust::database::eloquent::ModelKeyType::Integer {
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
