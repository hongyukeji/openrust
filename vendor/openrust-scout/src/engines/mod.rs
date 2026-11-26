mod collection_engine;
mod null_engine;
use crate::{Builder, ScoutFuture, Searchable};
pub use collection_engine::CollectionEngine;
pub use null_engine::NullEngine;
use openrust::database::Value;
/// Engine results retain the ordered model keys alongside provider-specific data.
#[derive(Clone, Debug, Default)]
pub struct SearchResults {
    pub ids: Vec<Value>,
    pub total: u64,
    pub raw: serde_json::Value,
}
pub trait Engine<M: Searchable>: Send + Sync {
    fn supports_semantic_search(&self) -> bool {
        false
    }
    fn update<'a>(&'a self, models: &'a [M]) -> ScoutFuture<'a, ()>;
    fn delete<'a>(&'a self, models: &'a [M]) -> ScoutFuture<'a, ()>;
    fn search<'a>(&'a self, builder: &'a Builder<M>) -> ScoutFuture<'a, SearchResults>;
    fn paginate<'a>(
        &'a self,
        builder: &'a Builder<M>,
        per_page: u64,
        page: u64,
    ) -> ScoutFuture<'a, SearchResults>;
    fn flush<'a>(&'a self, model: &'a M) -> ScoutFuture<'a, ()>;
    fn create_index<'a>(
        &'a self,
        name: &'a str,
        options: &'a serde_json::Value,
    ) -> ScoutFuture<'a, ()>;
    fn delete_index<'a>(&'a self, name: &'a str) -> ScoutFuture<'a, ()>;
    fn map_ids(&self, results: &SearchResults) -> Vec<Value> {
        results.ids.clone()
    }
    fn map_ids_from(&self, results: &SearchResults, _key: &str) -> Vec<Value> {
        self.map_ids(results)
    }
    fn get_total_count(&self, results: &SearchResults) -> u64 {
        results.total
    }
    fn keys<'a>(&'a self, builder: &'a Builder<M>) -> ScoutFuture<'a, Vec<Value>> {
        Box::pin(async move { Ok(self.map_ids(&self.search(builder).await?)) })
    }
    fn get<'a>(
        &'a self,
        builder: &'a Builder<M>,
    ) -> ScoutFuture<'a, openrust::database::eloquent::Collection<M>> {
        Box::pin(async move {
            let results = builder.apply_after_raw_search_callback(self.search(builder).await?)?;
            Ok(M::new_collection(self.map(builder, &results).await?))
        })
    }
    fn cursor<'a>(
        &'a self,
        builder: &'a Builder<M>,
    ) -> ScoutFuture<'a, futures_util::stream::BoxStream<'static, crate::Result<M>>> {
        Box::pin(async move {
            let results = builder.apply_after_raw_search_callback(self.search(builder).await?)?;
            Ok(self.lazy_map(builder, &results))
        })
    }
    fn lazy_map(
        &self,
        builder: &Builder<M>,
        results: &SearchResults,
    ) -> futures_util::stream::BoxStream<'static, crate::Result<M>> {
        use futures_util::{StreamExt as _, TryStreamExt as _};
        if results.ids.is_empty() {
            return futures_util::stream::empty().boxed();
        }
        match builder
            .model
            .query_scout_models_by_ids(builder, results.ids.clone())
        {
            Ok(query) => query
                .reorder()
                .in_order_of(
                    &builder.model.get_scout_key_name(),
                    crate::searchable::normalized_keys(&builder.model, results.ids.clone()),
                )
                .cursor()
                .map_err(crate::Error::from)
                .boxed(),
            Err(error) => futures_util::stream::once(async { Err(error) }).boxed(),
        }
    }
    fn map<'a>(
        &'a self,
        builder: &'a Builder<M>,
        results: &'a SearchResults,
    ) -> ScoutFuture<'a, Vec<M>> {
        Box::pin(async move {
            if results.ids.is_empty() {
                return Ok(Vec::new());
            }
            let mut models: Vec<_> = builder
                .model
                .get_scout_models_by_ids(builder, results.ids.clone())
                .await?
                .into_iter()
                .collect();
            models.retain(|model| {
                model
                    .get_scout_key()
                    .is_some_and(|id| results.ids.iter().any(|key| same_key(key, &id)))
            });
            models.sort_by_key(|model| {
                results
                    .ids
                    .iter()
                    .position(|id| model.get_scout_key().is_some_and(|key| same_key(id, &key)))
            });
            Ok(models)
        })
    }
}

fn same_key(left: &Value, right: &Value) -> bool {
    let text = |key: &Value| match key {
        Value::Integer(n) => Some(n.to_string()),
        Value::Unsigned(n) => Some(n.to_string()),
        Value::Text(s) => Some(s.clone()),
        _ => None,
    };
    left == right
        || text(left)
            .zip(text(right))
            .is_some_and(|(a, b)| openrust::support::php_loose_string_equal(&a, &b))
}
