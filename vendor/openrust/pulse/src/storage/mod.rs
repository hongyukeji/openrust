mod database_storage;
mod queries;
pub use database_storage::DatabaseStorage;
pub use queries::Graph;

pub trait Storage: Send + Sync {
    fn values<'a>(
        &'a self,
        r#type: &'a str,
        keys: Option<&'a [String]>,
    ) -> crate::PulseFuture<'a, Vec<openrust::database::Row>>;
    fn graph<'a>(
        &'a self,
        types: &'a [String],
        aggregate: &'a str,
        interval: i64,
    ) -> crate::PulseFuture<'a, Graph>;
    fn aggregate<'a>(
        &'a self,
        r#type: &'a str,
        aggregates: &'a [String],
        interval: i64,
        order_by: Option<&'a str>,
        direction: &'a str,
        limit: u64,
    ) -> crate::PulseFuture<'a, Vec<openrust::database::Row>>;
    fn aggregate_types<'a>(
        &'a self,
        types: &'a [String],
        aggregate: &'a str,
        interval: i64,
        order_by: Option<&'a str>,
        direction: &'a str,
        limit: u64,
    ) -> crate::PulseFuture<'a, Vec<openrust::database::Row>>;
    fn aggregate_total<'a>(
        &'a self,
        types: &'a [String],
        aggregate: &'a str,
        interval: i64,
    ) -> crate::PulseFuture<'a, indexmap::IndexMap<String, Option<f64>>>;
    fn store<'a>(&'a self, items: &'a [crate::Item]) -> crate::PulseFuture<'a, ()>;
    fn trim(&self) -> crate::PulseFuture<'_, ()>;
    fn purge<'a>(&'a self, types: Option<&'a [String]>) -> crate::PulseFuture<'a, ()>;
}
