//! 基于 Redis 的队列驱动，使用令牌保护租约。

use super::{
    DispatchOptions, JobContext, JobRegistry, QueueDriver, new_id, persist_execution_metadata,
    restore_execution_metadata, safe_error_code, validate_name, validate_options, validate_payload,
};
use crate::{AppState, BoxFuture, Cancellation, Error, ExecutionMetadata, Result};
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::sync::{OnceCell, watch};

const DISPATCH: &str = r"
local t=redis.call('TIME'); local now=t[1]*1000+math.floor(t[2]/1000)
if ARGV[4]~='' then local old=redis.call('HGET',KEYS[3],ARGV[4]); if old and redis.call('HEXISTS',KEYS[1],old)==1 then return old end end
local j=cjson.decode(ARGV[2]); j.available_at=now+tonumber(ARGV[3])
redis.call('HSET',KEYS[1],ARGV[1],cjson.encode(j)); redis.call('ZADD',KEYS[2],j.available_at,ARGV[1])
if ARGV[4]~='' then redis.call('HSET',KEYS[3],ARGV[4],ARGV[1]) end
return ARGV[1]
";

const CLAIM: &str = r"
local t=redis.call('TIME'); local now=t[1]*1000+math.floor(t[2]/1000)
local xs=redis.call('ZRANGEBYSCORE',KEYS[3],'-inf',now,'LIMIT',0,100)
for _,id in ipairs(xs) do
 local raw=redis.call('HGET',KEYS[1],id); redis.call('ZREM',KEYS[3],id)
 if raw then
  local j=cjson.decode(raw)
  if tonumber(j.lease_until or 0)<=now then
   j.owner=cjson.null; j.lease_until=cjson.null
   if tonumber(j.attempts)>=tonumber(j.max_attempts) then
    j.error_code='worker_lost'; j.failed_at=now; redis.call('HSET',KEYS[4],id,cjson.encode(j)); redis.call('HDEL',KEYS[1],id)
    if j.unique_field and j.unique_field~=cjson.null then redis.call('HDEL',KEYS[5],j.unique_field) end
   else
    local delay=math.min(2^math.min(math.max(tonumber(j.attempts)-1,0),18),300)*1000
    j.available_at=now+delay; j.error_code='worker_lost'; redis.call('HSET',KEYS[1],id,cjson.encode(j)); redis.call('ZADD',KEYS[2],j.available_at,id)
   end
  end
 end
end
local ready=redis.call('ZRANGEBYSCORE',KEYS[2],'-inf',now,'LIMIT',0,1); if #ready==0 then return false end
local id=ready[1]; local raw=redis.call('HGET',KEYS[1],id); if not raw then redis.call('ZREM',KEYS[2],id); return false end
local j=cjson.decode(raw); redis.call('ZREM',KEYS[2],id); j.attempts=tonumber(j.attempts)+1
j.owner=ARGV[1]; j.lease_until=now+tonumber(ARGV[2]); redis.call('HSET',KEYS[1],id,cjson.encode(j)); redis.call('ZADD',KEYS[3],j.lease_until,id)
return cjson.encode(j)
";

const RENEW: &str = r"
local t=redis.call('TIME'); local now=t[1]*1000+math.floor(t[2]/1000); local raw=redis.call('HGET',KEYS[1],ARGV[1])
if not raw then return 0 end; local j=cjson.decode(raw)
if j.owner~=ARGV[2] or tonumber(j.lease_until or 0)<=now then return 0 end
j.lease_until=now+tonumber(ARGV[3]); redis.call('HSET',KEYS[1],ARGV[1],cjson.encode(j)); redis.call('ZADD',KEYS[2],j.lease_until,ARGV[1]); return 1
";

const ACK: &str = r"
local t=redis.call('TIME'); local now=t[1]*1000+math.floor(t[2]/1000)
local raw=redis.call('HGET',KEYS[1],ARGV[1]); if not raw then return 0 end; local j=cjson.decode(raw)
if j.owner~=ARGV[2] or tonumber(j.lease_until or 0)<=now then return 0 end
redis.call('HDEL',KEYS[1],ARGV[1]); redis.call('ZREM',KEYS[2],ARGV[1])
if j.unique_field and j.unique_field~=cjson.null then redis.call('HDEL',KEYS[3],j.unique_field) end; return 1
";

const FAIL: &str = r"
local t=redis.call('TIME'); local now=t[1]*1000+math.floor(t[2]/1000); local raw=redis.call('HGET',KEYS[1],ARGV[1])
if not raw then return 0 end; local j=cjson.decode(raw); if j.owner~=ARGV[2] then return 0 end
if tonumber(j.lease_until or 0)<=now then return 0 end
redis.call('ZREM',KEYS[3],ARGV[1]); j.owner=cjson.null; j.lease_until=cjson.null; j.error_code=ARGV[3]
if tonumber(j.attempts)>=tonumber(j.max_attempts) then
 j.failed_at=now; redis.call('HSET',KEYS[2],ARGV[1],cjson.encode(j)); redis.call('HDEL',KEYS[1],ARGV[1])
 if j.unique_field and j.unique_field~=cjson.null then redis.call('HDEL',KEYS[4],j.unique_field) end
else
 j.available_at=now+tonumber(ARGV[4]); redis.call('HSET',KEYS[1],ARGV[1],cjson.encode(j)); redis.call('ZADD',KEYS[5],j.available_at,ARGV[1])
end
return 1
";

const PROGRESS: &str = r"
local t=redis.call('TIME'); local now=t[1]*1000+math.floor(t[2]/1000)
local raw=redis.call('HGET',KEYS[1],ARGV[1]); if not raw then return 0 end; local j=cjson.decode(raw)
if j.owner~=ARGV[2] or tonumber(j.lease_until or 0)<=now then return 0 end
j.progress=cjson.decode(ARGV[3]); redis.call('HSET',KEYS[1],ARGV[1],cjson.encode(j)); return 1
";

const RETRY: &str = r"
local raw=redis.call('HGET',KEYS[1],ARGV[1]); if not raw then return redis.error_reply('failed_job_not_found') end
local f=cjson.decode(raw); if f.retried_job_id and f.retried_job_id~=cjson.null then return f.retried_job_id end
local j=cjson.decode(ARGV[3]); local t=redis.call('TIME'); local now=t[1]*1000+math.floor(t[2]/1000)
j.available_at=now; j.attempts=0; j.owner=cjson.null; j.lease_until=cjson.null; j.progress=cjson.null; j.error_code=cjson.null; j.unique_field=cjson.null; j.retried_job_id=cjson.null
redis.call('HSET',KEYS[2],ARGV[2],cjson.encode(j)); redis.call('ZADD',KEYS[3],now,ARGV[2]); f.retried_job_id=ARGV[2]; redis.call('HSET',KEYS[1],ARGV[1],cjson.encode(f)); return ARGV[2]
";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RedisJob {
    id: String,
    queue: String,
    job_name: String,
    payload: Value,
    attempts: u64,
    max_attempts: u32,
    available_at: u64,
    owner: Option<String>,
    lease_until: Option<u64>,
    progress: Option<Value>,
    error_code: Option<String>,
    failed_at: Option<u64>,
    unique_field: Option<String>,
    retried_job_id: Option<String>,
}

/// 使用 Redis 共享任务状态的可选队列驱动。
///
/// 启用 `queue-redis` feature 后，标准应用通过 `QueuePlugin` 和 `queue` 配置选择。
/// Redis 的持久性与故障切换取决于 Redis 部署设置。
pub struct RedisQueueDriver {
    client: redis::Client,
    connection: OnceCell<ConnectionManager>,
    prefix: String,
    registry: Arc<JobRegistry>,
    config: crate::config::QueueConfig,
    shutdown: watch::Sender<bool>,
}

impl RedisQueueDriver {
    /// 创建显式命名空间的 Redis 队列驱动；网络连接在首次操作时建立。
    pub fn new(
        url: &str,
        namespace: &str,
        registry: JobRegistry,
        config: crate::config::QueueConfig,
    ) -> Result<Self> {
        validate_namespace(namespace)?;
        let client = redis::Client::open(url)
            .map_err(|_| Error::custom("invalid_queue_redis_url", "Redis queue URL is invalid"))?;
        Ok(Self {
            client,
            connection: OnceCell::new(),
            prefix: format!("{{openrust:{namespace}}}"),
            registry: Arc::new(registry),
            config,
            shutdown: watch::channel(false).0,
        })
    }

    async fn connection(&self) -> Result<ConnectionManager> {
        self.connection
            .get_or_try_init(|| async {
                ConnectionManager::new(self.client.clone())
                    .await
                    .map_err(|_| redis_unavailable())
            })
            .await
            .cloned()
    }

    fn keys(&self, queue: &str) -> QueueKeys {
        let q = format!("{}:{queue}", self.prefix);
        QueueKeys {
            jobs: format!("{}:jobs", self.prefix),
            unique: format!("{}:unique", self.prefix),
            failed: format!("{}:failed", self.prefix),
            due: format!("{q}:due"),
            leases: format!("{q}:leases"),
        }
    }

    async fn dispatch_value(
        &self,
        queue: &str,
        job_name: &str,
        payload: Value,
        options: DispatchOptions,
    ) -> Result<String> {
        validate_name("queue", queue, 64)?;
        validate_name("job name", job_name, 128)?;
        validate_payload(&payload)?;
        validate_options(&options)?;
        let id = new_id()?;
        let unique_field = options
            .unique_key
            .as_deref()
            .map(|key| unique_field(queue, key));
        let job = RedisJob {
            id: id.clone(),
            queue: queue.to_owned(),
            job_name: job_name.to_owned(),
            payload,
            attempts: 0,
            max_attempts: options.max_attempts,
            available_at: 0,
            owner: None,
            lease_until: None,
            progress: None,
            error_code: None,
            failed_at: None,
            unique_field,
            retried_job_id: None,
        };
        let encoded = serde_json::to_string(&job).map_err(Error::internal)?;
        let keys = self.keys(queue);
        let mut connection = self.connection().await?;
        redis::cmd("EVAL")
            .arg(DISPATCH)
            .arg(3)
            .arg(keys.jobs)
            .arg(keys.due)
            .arg(keys.unique)
            .arg(&id)
            .arg(encoded)
            .arg(options.delay.as_millis().to_string())
            .arg(job.unique_field.as_deref().unwrap_or_default())
            .query_async(&mut connection)
            .await
            .map_err(|_| redis_unavailable())
    }

    // Redis 领取、续租、handler 生命周期与最终确认共享一条 token-fencing 状态机。
    #[allow(clippy::too_many_lines)]
    async fn run_next_inner(&self, queue: &str, state: Option<AppState>) -> Result<bool> {
        validate_name("queue", queue, 64)?;
        let mut connection = self.connection().await?;
        let keys = self.keys(queue);
        let owner = new_id()?;
        let lease_ms = self.config.lease_secs.saturating_mul(1000);
        let raw: Option<String> = redis::cmd("EVAL")
            .arg(CLAIM)
            .arg(5)
            .arg(&keys.jobs)
            .arg(&keys.due)
            .arg(&keys.leases)
            .arg(&keys.failed)
            .arg(&keys.unique)
            .arg(&owner)
            .arg(lease_ms)
            .query_async(&mut connection)
            .await
            .map_err(|_| redis_unavailable())?;
        let Some(raw) = raw else { return Ok(false) };
        let job: RedisJob = serde_json::from_str(&raw).map_err(Error::internal)?;
        let (payload, metadata) = restore_execution_metadata(job.payload.clone())?;
        let cancellation = Cancellation::new();
        let mut shutdown = self.shutdown.subscribe();
        let deadline =
            tokio::time::Instant::now() + Duration::from_secs(self.config.job_timeout_secs);
        let progress_connection = connection.clone();
        let progress_key = keys.jobs.clone();
        let progress_id = job.id.clone();
        let progress_owner = owner.clone();
        let context = JobContext::new(
            job.id.clone(),
            job.attempts,
            u64::from(job.max_attempts),
            cancellation.clone(),
        )?
        .with_deadline(deadline)
        .with_execution_metadata(metadata)
        .with_progress_reporter(move |progress| {
            let mut connection = progress_connection.clone();
            let key = progress_key.clone();
            let id = progress_id.clone();
            let owner = progress_owner.clone();
            Box::pin(async move {
                let encoded = serde_json::to_string(&progress).map_err(Error::internal)?;
                let updated: i64 = redis::cmd("EVAL")
                    .arg(PROGRESS)
                    .arg(1)
                    .arg(key)
                    .arg(id)
                    .arg(owner)
                    .arg(encoded)
                    .query_async(&mut connection)
                    .await
                    .map_err(|_| redis_unavailable())?;
                if updated == 1 {
                    Ok(())
                } else {
                    Err(Error::custom(
                        "queue_lease_lost",
                        "job lease ownership changed",
                    ))
                }
            })
        });
        let context = state.map_or(context.clone(), |state| context.with_app_state(state));
        let registry = Arc::clone(&self.registry);
        let mut handler = Box::pin(registry.execute(&job.job_name, payload, context));
        let heartbeat_ms = std::cmp::Ord::max(lease_ms / 3, 100);
        let mut heartbeat = tokio::time::interval(Duration::from_millis(heartbeat_ms));
        heartbeat.tick().await;
        let mut shutdown_deadline = None;
        let result = loop {
            tokio::select! {
                result = &mut handler => break Some(result),
                _ = heartbeat.tick() => {
                    match renew(&mut connection, &keys, &job.id, &owner, lease_ms).await {
                        Ok(true) => {}
                        Ok(false) => {
                            cancellation.cancel();
                            break Some(Err(Error::custom("queue_lease_lost", "job lease ownership changed")));
                        }
                        Err(error) => {
                            cancellation.cancel();
                            break Some(Err(error));
                        }
                    }
                }
                () = tokio::time::sleep_until(deadline) => {
                    cancellation.cancel();
                    break None;
                }
                changed = shutdown.changed() => {
                    if changed.is_ok() && *shutdown.borrow() && shutdown_deadline.is_none() {
                        cancellation.cancel();
                        shutdown_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(self.config.shutdown_timeout_secs));
                    }
                }
                () = async {
                    if let Some(deadline) = shutdown_deadline { tokio::time::sleep_until(deadline).await }
                    else { std::future::pending::<()>().await }
                } => {
                    break Some(Err(Error::custom("queue_shutdown_timeout", "job did not stop before the worker shutdown deadline")));
                }
            }
        };
        match result {
            Some(Ok(())) => acknowledge(&mut connection, &keys, &job.id, &owner).await?,
            Some(Err(error)) => {
                fail(
                    &mut connection,
                    &keys,
                    &job.id,
                    &owner,
                    safe_error_code(error.code()),
                    retry_delay_ms(job.attempts),
                )
                .await?;
                if error.code() == "queue_shutdown_timeout" {
                    return Err(error);
                }
            }
            None => fail(&mut connection, &keys, &job.id, &owner, "job_timeout", 1000).await?,
        }
        Ok(true)
    }
}

#[derive(Clone)]
struct QueueKeys {
    jobs: String,
    unique: String,
    failed: String,
    due: String,
    leases: String,
}

impl QueueDriver for RedisQueueDriver {
    fn dispatch<'a>(
        &'a self,
        queue: &'a str,
        job_name: &'a str,
        payload: Value,
        options: DispatchOptions,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move { self.dispatch_value(queue, job_name, payload, options).await })
    }
    fn dispatch_in_transaction<'a>(
        &'a self,
        _transaction: &'a crate::db::DatabaseTransaction,
        _queue: &'a str,
        _job_name: &'a str,
        _payload: Value,
        _options: DispatchOptions,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async {
            Err(Error::custom(
                "queue_transaction_unsupported",
                "Redis queue cannot participate in a database transaction",
            ))
        })
    }
    fn dispatch_with_metadata<'a>(
        &'a self,
        queue: &'a str,
        job_name: &'a str,
        payload: Value,
        options: DispatchOptions,
        metadata: ExecutionMetadata,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let payload = persist_execution_metadata(payload, &metadata)?;
            self.dispatch_value(queue, job_name, payload, options).await
        })
    }
    fn dispatch_in_transaction_with_metadata<'a>(
        &'a self,
        _transaction: &'a crate::db::DatabaseTransaction,
        _queue: &'a str,
        _job_name: &'a str,
        _payload: Value,
        _options: DispatchOptions,
        _metadata: ExecutionMetadata,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async {
            Err(Error::custom(
                "queue_transaction_unsupported",
                "Redis queue cannot participate in a database transaction",
            ))
        })
    }
    fn run_next<'a>(&'a self, queue: &'a str) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move { self.run_next_inner(queue, None).await })
    }
    fn run_next_with_state<'a>(
        &'a self,
        queue: &'a str,
        state: &'a AppState,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move { self.run_next_inner(queue, Some(state.clone())).await })
    }
    fn retry_failed<'a>(&'a self, failed_id: &'a str) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            if failed_id.len() != 32 || !failed_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(Error::custom(
                    "invalid_failed_job_id",
                    "failed job ID must be 32 hexadecimal characters",
                ));
            }
            let mut connection = self.connection().await?;
            let failed_key = format!("{}:failed", self.prefix);
            let jobs_key = format!("{}:jobs", self.prefix);
            let raw: Option<String> = redis::cmd("HGET")
                .arg(&failed_key)
                .arg(failed_id)
                .query_async(&mut connection)
                .await
                .map_err(|_| redis_unavailable())?;
            let raw = raw.ok_or_else(|| {
                Error::custom("failed_job_not_found", "failed job does not exist")
            })?;
            let failed: RedisJob = serde_json::from_str(&raw).map_err(Error::internal)?;
            if let Some(id) = failed.retried_job_id {
                return Ok(id);
            }
            let id = new_id()?;
            let keys = self.keys(&failed.queue);
            let replacement = RedisJob {
                id: id.clone(),
                queue: failed.queue,
                job_name: failed.job_name,
                payload: failed.payload,
                attempts: 0,
                max_attempts: 5,
                available_at: 0,
                owner: None,
                lease_until: None,
                progress: None,
                error_code: None,
                failed_at: None,
                unique_field: None,
                retried_job_id: None,
            };
            let encoded = serde_json::to_string(&replacement).map_err(Error::internal)?;
            let result: String = redis::cmd("EVAL")
                .arg(RETRY)
                .arg(3)
                .arg(&failed_key)
                .arg(&jobs_key)
                .arg(&keys.due)
                .arg(failed_id)
                .arg(&id)
                .arg(encoded)
                .query_async(&mut connection)
                .await
                .map_err(|_| redis_unavailable())?;
            Ok(result)
        })
    }
    fn reset_shutdown(&self) {
        self.shutdown.send_replace(false);
    }
    fn request_shutdown(&self) {
        self.shutdown.send_replace(true);
    }
    fn is_shutdown_requested(&self) -> bool {
        *self.shutdown.borrow()
    }
}

async fn renew(
    connection: &mut ConnectionManager,
    keys: &QueueKeys,
    id: &str,
    owner: &str,
    lease_ms: u64,
) -> Result<bool> {
    let value: i64 = redis::cmd("EVAL")
        .arg(RENEW)
        .arg(2)
        .arg(&keys.jobs)
        .arg(&keys.leases)
        .arg(id)
        .arg(owner)
        .arg(lease_ms)
        .query_async(connection)
        .await
        .map_err(|_| redis_unavailable())?;
    Ok(value == 1)
}
async fn acknowledge(
    connection: &mut ConnectionManager,
    keys: &QueueKeys,
    id: &str,
    owner: &str,
) -> Result<()> {
    let value: i64 = redis::cmd("EVAL")
        .arg(ACK)
        .arg(3)
        .arg(&keys.jobs)
        .arg(&keys.leases)
        .arg(&keys.unique)
        .arg(id)
        .arg(owner)
        .query_async(connection)
        .await
        .map_err(|_| redis_unavailable())?;
    if value == 1 {
        Ok(())
    } else {
        Err(Error::custom(
            "queue_lease_lost",
            "job lease ownership changed before acknowledgement",
        ))
    }
}
async fn fail(
    connection: &mut ConnectionManager,
    keys: &QueueKeys,
    id: &str,
    owner: &str,
    code: &str,
    delay_ms: u64,
) -> Result<()> {
    let value: i64 = redis::cmd("EVAL")
        .arg(FAIL)
        .arg(5)
        .arg(&keys.jobs)
        .arg(&keys.failed)
        .arg(&keys.leases)
        .arg(&keys.unique)
        .arg(&keys.due)
        .arg(id)
        .arg(owner)
        .arg(code)
        .arg(delay_ms)
        .query_async(connection)
        .await
        .map_err(|_| redis_unavailable())?;
    if value == 1 {
        Ok(())
    } else {
        Err(Error::custom(
            "queue_lease_lost",
            "job lease ownership changed before retry scheduling",
        ))
    }
}
fn unique_field(queue: &str, key: &str) -> String {
    use sha2::{Digest as _, Sha256};
    let mut input = queue.as_bytes().to_vec();
    input.push(0);
    input.extend_from_slice(key.as_bytes());
    let mut output = String::new();
    for byte in Sha256::digest(input) {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}
fn retry_delay_ms(attempt: u64) -> u64 {
    let shift = u32::try_from(attempt.saturating_sub(1)).unwrap_or(u32::MAX);
    std::cmp::Ord::min(1_u64.checked_shl(shift).unwrap_or(300), 300) * 1000
}
fn validate_namespace(namespace: &str) -> Result<()> {
    if namespace.is_empty()
        || namespace.len() > 63
        || !namespace
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    {
        return Err(Error::custom(
            "invalid_queue_redis_namespace",
            "Redis queue namespace must contain 1..=63 ASCII letters, digits, '_' or '-'",
        ));
    }
    Ok(())
}
fn redis_unavailable() -> Error {
    Error::custom("queue_redis_unavailable", "Redis queue operation failed")
}
