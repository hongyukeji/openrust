use openrust::{container::ServiceKey, support::facades::Facade};
pub struct Pulse;
impl Facade for Pulse {
    type Service = crate::Pulse;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<crate::Pulse>()
    }
}

pub(crate) struct Options;
impl Facade for Options {
    type Service = crate::pulse::Options;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<crate::pulse::Options>()
    }
}

impl Pulse {
    pub fn with_css(css: impl IntoIterator<Item = crate::Css>) -> crate::Result<()> {
        Self::manager()?.with_css(css);
        Ok(())
    }
    pub fn css() -> crate::Result<String> {
        Self::manager()?.css()
    }
    pub fn js() -> crate::Result<String> {
        Ok(Self::manager()?.js())
    }

    pub fn register(
        recorders: Vec<std::sync::Arc<dyn crate::recorders::Recorder>>,
    ) -> crate::Result<()> {
        Self::manager()?.register(recorders)?;
        Ok(())
    }
    pub fn recorders() -> crate::Result<Vec<std::sync::Arc<dyn crate::recorders::Recorder>>> {
        Ok(Self::manager()?.recorders())
    }

    pub fn manager() -> crate::Result<std::sync::Arc<crate::Pulse>> {
        Ok(Self::get_facade_root()?)
    }
    pub async fn record(
        r#type: impl Into<String>,
        key: impl Into<String>,
        value: Option<i64>,
        timestamp: Option<i64>,
    ) -> crate::Result<crate::Entry> {
        Ok(Self::manager()?.record(r#type, key, value, timestamp).await)
    }
    pub async fn set(
        r#type: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<String>,
        timestamp: Option<i64>,
    ) -> crate::Result<crate::Value> {
        Ok(Self::manager()?.set(r#type, key, value, timestamp).await)
    }
    pub fn filter(
        callback: impl Fn(&crate::Item) -> crate::Result<bool> + Send + Sync + 'static,
    ) -> crate::Result<()> {
        Self::manager()?.filter(callback);
        Ok(())
    }
    pub fn handle_exceptions_using(
        callback: impl Fn(&crate::Error) + Send + Sync + 'static,
    ) -> crate::Result<()> {
        Self::manager()?.handle_exceptions_using(callback);
        Ok(())
    }
    pub async fn report(class: impl Into<String>, location: Option<String>) -> crate::Result<()> {
        Self::manager()?.report(class, location).await;
        Ok(())
    }
    /// 将原生错误报告给 Pulse，保留具体类型和错误来源。
    pub async fn report_exception(
        exception: impl std::error::Error + Send + Sync + 'static,
    ) -> crate::Result<()> {
        Self::manager()?.report_exception(exception).await;
        Ok(())
    }
    pub fn start_recording() -> crate::Result<()> {
        Self::manager()?.start_recording();
        Ok(())
    }
    pub fn stop_recording() -> crate::Result<()> {
        Self::manager()?.stop_recording();
        Ok(())
    }
    pub fn is_recording() -> crate::Result<bool> {
        Ok(Self::manager()?.is_recording())
    }
    pub fn ignore_routes() -> crate::Result<()> {
        Self::manager()?.ignore_routes();
        Ok(())
    }
    pub fn registers_routes() -> crate::Result<bool> {
        Ok(Self::manager()?.registers_routes())
    }
    pub fn flush() -> crate::Result<()> {
        Self::manager()?.flush();
        Ok(())
    }
    pub fn remember_user(
        user: &dyn openrust::contracts::auth::Authenticatable,
    ) -> crate::Result<()> {
        Self::manager()?.remember_user(user);
        Ok(())
    }
    pub fn resolve_authenticated_user_id() -> crate::Result<Option<String>> {
        Ok(Self::manager()?.resolve_authenticated_user_id())
    }
    pub async fn ingest() -> crate::Result<usize> {
        Ok(Self::manager()?.ingest().await)
    }
    pub async fn digest() -> crate::Result<usize> {
        Self::manager()?.digest().await
    }
    pub async fn ignore<T>(callback: impl std::future::Future<Output = T>) -> crate::Result<T> {
        Ok(Self::manager()?.ignore(callback).await)
    }
    pub async fn lazy(
        callback: impl for<'a> FnOnce(&'a crate::Pulse) -> crate::PulseFuture<'a, ()> + Send + 'static,
    ) -> crate::Result<()> {
        Self::manager()?.lazy(callback).await;
        Ok(())
    }
    pub async fn purge(types: Option<&[String]>) -> crate::Result<()> {
        Self::manager()?.purge(types).await
    }
    pub async fn trim() -> crate::Result<()> {
        Self::manager()?.trim().await
    }
    pub async fn values(
        r#type: &str,
        keys: Option<&[String]>,
    ) -> crate::Result<Vec<openrust::database::Row>> {
        Self::manager()?.values(r#type, keys).await
    }
    pub async fn graph(
        types: &[String],
        aggregate: &str,
        interval: i64,
    ) -> crate::Result<crate::storage::Graph> {
        Self::manager()?.graph(types, aggregate, interval).await
    }
    pub async fn graph_bytes(
        types: &[String],
        aggregate: &str,
        interval: i64,
    ) -> crate::Result<crate::storage::ByteGraph> {
        Self::manager()?
            .graph_bytes(types, aggregate, interval)
            .await
    }
    pub async fn aggregate(
        r#type: &str,
        aggregates: &[String],
        interval: i64,
        order_by: Option<&str>,
        direction: &str,
        limit: u64,
    ) -> crate::Result<Vec<openrust::database::Row>> {
        Self::manager()?
            .aggregate(r#type, aggregates, interval, order_by, direction, limit)
            .await
    }
    pub async fn aggregate_types(
        types: &[String],
        aggregate: &str,
        interval: i64,
        order_by: Option<&str>,
        direction: &str,
        limit: u64,
    ) -> crate::Result<Vec<openrust::database::Row>> {
        Self::manager()?
            .aggregate_types(types, aggregate, interval, order_by, direction, limit)
            .await
    }
    pub async fn aggregate_total(
        types: &[String],
        aggregate: &str,
        interval: i64,
    ) -> crate::Result<indexmap::IndexMap<String, Option<f64>>> {
        Self::manager()?
            .aggregate_total(types, aggregate, interval)
            .await
    }
    pub async fn resolve_users(
        keys: &[String],
    ) -> crate::Result<std::sync::Arc<dyn crate::ResolvesUsers>> {
        Self::manager()?.resolve_users(keys).await
    }
    pub fn users(
        callback: impl Fn(&[String]) -> crate::Result<std::collections::BTreeMap<String, crate::User>>
        + Send
        + Sync
        + 'static,
    ) -> crate::Result<()> {
        Self::manager()?.users(callback);
        Ok(())
    }
    pub fn user(
        callback: impl Fn(&dyn openrust::contracts::auth::Authenticatable) -> crate::User
        + Send
        + Sync
        + 'static,
    ) -> crate::Result<()> {
        Self::manager()?.user(callback)?;
        Ok(())
    }
    pub async fn rescue<T>(
        callback: impl std::future::Future<Output = crate::Result<T>>,
    ) -> crate::Result<Option<T>> {
        Ok(Self::manager()?.rescue(callback).await)
    }
    pub fn default_vendor_cache_keys() -> Vec<String> {
        crate::Pulse::default_vendor_cache_keys()
    }
}
