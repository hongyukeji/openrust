use super::{Repository, encode, processes::string_list, redis_error, text};
use crate::{Error, Result};
use openrust::redis::Value;
use serde_json::{Value as Json, json};
#[derive(Clone)]
pub struct MetricsRepository(pub(super) Repository);
impl Repository {
    pub fn metrics_repository(&self) -> MetricsRepository {
        MetricsRepository(self.clone())
    }
}
impl MetricsRepository {
    pub fn new(repository: Repository) -> Self {
        Self(repository)
    }
    fn key(&self, kind: &str, name: &str) -> String {
        self.0.key(&format!("metric:{kind}:{}", encode(name)))
    }
    async fn measured(&self, kind: &str) -> Result<Vec<String>> {
        string_list(
            self.0
                .command("SMEMBERS", &[self.0.key(&format!("measured:{kind}"))])
                .await?,
        )
    }
    pub async fn measured_jobs(&self) -> Result<Vec<String>> {
        self.measured("job").await
    }
    pub async fn measured_queues(&self) -> Result<Vec<String>> {
        self.measured("queue").await
    }
    async fn increment(&self, kind: &str, name: &str, runtime: f64) -> Result<()> {
        if name.is_empty() || name.len() > 1024 || !runtime.is_finite() || runtime < 0. {
            return Err(Error::InvalidRecord);
        }
        self.0.eval("redis.call('SADD',KEYS[1],ARGV[1]);redis.call('HINCRBY',KEYS[2],'throughput',1);redis.call('HINCRBYFLOAT',KEYS[2],'total_runtime',ARGV[2]);return 1",vec![self.0.key(&format!("measured:{kind}")),self.key(kind,name)],vec![name.into(),runtime.to_string()]).await?;
        Ok(())
    }
    pub async fn increment_job(&self, job: &str, runtime: f64) -> Result<()> {
        self.increment("job", job, runtime).await
    }
    pub async fn increment_queue(&self, queue: &str, runtime: f64) -> Result<()> {
        self.increment("queue", queue, runtime).await
    }
    async fn throughput_for(&self, kind: &str, name: &str) -> Result<u64> {
        let value = self
            .0
            .command("HGET", &[self.key(kind, name), "throughput".into()])
            .await?;
        if value == Value::Nil {
            Ok(0)
        } else {
            text(&value)?.parse().map_err(|_| Error::Redis)
        }
    }
    async fn runtime_for(&self, kind: &str, name: &str) -> Result<f64> {
        let value=self.0.eval("local n=tonumber(redis.call('HGET',KEYS[1],'throughput') or 0);if n==0 then return '0' end;return tostring(tonumber(redis.call('HGET',KEYS[1],'total_runtime') or 0)/n)",vec![self.key(kind,name)],vec![]).await?;
        text(&value)?.parse().map_err(|_| Error::Redis)
    }
    pub async fn throughput_for_job(&self, job: &str) -> Result<u64> {
        self.throughput_for("job", job).await
    }
    pub async fn throughput_for_queue(&self, queue: &str) -> Result<u64> {
        self.throughput_for("queue", queue).await
    }
    pub async fn runtime_for_job(&self, job: &str) -> Result<f64> {
        self.runtime_for("job", job).await
    }
    pub async fn runtime_for_queue(&self, queue: &str) -> Result<f64> {
        self.runtime_for("queue", queue).await
    }
    pub async fn throughput(&self) -> Result<u64> {
        let mut count = 0;
        for queue in self.measured_queues().await? {
            count += self.throughput_for_queue(&queue).await?;
        }
        Ok(count)
    }
    pub async fn jobs_processed_per_minute(&self) -> Result<u64> {
        let now = self.0.now().await?;
        let last=self.0.eval("local last=redis.call('GET',KEYS[1]);if not last then redis.call('SET',KEYS[1],ARGV[1]);return ARGV[1] end;return last",vec![self.0.key("last_snapshot_at")],vec![now.to_string()]).await?;
        let last = text(&last)?.parse::<u64>().map_err(|_| Error::Redis)?;
        let minutes = (now.saturating_sub(last) as f64 / 60000.).max(1.);
        Ok((self.throughput().await? as f64 / minutes).round() as u64)
    }
    async fn snapshots(&self, kind: &str, name: &str) -> Result<Vec<Json>> {
        string_list(
            self.0
                .command(
                    "ZRANGE",
                    &[
                        format!("{}:snapshots", self.key(kind, name)),
                        "0".into(),
                        "-1".into(),
                    ],
                )
                .await?,
        )?
        .iter()
        .map(|v| serde_json::from_str(v).map_err(|_| Error::Redis))
        .collect()
    }
    pub async fn snapshots_for_job(&self, job: &str) -> Result<Vec<Json>> {
        self.snapshots("job", job).await
    }
    pub async fn snapshots_for_queue(&self, queue: &str) -> Result<Vec<Json>> {
        self.snapshots("queue", queue).await
    }
    pub async fn acquire_snapshot_lock(&self) -> Result<bool> {
        Ok(self
            .0
            .command(
                "SET",
                &[
                    self.0.key("metrics:snapshot"),
                    "1".into(),
                    "NX".into(),
                    "EX".into(),
                    self.0.snapshot_lock.saturating_sub(30).max(1).to_string(),
                ],
            )
            .await?
            != Value::Nil)
    }
    pub async fn snapshot(&self) -> Result<()> {
        let now = self.0.now().await?;
        let workload = if openrust::queue::QueueFacade::manager().is_ok() {
            self.0.workload().get().await?
        } else {
            Vec::new()
        };
        for kind in ["job", "queue"] {
            for name in self.measured(kind).await? {
                let key = self.key(kind, &name);
                self.0.eval("local n=tonumber(redis.call('HGET',KEYS[1],'throughput') or 0);local total=tonumber(redis.call('HGET',KEYS[1],'total_runtime') or 0);redis.call('DEL',KEYS[1]);local data={throughput=n,runtime=0,time=tonumber(ARGV[1])/1000};if n>0 then data.runtime=total/n end;if ARGV[2]=='queue' then data.wait=tonumber(ARGV[4]) end;redis.call('ZADD',KEYS[2],ARGV[1],cjson.encode(data));redis.call('ZREMRANGEBYRANK',KEYS[2],0,-tonumber(ARGV[3])-1);return 1",vec![key.clone(),format!("{key}:snapshots")],vec![now.to_string(),kind.into(),if kind=="job"{self.0.snapshots.0}else{self.0.snapshots.1}.to_string(),workload.iter().find(|queue|format!("{}:{}",queue["connection"].as_str().unwrap_or_default(),queue["name"].as_str().unwrap_or_default())==name).and_then(|queue|queue["wait"].as_f64()).unwrap_or(0.).to_string()]).await?;
            }
        }
        self.0
            .command("SET", &[self.0.key("last_snapshot_at"), now.to_string()])
            .await?;
        Ok(())
    }
    async fn maximum(&self, field: &str) -> Result<Option<String>> {
        let mut maximum = 0.;
        let mut result = None;
        for queue in self.measured_queues().await? {
            if let Some(snapshot) = self.snapshots_for_queue(&queue).await?.last() {
                let value = snapshot.get(field).and_then(Json::as_f64).unwrap_or(0.);
                if value > maximum {
                    maximum = value;
                    result = Some(queue);
                }
            }
        }
        Ok(result)
    }
    pub async fn queue_with_maximum_runtime(&self) -> Result<Option<String>> {
        self.maximum("runtime").await
    }
    pub async fn queue_with_maximum_throughput(&self) -> Result<Option<String>> {
        self.maximum("throughput").await
    }
    pub async fn acquire_wait_time_monitor_lock(&self) -> Result<bool> {
        Ok(self
            .0
            .command(
                "SET",
                &[
                    self.0.key("wait_time_monitor"),
                    "1".into(),
                    "NX".into(),
                    "EX".into(),
                    "60".into(),
                ],
            )
            .await?
            != Value::Nil)
    }
    pub async fn forget(&self, key: &str) -> Result<()> {
        let Some((kind, name)) = key.split_once(':') else {
            return Err(Error::InvalidRecord);
        };
        if !matches!(kind, "job" | "queue") {
            return Err(Error::InvalidRecord);
        }
        let key = self.key(kind, name);
        self.0
            .command("DEL", &[key.clone(), format!("{key}:snapshots")])
            .await?;
        self.0
            .command(
                "SREM",
                &[self.0.key(&format!("measured:{kind}")), name.into()],
            )
            .await?;
        Ok(())
    }
    pub async fn clear(&self) -> Result<()> {
        for kind in ["job", "queue"] {
            for name in self.measured(kind).await? {
                self.forget(&format!("{kind}:{name}")).await?;
            }
        }
        // The compatibility dashboard also reads rolling attempt buckets.
        // Clear those in the same metadata namespace without scanning job data.
        let prefix = self.0.key("metrics:");
        let mut pattern = String::with_capacity(prefix.len() + 1);
        for character in prefix.chars() {
            if matches!(character, '\\' | '[' | ']' | '?' | '*') {
                pattern.push('\\');
            }
            pattern.push(character);
        }
        pattern.push('*');
        let mut cursor = 0;
        loop {
            let (next, keys) = self.scan_rolling_metrics(cursor, &pattern).await?;
            if !keys.is_empty() {
                self.0.command("DEL", &keys).await?;
            }
            cursor = next;
            if cursor == 0 {
                break;
            }
        }
        self.0
            .command(
                "DEL",
                &[
                    self.0.key("last_snapshot_at"),
                    self.0.key("wait_time_monitor"),
                    self.0.key("metrics:snapshot"),
                ],
            )
            .await?;
        Ok(())
    }

    async fn scan_rolling_metrics(&self, cursor: u64, pattern: &str) -> Result<(u64, Vec<String>)> {
        if self.0.connection.is_cluster() {
            let (next, keys) = self
                .0
                .connection
                .scan_slot_keys(&self.0.key("metrics:snapshot"), pattern, cursor, 1000)
                .await
                .map_err(redis_error)?;
            let keys = keys
                .into_iter()
                .map(|key| String::from_utf8(key).map_err(|_| Error::InvalidRecord))
                .collect::<Result<_>>()?;
            return Ok((next, keys));
        }
        let Value::Array(page) = self
            .0
            .command(
                "SCAN",
                &[
                    cursor.to_string(),
                    "MATCH".into(),
                    pattern.into(),
                    "COUNT".into(),
                    "1000".into(),
                ],
            )
            .await?
        else {
            return Err(Error::Redis);
        };
        let [next, keys] = page.as_slice() else {
            return Err(Error::Redis);
        };
        let next = text(next)?.parse().map_err(|_| Error::Redis)?;
        Ok((next, string_list(keys.clone())?))
    }
    pub async fn summary(&self) -> Result<Json> {
        Ok(
            json!({"jobsPerMinute":self.jobs_processed_per_minute().await?,"queueWithMaxRuntime":self.queue_with_maximum_runtime().await?,"queueWithMaxThroughput":self.queue_with_maximum_throughput().await?}),
        )
    }
}
