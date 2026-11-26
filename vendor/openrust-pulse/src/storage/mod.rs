mod database_storage;
mod queries;
pub use database_storage::DatabaseStorage;
pub use queries::{ByteGraph, Graph};

pub trait Storage: Send + Sync {
    fn values<'a>(
        &'a self,
        r#type: &'a str,
        keys: Option<&'a [String]>,
    ) -> crate::PulseFuture<'a, Vec<openrust::database::Row>>;
    /// 按原始字节键查询仪表值。默认实现委托给支持 UTF-8 的文本查询。
    fn values_bytes<'a>(
        &'a self,
        r#type: &'a str,
        keys: Option<&'a [Vec<u8>]>,
    ) -> crate::PulseFuture<'a, Vec<openrust::database::Row>> {
        Box::pin(async move {
            let keys = keys
                .map(|keys| {
                    keys.iter()
                        .map(|key| {
                            std::str::from_utf8(key).map(str::to_owned).map_err(|_| {
                                crate::Error::InvalidConfiguration(
                                    "当前 Pulse 存储仅支持 UTF-8 值键".into(),
                                )
                            })
                        })
                        .collect::<crate::Result<Vec<_>>>()
                })
                .transpose()?;
            self.values(r#type, keys.as_deref()).await
        })
    }
    fn graph<'a>(
        &'a self,
        types: &'a [String],
        aggregate: &'a str,
        interval: i64,
    ) -> crate::PulseFuture<'a, Graph>;
    /// 读取以原始字节键索引的图表。仅支持文本键的存储可沿用默认实现。
    fn graph_bytes<'a>(
        &'a self,
        types: &'a [String],
        aggregate: &'a str,
        interval: i64,
    ) -> crate::PulseFuture<'a, ByteGraph> {
        Box::pin(async move {
            Ok(self
                .graph(types, aggregate, interval)
                .await?
                .into_iter()
                .map(|(key, series)| (key.into_bytes(), series))
                .collect())
        })
    }
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
