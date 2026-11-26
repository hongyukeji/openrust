use crate::{Error, Result, label};
use openrust::redis::{Value, connections::Connection};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::time::{Duration, Instant};

pub(crate) mod command_queue;
mod jobs;
mod metrics;
mod processes;
mod tags;
mod workload;
pub use command_queue::{
    CommandQueueTarget, HorizonCommandQueue, QueuedCommand, RedisHorizonCommandQueue,
};
pub use jobs::{JobDetails, JobRepository};
pub use metrics::MetricsRepository;
pub(crate) use processes::{MasterLease, MasterOwnership};
pub use processes::{MasterSupervisorRepository, ProcessRepository, SupervisorRepository};
pub use tags::TagRepository;
pub use workload::WorkloadRepository;

#[derive(Default)]
pub(crate) struct MetadataRecovery(Option<Instant>);

impl MetadataRecovery {
    pub(crate) fn recovered(&mut self) {
        self.0 = None;
    }

    pub(crate) async fn retry(&mut self, started: Instant, error: Error) -> Result<()> {
        if !matches!(error, Error::RedisUnavailable(_)) {
            return Err(error);
        }
        // 最长等待一个所有权租期；恢复后各入口仍须在 Redis 验证令牌。
        let deadline = self.0.get_or_insert(started + Duration::from_secs(15));
        if Instant::now() >= *deadline {
            return Err(error);
        }
        if openrust::support::report(error).await.is_err() {
            let _ = openrust::support::facades::Log::warning(
                "Horizon 元数据 Redis 暂不可用，监控循环将重试。",
                &Default::default(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: String,
    pub name: String,
    pub connection: String,
    pub queue: String,
    pub attempts: u32,
    pub status: String,
    pub duration: f64,
    pub updated_at: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub attempts: u64,
    pub completed: u64,
    pub failed: u64,
    pub released: u64,
    pub retrying: u64,
    pub runtime_ms: f64,
}
impl Metrics {
    pub fn runtime_for_queue(&self) -> f64 {
        if self.completed == 0 {
            0.
        } else {
            self.runtime_ms / self.completed as f64
        }
    }
}
#[derive(Clone)]
pub struct Repository {
    pub(crate) master_names: Arc<crate::master_names::MasterNames>,
    connection: Arc<Connection>,
    prefix: String,
    retention: u32,
    trim: std::collections::BTreeMap<String, u32>,
    snapshots: (u32, u32),
    snapshot_lock: u32,
}
const RECORD: &str = r#"
local old=tonumber(redis.call('HGET',KEYS[1],'attempts') or '-1')
local incoming=tonumber(ARGV[3])
if old>incoming and ARGV[2]~='processing' then return 0 end
if old==incoming and redis.call('HGET',KEYS[1],'finalized')=='1' then
 if ARGV[2]~='processing' then return 0 end
 redis.call('HDEL',KEYS[1],'finalized','status')
end
if old~=incoming then redis.call('HDEL',KEYS[1],'finalized','status') end
local data=cjson.decode(ARGV[1])
local phase=ARGV[2]
if phase=='finalize' then
 local previous=redis.call('HGET',KEYS[1],'status')
 if previous=='failed' then phase=previous
 elseif ARGV[4]=='1' then phase='retrying' else phase='completed' end
 if previous=='released' and ARGV[4]=='0' then phase=previous end
 data.status=phase
 redis.call('HSET',KEYS[1],'finalized','1')
 redis.call('HINCRBY',KEYS[3],'attempts',1)
 redis.call('HINCRBY',KEYS[3],phase,1)
 if phase=='completed' then
 redis.call('HINCRBYFLOAT',KEYS[3],'runtime_ms',ARGV[5])
 for i=4,6,2 do redis.call('SADD',KEYS[i],ARGV[8+(i/2)]);redis.call('HINCRBY',KEYS[i+1],'throughput',1);redis.call('HINCRBYFLOAT',KEYS[i+1],'total_runtime',ARGV[5]) end
 end
 redis.call('EXPIRE',KEYS[3],ARGV[6])
end
redis.call('HSET',KEYS[1],'attempts',ARGV[3],'status',phase,'data',cjson.encode(data))
redis.call('EXPIRE',KEYS[1],ARGV[6])
redis.call('ZADD',KEYS[2],ARGV[7],ARGV[8])
redis.call('ZREMRANGEBYSCORE',KEYS[2],'-inf',ARGV[9])
redis.call('ZREMRANGEBYRANK',KEYS[2],0,-1001)
redis.call('EXPIRE',KEYS[2],ARGV[6])
return phase
"#;
impl Repository {
    pub fn new(connection: Arc<Connection>, prefix: String, retention: u32) -> Result<Self> {
        if prefix.is_empty()
            || prefix.len() > 128
            || !prefix
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_:-.".contains(&c))
            || !(60..=86400).contains(&retention)
        {
            return Err(Error::InvalidConfiguration);
        }
        Ok(Self {
            master_names: Arc::default(),
            connection,
            prefix,
            retention,
            trim: std::collections::BTreeMap::new(),
            snapshots: (24, 24),
            snapshot_lock: 300,
        })
    }
    pub fn with_config(
        mut self,
        config: &dyn openrust::contracts::config::Repository,
    ) -> Result<Self> {
        for (kind, default) in [
            ("recent", 60),
            ("pending", 60),
            ("completed", 60),
            ("recent_failed", 10080),
            ("failed", 10080),
            ("monitored", 10080),
        ] {
            let minutes = config
                .get(&format!("horizon.trim.{kind}"))
                .map_or(Ok(default), |v| {
                    v.as_u64()
                        .and_then(|v| u32::try_from(v).ok())
                        .filter(|v| *v > 0 && *v <= 525600)
                        .ok_or(Error::InvalidConfiguration)
                })?;
            self.trim.insert(kind.into(), minutes * 60);
        }
        let limit = |kind: &str| -> Result<u32> {
            config
                .get(&format!("horizon.metrics.trim_snapshots.{kind}"))
                .map_or(Ok(24), |v| {
                    v.as_u64()
                        .and_then(|v| u32::try_from(v).ok())
                        .filter(|v| *v > 0 && *v <= 10000)
                        .ok_or(Error::InvalidConfiguration)
                })
        };
        self.snapshots = (limit("job")?, limit("queue")?);
        self.snapshot_lock =
            config
                .get("horizon.metrics.snapshot_lock")
                .map_or(Ok(300), |value| {
                    value
                        .as_u64()
                        .and_then(|value| u32::try_from(value).ok())
                        .filter(|value| *value > 30 && *value <= 86400)
                        .ok_or(Error::InvalidConfiguration)
                })?;
        Ok(self)
    }
    fn retention_for(&self, kind: &str) -> u32 {
        let kind = if kind == "silenced" {
            "completed"
        } else {
            kind
        };
        self.trim.get(kind).copied().unwrap_or(self.retention)
    }
    pub(crate) fn key(&self, suffix: &str) -> String {
        self.connection
            .key(&format!("{}{{horizon}}:{suffix}", self.prefix))
    }
    pub(crate) async fn command(&self, command: &str, args: &[String]) -> Result<Value> {
        self.connection
            .command(
                command,
                &args.iter().map(|s| s.as_bytes()).collect::<Vec<_>>(),
            )
            .await
            .map_err(|error| {
                if error.is_temporarily_unavailable() {
                    Error::RedisUnavailable(error)
                } else {
                    Error::Redis
                }
            })
    }
    async fn eval(&self, script: &str, keys: Vec<String>, args: Vec<String>) -> Result<Value> {
        let mut parameters = vec![script.into(), keys.len().to_string()];
        parameters.extend(keys);
        parameters.extend(args);
        self.command("EVAL", &parameters).await
    }
    pub(crate) async fn now(&self) -> Result<u64> {
        let Value::Array(values) = self.command("TIME", &[]).await? else {
            return Err(Error::Redis);
        };
        if values.len() != 2 {
            return Err(Error::Redis);
        }
        let number = |v: &Value| text(v).and_then(|v| v.parse::<u64>().map_err(|_| Error::Redis));
        number(&values[0])?
            .checked_mul(1000)
            .and_then(|v| v.checked_add(number(&values[1]).ok()? / 1000))
            .ok_or(Error::Redis)
    }
    /// Stores the fixed attempt metadata projection; use recordPayload for authorized job details.
    pub async fn record(&self, record: JobRecord, phase: &str, exception: bool) -> Result<()> {
        self.record_accepted(record, phase, exception)
            .await
            .map(|_| ())
    }
    async fn record_accepted(
        &self,
        mut record: JobRecord,
        phase: &str,
        exception: bool,
    ) -> Result<Option<String>> {
        if !label(&record.id)
            || record.name.is_empty()
            || record.name.len() > 1024
            || !label(&record.connection)
            || openrust::queue::RedisQueue::validate_queue_name(&record.queue).is_err()
            || !matches!(phase, "processing" | "failed" | "released" | "finalize")
            || !record.duration.is_finite()
            || record.duration < 0.
        {
            return Err(Error::InvalidRecord);
        }
        let now = self.now().await?;
        record.updated_at = now;
        record.status = phase.into();
        let id = job_member(&record)?;
        let keys = vec![
            self.key(&format!("job:{id}")),
            self.key("recent"),
            self.metric_key(&record.connection, &record.queue, now / 60000),
            self.key("measured:job"),
            self.key(&format!("metric:job:{}", encode(&record.name))),
            self.key("measured:queue"),
            self.key(&format!(
                "metric:queue:{}",
                encode(&format!("{}:{}", record.connection, record.queue))
            )),
        ];
        let args = vec![
            serde_json::to_string(&record).map_err(|_| Error::InvalidRecord)?,
            phase.into(),
            record.attempts.to_string(),
            if exception { "1" } else { "0" }.into(),
            record.duration.to_string(),
            self.retention.to_string(),
            now.to_string(),
            id,
            (now.saturating_sub(u64::from(self.retention) * 1000)).to_string(),
            record.name.clone(),
            format!("{}:{}", record.connection, record.queue),
        ];
        let accepted = self.eval(RECORD, keys, args).await?;
        if accepted == Value::Int(0) {
            Ok(None)
        } else {
            text(&accepted).map(Some)
        }
    }
    fn metric_key(&self, connection: &str, queue: &str, minute: u64) -> String {
        self.key(&format!(
            "metrics:{}:{}:{minute}",
            encode(connection),
            encode(queue)
        ))
    }
    /// Aggregates the latest 60 server-clock minute buckets. Expired buckets count as zero.
    pub async fn metrics(&self, connection: &str, queue: &str) -> Result<Metrics> {
        if !label(connection) || openrust::queue::RedisQueue::validate_queue_name(queue).is_err() {
            return Err(Error::InvalidRecord);
        }
        let minute = self.now().await? / 60000;
        let keys = (0..60)
            .map(|n| self.metric_key(connection, queue, minute.saturating_sub(n)))
            .collect();
        let value=self.eval(r#"
local total={attempts=0,completed=0,failed=0,released=0,retrying=0,runtime_ms=0}
for _,key in ipairs(KEYS) do
 for field,_ in pairs(total) do total[field]=total[field]+tonumber(redis.call('HGET',key,field) or '0') end
end
return cjson.encode(total)
"#,keys,vec![]).await?;
        serde_json::from_str(&text(&value)?).map_err(|_| Error::Redis)
    }
    pub async fn recent(&self, limit: usize) -> Result<Vec<JobRecord>> {
        if !(1..=100).contains(&limit) {
            return Err(Error::InvalidRecord);
        }
        let now = self.now().await?;
        let Value::Array(ids) = self
            .command(
                "ZREVRANGEBYSCORE",
                &[
                    self.key("recent"),
                    "+inf".into(),
                    now.saturating_sub(u64::from(self.retention) * 1000)
                        .to_string(),
                    "LIMIT".into(),
                    "0".into(),
                    limit.to_string(),
                ],
            )
            .await?
        else {
            return Err(Error::Redis);
        };
        let mut records = Vec::new();
        for id in ids {
            let id = text(&id)?;
            // The sorted set is internal state, never permit a stored value to address another prefix.
            if id.len() > 1024 || !id.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err(Error::Redis);
            }
            let value = self
                .command("HGET", &[self.key(&format!("job:{id}")), "data".into()])
                .await?;
            if value != Value::Nil {
                records.push(serde_json::from_str(&text(&value)?).map_err(|_| Error::Redis)?);
            }
        }
        Ok(records)
    }
    pub async fn notification_lock(&self, signature: &str) -> Result<bool> {
        if signature.is_empty() || signature.len() > 1024 {
            return Err(Error::InvalidRecord);
        }
        Ok(self
            .command(
                "SET",
                &[
                    self.key(&format!("notification:{}", encode(signature))),
                    "1".into(),
                    "NX".into(),
                    "EX".into(),
                    "300".into(),
                ],
            )
            .await?
            != Value::Nil)
    }
    pub async fn pause(&self) -> Result<()> {
        self.command("SET", &[self.key("paused"), "1".into()])
            .await?;
        Ok(())
    }
    pub async fn r#continue(&self) -> Result<()> {
        self.command("DEL", &[self.key("paused")]).await?;
        Ok(())
    }
    pub async fn paused(&self) -> Result<bool> {
        Ok(self.command("GET", &[self.key("paused")]).await? != Value::Nil)
    }
    pub async fn terminate(&self) -> Result<()> {
        self.terminate_with_wait(false).await
    }
    pub async fn terminate_with_wait(&self, wait: bool) -> Result<()> {
        self.eval(
            "redis.call('SET',KEYS[1],ARGV[1]);redis.call('SET',KEYS[2],ARGV[2]);return 1",
            vec![self.key("terminate-wait"), self.key("terminate")],
            vec![
                if wait { "1" } else { "0" }.into(),
                uuid::Uuid::now_v7().to_string(),
            ],
        )
        .await?;
        Ok(())
    }
    pub async fn termination_wait(&self) -> Result<bool> {
        Ok(self.command("GET", &[self.key("terminate-wait")]).await?
            == Value::BulkString(b"1".to_vec()))
    }
    pub(crate) async fn generation(&self) -> Result<Option<String>> {
        let value = self.command("GET", &[self.key("terminate")]).await?;
        if value == Value::Nil {
            Ok(None)
        } else {
            text(&value).map(Some)
        }
    }
    pub async fn masters(&self) -> Result<Vec<serde_json::Value>> {
        let now = self.now().await?;
        let Value::Array(ids) = self
            .command(
                "ZRANGEBYSCORE",
                &[
                    self.key("masters"),
                    now.saturating_sub(15000).to_string(),
                    "+inf".into(),
                    "LIMIT".into(),
                    "0".into(),
                    "100".into(),
                ],
            )
            .await?
        else {
            return Err(Error::Redis);
        };
        let mut result = Vec::new();
        for id in ids {
            let id = text(&id)?;
            if !crate::master_names::valid_process_name(&id) {
                return Err(Error::Redis);
            }
            let value = self
                .command("GET", &[self.key(&format!("master:{id}"))])
                .await?;
            if value != Value::Nil {
                result.push(serde_json::from_str(&text(&value)?).map_err(|_| Error::Redis)?);
            }
        }
        Ok(result)
    }
}
pub(crate) fn text(value: &Value) -> Result<String> {
    match value {
        Value::BulkString(v) => String::from_utf8(v.clone()).map_err(|_| Error::Redis),
        Value::SimpleString(v) => Ok(v.clone()),
        Value::Int(v) => Ok(v.to_string()),
        _ => Err(Error::Redis),
    }
}
fn job_member(record: &JobRecord) -> Result<String> {
    let route = (&record.connection, &record.queue, &record.id);
    Ok(encode(
        &serde_json::to_string(&route).map_err(|_| Error::InvalidRecord)?,
    ))
}

fn encode(value: &str) -> String {
    use std::fmt::Write;
    value.as_bytes().iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}
