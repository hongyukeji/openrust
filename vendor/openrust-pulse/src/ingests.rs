use crate::{Item, PulseFuture, Result, storage::Storage};
use std::sync::Arc;
pub trait Ingest: Send + Sync {
    fn ingest<'a>(&'a self, items: &'a [Item]) -> PulseFuture<'a, ()>;
    fn trim(&self) -> PulseFuture<'_, ()>;
    fn digest<'a>(&'a self, storage: &'a dyn Storage) -> PulseFuture<'a, usize>;
}
pub struct StorageIngest(pub Arc<dyn Storage>);
impl Ingest for StorageIngest {
    fn ingest<'a>(&'a self, items: &'a [Item]) -> PulseFuture<'a, ()> {
        self.0.store(items)
    }
    fn trim(&self) -> PulseFuture<'_, ()> {
        self.0.trim()
    }
    fn digest<'a>(&'a self, _: &'a dyn Storage) -> PulseFuture<'a, usize> {
        Box::pin(async { Ok(0) })
    }
}
pub struct NullIngest;
impl Ingest for NullIngest {
    fn ingest<'a>(&'a self, _: &'a [Item]) -> PulseFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn trim(&self) -> PulseFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
    fn digest<'a>(&'a self, _: &'a dyn Storage) -> PulseFuture<'a, usize> {
        Box::pin(async { Ok(0) })
    }
}
pub struct RedisIngest {
    connection: Arc<openrust::redis::connections::Connection>,
    stream: String,
    chunk: usize,
    keep: Retention,
}
pub enum Retention {
    Seconds(i64),
    Entries(u64),
}
impl RedisIngest {
    pub fn new(connection: Arc<openrust::redis::connections::Connection>) -> Self {
        let stream = connection.key("openrust:pulse:ingest");
        Self {
            connection,
            stream,
            chunk: 1000,
            keep: Retention::Seconds(604800),
        }
    }
    pub fn with_chunk(mut self, chunk: usize) -> Result<Self> {
        if chunk == 0 {
            return Err(crate::Error::InvalidConfiguration(
                "ingest.redis.chunk must be positive".into(),
            ));
        }
        self.chunk = chunk;
        Ok(self)
    }
    pub fn with_keep(mut self, keep: Retention) -> Self {
        self.keep = keep;
        self
    }
}
impl Ingest for RedisIngest {
    fn ingest<'a>(&'a self, items: &'a [Item]) -> PulseFuture<'a, ()> {
        Box::pin(async move {
            let mut pipeline = openrust::redis::Pipeline::new();
            for item in items {
                pipeline
                    .cmd("XADD")
                    .arg(&self.stream)
                    .arg("*")
                    .arg("data")
                    .arg(serde_json::to_vec(item)?)
                    .ignore();
            }
            if !items.is_empty() {
                self.connection.pipeline(&pipeline).await?;
            }
            Ok(())
        })
    }
    fn trim(&self) -> PulseFuture<'_, ()> {
        Box::pin(async move {
            let (strategy, keep) = match self.keep {
                Retention::Entries(n) => (b"MAXLEN".as_slice(), n.to_string()),
                Retention::Seconds(s) => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as i64;
                    (b"MINID".as_slice(), (now - s * 1000).to_string())
                }
            };
            self.connection
                .command(
                    "XTRIM",
                    &[self.stream.as_bytes(), strategy, b"~", keep.as_bytes()],
                )
                .await?;
            Ok(())
        })
    }
    fn digest<'a>(&'a self, storage: &'a dyn Storage) -> PulseFuture<'a, usize> {
        Box::pin(async move {
            use openrust::redis::Value;
            let mut total = 0;
            loop {
                let chunk = self.chunk.to_string();
                let payload = self
                    .connection
                    .command(
                        "XRANGE",
                        &[
                            self.stream.as_bytes(),
                            b"-",
                            b"+",
                            b"COUNT",
                            chunk.as_bytes(),
                        ],
                    )
                    .await?;
                let Value::Array(rows) = payload else {
                    return Err(crate::Error::StateUnavailable);
                };
                if rows.is_empty() {
                    return Ok(total);
                }
                let mut items = Vec::new();
                let mut ids = Vec::new();
                for row in rows {
                    let Value::Array(row) = row else {
                        return Err(crate::Error::StateUnavailable);
                    };
                    let [Value::BulkString(id), Value::Array(fields)] = row.as_slice() else {
                        return Err(crate::Error::StateUnavailable);
                    };
                    let data = fields
                        .chunks_exact(2)
                        .find_map(|f| match f {
                            [Value::BulkString(key), Value::BulkString(value)]
                                if key == b"data" =>
                            {
                                Some(value)
                            }
                            _ => None,
                        })
                        .ok_or(crate::Error::StateUnavailable)?;
                    items.push(serde_json::from_slice::<Item>(data)?);
                    ids.push(id.clone());
                }
                storage.store(&items).await?;
                let mut args = vec![self.stream.as_bytes()];
                args.extend(ids.iter().map(Vec::as_slice));
                self.connection.command("XDEL", &args).await?;
                total += items.len();
                if items.len() < self.chunk {
                    return Ok(total);
                }
            }
        })
    }
}
