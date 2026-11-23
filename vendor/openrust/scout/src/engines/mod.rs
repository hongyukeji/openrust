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
    fn supportsSemanticSearch(&self) -> bool {
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
    fn createIndex<'a>(
        &'a self,
        name: &'a str,
        options: &'a serde_json::Value,
    ) -> ScoutFuture<'a, ()>;
    fn deleteIndex<'a>(&'a self, name: &'a str) -> ScoutFuture<'a, ()>;
    fn mapIds(&self, results: &SearchResults) -> Vec<Value> {
        results.ids.clone()
    }
    fn mapIdsFrom(&self, results: &SearchResults, _key: &str) -> Vec<Value> {
        self.mapIds(results)
    }
    fn getTotalCount(&self, results: &SearchResults) -> u64 {
        results.total
    }
    fn keys<'a>(&'a self, builder: &'a Builder<M>) -> ScoutFuture<'a, Vec<Value>> {
        Box::pin(async move { Ok(self.mapIds(&self.search(builder).await?)) })
    }
    fn get<'a>(
        &'a self,
        builder: &'a Builder<M>,
    ) -> ScoutFuture<'a, openrust::database::eloquent::Collection<M>> {
        Box::pin(async move {
            let results = builder.applyAfterRawSearchCallback(self.search(builder).await?)?;
            Ok(M::new_collection(self.map(builder, &results).await?))
        })
    }
    fn cursor<'a>(
        &'a self,
        builder: &'a Builder<M>,
    ) -> ScoutFuture<'a, futures_util::stream::BoxStream<'static, crate::Result<M>>> {
        Box::pin(async move {
            let results = builder.applyAfterRawSearchCallback(self.search(builder).await?)?;
            Ok(self.lazyMap(builder, &results))
        })
    }
    fn lazyMap(
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
            .queryScoutModelsByIds(builder, results.ids.clone())
        {
            Ok(query) => query
                .reorder()
                .in_order_of(
                    &builder.model.getScoutKeyName(),
                    crate::searchable::normalizedKeys(&builder.model, results.ids.clone()),
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
                .getScoutModelsByIds(builder, results.ids.clone())
                .await?
                .into_iter()
                .collect();
            models.retain(|model| {
                model
                    .getScoutKey()
                    .is_some_and(|id| results.ids.iter().any(|key| sameKey(key, &id)))
            });
            models.sort_by_key(|model| {
                results
                    .ids
                    .iter()
                    .position(|id| model.getScoutKey().is_some_and(|key| sameKey(id, &key)))
            });
            Ok(models)
        })
    }
}

fn sameKey(left: &Value, right: &Value) -> bool {
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
