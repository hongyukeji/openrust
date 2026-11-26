use std::{
    borrow::Cow,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    contracts::queue::{ClearableQueue, Job, Queue as Contract, QueueMetrics},
    redis::connections::Connection,
};

use super::{
    Delay, JobContext, Payload, Queue, QueueError, QueueFuture, QueueManager, QueueResult, backend,
    events::{JobQueued, JobSnapshot, RedisJobQueueing, RedisJobReserved, RedisJobsMigrated},
    job_popper::may_resume_reservation,
    jobs::{Reservation, ReservedJob},
    new_id, queue_name,
};

const CLOCK: &str = "local t=redis.call('TIME'); local now=t[1]*1000+math.floor(t[2]/1000)\n";
const ENQUEUE: &str = r"
local available = tonumber(ARGV[2])
if ARGV[3] == 'relative' then available = now + available end
if available <= now then
    redis.call('RPUSH', KEYS[1], ARGV[1])
    redis.call('RPUSH', KEYS[3], 1)
else
    redis.call('ZADD', KEYS[2], available, ARGV[1])
end
return 1
";

const MIGRATE: &str = r"
local function move_due(source, ready, notify)
    local jobs = redis.call('ZRANGEBYSCORE', source, '-inf', now, 'LIMIT', 0, 1000)
    for _, job in ipairs(jobs) do
        redis.call('RPUSH', ready, job)
        redis.call('RPUSH', notify, 1)
        redis.call('ZREM', source, job)
    end
    return jobs
end
";
const RESERVE: &str = r"
local raw = redis.call('LINDEX', KEYS[1], 0)
if not raw then return false end
local job = cjson.decode(raw)
job.attempts = job.attempts + 1
local reserved = cjson.encode(job)
redis.call('ZADD', KEYS[2], now + tonumber(ARGV[1]), reserved)
redis.call('LPOP', KEYS[1])
redis.call('LPOP', KEYS[3])
return reserved
";

// The exact serialized reservation includes its attempt number, fencing stale workers.
const ACK: &str = r"
local expires = redis.call('ZSCORE', KEYS[1], ARGV[1])
if not expires or tonumber(expires) <= now then return 0 end
return redis.call('ZREM', KEYS[1], ARGV[1])
";

const RELEASE: &str = r"
local expires = redis.call('ZSCORE', KEYS[1], ARGV[1])
if not expires or tonumber(expires) <= now then return 0 end
redis.call('ZADD', KEYS[2], now + tonumber(ARGV[2]), ARGV[1])
return redis.call('ZREM', KEYS[1], ARGV[1])
";

// 精确匹配尚未过期的保留项。未运行的任务回到队头并恢复领取前的尝试次数。
const RESTORE_UNSTARTED: &str = r"
local expires = redis.call('ZSCORE', KEYS[1], ARGV[1])
if not expires or tonumber(expires) <= now then return 0 end
local job = cjson.decode(ARGV[1])
job.attempts = job.attempts - 1
redis.call('RPUSH', KEYS[3], 1)
redis.call('LPUSH', KEYS[2], cjson.encode(job))
return redis.call('ZREM', KEYS[1], ARGV[1])
";

const CHAIN_ENQUEUE: &str = r"
local available = tonumber(ARGV[3])
if ARGV[4] == 'relative' then available = now + available end
local function check_type(key, expected)
    local kind = redis.call('TYPE', key).ok
    return kind == 'none' or kind == expected
end
if available <= now then
    if not check_type(KEYS[2], 'list') or not check_type(KEYS[4], 'list') then
        return redis.error_reply('Invalid chain queue key type')
    end
    redis.call('RPUSH', KEYS[2], ARGV[2])
    redis.call('RPUSH', KEYS[4], 1)
else
    if not check_type(KEYS[3], 'zset') then
        return redis.error_reply('Invalid delayed queue key type')
    end
    redis.call('ZADD', KEYS[3], available, ARGV[2])
end
";

const ADVANCE_CHAIN_START: &str = r"
local expires = redis.call('ZSCORE', KEYS[1], ARGV[1])
if not expires or tonumber(expires) <= now then return 0 end
";
const ADVANCE_CHAIN_END: &str = "return redis.call('ZREM', KEYS[1], ARGV[1])";

const SIZE: &str = r"
return redis.call('LLEN', KEYS[1]) + redis.call('ZCARD', KEYS[2]) + redis.call('ZCARD', KEYS[3])
";

const CLEAR: &str = r"
local count = redis.call('LLEN', KEYS[1]) + redis.call('ZCARD', KEYS[2]) + redis.call('ZCARD', KEYS[3])
redis.call('DEL', KEYS[1], KEYS[2], KEYS[3], KEYS[4])
return count
";

const METRICS: &str = r"
return {
    redis.call('LLEN', KEYS[1]),
    redis.call('ZCARD', KEYS[2]),
    redis.call('ZCARD', KEYS[3]),
    redis.call('LINDEX', KEYS[1], 0)
}
";

#[derive(Serialize, Deserialize)]
struct RedisJob {
    id: String,
    // Lua changes reservation metadata without rounding JSON numbers inside the payload.
    payload: String,
    attempts: u32,
    #[serde(default)]
    created_at: Option<u64>,
}

/// Stores and reserves jobs using a Redis connection.
pub struct RedisQueue {
    pub(super) name: String,
    pub(super) queue: String,
    pub(super) retry_after: i64,
    pub(super) block_for: Option<u32>,
    pub(super) connection: Arc<Connection>,
    pub(super) registry: Arc<Queue>,
}

impl RedisQueue {
    pub(super) fn queue_name<'a>(&'a self, selected: Option<&'a str>) -> QueueResult<&'a str> {
        let queue = selected.unwrap_or(&self.queue);
        Self::validate_queue_name(queue)?;
        Ok(queue)
    }

    /// 验证原生 Redis 队列名称及传输内部的保留后缀。
    pub fn validate_queue_name(name: &str) -> QueueResult<()> {
        queue_name(Some(name), "")?;
        if [":delayed", ":reserved", ":notify"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
        {
            return Err(QueueError::InvalidConfiguration);
        }
        Ok(())
    }

    fn keys(&self, queue: &str) -> [String; 3] {
        let ready = self.connection.key(&format!("queues:{queue}"));
        let delayed = format!("{ready}:delayed");
        let reserved = format!("{ready}:reserved");
        [ready, delayed, reserved]
    }

    async fn enqueue(
        &self,
        delay: Delay,
        payload: &Payload,
        queue: Option<&str>,
    ) -> QueueResult<Option<String>> {
        let queue = self.queue_name(queue)?;
        let [ready, delayed, _] = self.keys(queue);
        let notify = format!("{ready}:notify");
        let manager = self.registry.manager()?;
        let payload = manager.prepare_redis_payload(payload).await?;
        let job = RedisJob {
            id: new_id()?,
            payload: payload.encode()?,
            attempts: 0,
            created_at: Some(created_at()),
        };
        let raw = serde_json::to_string(&job).map_err(|_| QueueError::InvalidPayload)?;
        let mut command = script(ENQUEUE, &[&ready, &delayed, &notify]);
        let (mode, duration) = match delay {
            Delay::For(duration) => ("relative", duration),
            Delay::Until(time) => (
                "absolute",
                time.duration_since(UNIX_EPOCH).unwrap_or_default(),
            ),
        };
        command.arg(raw).arg(milliseconds(duration)?).arg(mode);
        if manager.observes_worker_event(RedisJobQueueing::NAME)? {
            manager
                .emit_control_event(RedisJobQueueing {
                    connection_name: self.name.clone(),
                    queue: queue.into(),
                    id: job.id.clone(),
                    job: payload.as_ref().clone(),
                })
                .await?;
        }
        self.connection
            .query::<u32>(&command)
            .await
            .map_err(backend)?;
        if manager.observes_worker_event(JobQueued::NAME)? {
            manager
                .emit_control_event(JobQueued {
                    connection_name: self.name.clone(),
                    queue: queue.into(),
                    id: job.id.clone(),
                    job: payload.into_owned(),
                })
                .await?;
        }
        Ok(Some(job.id))
    }

    async fn reserve(&self, queue: &str) -> QueueResult<Option<Box<dyn Job>>> {
        let lease = self
            .retry_after
            .checked_mul(1000)
            .ok_or(QueueError::InvalidConfiguration)?;
        let [ready, delayed, reserved] = self.keys(queue);
        let notify = format!("{ready}:notify");
        let manager = self.registry.manager().ok();
        let mut command = if let Some(manager) = manager.as_ref().filter(|manager| {
            matches!(
                manager.observes_worker_event(RedisJobsMigrated::NAME),
                Ok(true)
            )
        }) {
            for source in [&delayed, &reserved] {
                self.migrate(manager, queue, source, &ready, &notify)
                    .await?;
            }
            script(RESERVE, &[&ready, &reserved, &notify])
        } else {
            script(
                &format!(
                    "{MIGRATE}move_due(KEYS[4], KEYS[1], KEYS[3])\nmove_due(KEYS[2], KEYS[1], KEYS[3])\n{RESERVE}"
                ),
                &[&ready, &reserved, &notify, &delayed],
            )
        };
        command.arg(lease);
        let Some(raw) = self
            .connection
            .query::<Option<String>>(&command)
            .await
            .map_err(backend)?
        else {
            return Ok(None);
        };
        // 领取请求与所有权检查可走不同连接。只在复核通过后把保留交给 Worker。
        let allowed = may_resume_reservation().await;
        if !matches!(allowed, Ok(true)) {
            let mut restore = script(RESTORE_UNSTARTED, &[&reserved, &ready, &notify]);
            restore.arg(&raw);
            self.connection
                .query::<u32>(&restore)
                .await
                .map_err(backend)?;
            allowed?;
            return Ok(None);
        }
        let job: RedisJob = serde_json::from_str(&raw).map_err(|_| QueueError::InvalidPayload)?;
        let payload = Payload::decode(&job.payload)?;
        let storage = RedisReservation {
            connection: self.connection.clone(),
            registry: self.registry.clone(),
            connection_name: self.name.clone(),
            queue: queue.into(),
            ready,
            delayed,
            reserved,
            raw,
            reserved_at: tokio::time::Instant::now(),
        };
        let reservation = ReservedJob {
            registry: self.registry.clone(),
            storage: Box::new(storage),
            id: job.id,
            context: JobContext {
                connection: self.name.clone(),
                queue: queue.into(),
                attempts: job.attempts,
                transaction: None,
                ..JobContext::default()
            },
            payload,
            deleted: false,
            released: false,
            failed: false,
        };
        if let Some(manager) = manager
            && matches!(
                manager.observes_worker_event(RedisJobReserved::NAME),
                Ok(true)
            )
        {
            manager
                .observe_confirmed_event(RedisJobReserved {
                    connection_name: self.name.clone(),
                    job: JobSnapshot::from_job(&reservation),
                })
                .await;
        }
        Ok(Some(Box::new(reservation)))
    }

    async fn migrate(
        &self,
        manager: &QueueManager,
        queue: &str,
        source: &str,
        ready: &str,
        notify: &str,
    ) -> QueueResult<()> {
        let command = script(
            &format!("{MIGRATE}return move_due(KEYS[1], KEYS[2], KEYS[3])"),
            &[source, ready, notify],
        );
        let raw: Vec<String> = self.connection.query(&command).await.map_err(backend)?;
        let mut jobs = Vec::with_capacity(raw.len());
        for raw in raw {
            let job = serde_json::from_str::<RedisJob>(&raw)
                .map_err(|_| QueueError::InvalidPayload)
                .and_then(|job| {
                    Ok(JobSnapshot {
                        id: job.id,
                        queue: queue.into(),
                        attempts: job.attempts,
                        payload: Payload::decode(&job.payload)?,
                    })
                });
            match job {
                Ok(job) => jobs.push(job),
                Err(error) => manager.report_confirmed_event_error(&error).await,
            }
        }
        manager
            .observe_confirmed_event(RedisJobsMigrated {
                connection_name: self.name.clone(),
                queue: queue.into(),
                jobs,
            })
            .await;
        Ok(())
    }
    async fn pop_waiting(
        &self,
        queue: &str,
        wait: Option<Duration>,
    ) -> QueueResult<Option<Box<dyn Job>>> {
        let started = tokio::time::Instant::now();
        let first = self.reserve(queue).await?;
        let Some(seconds) = self.block_for.filter(|_| first.is_none()) else {
            return Ok(first);
        };
        let configured = (seconds != 0).then(|| Duration::from_secs(u64::from(seconds)));
        let limit = match (configured, wait) {
            (Some(configured), Some(wait)) => Some(configured.min(wait)),
            (configured, wait) => configured.or(wait),
        };
        let deadline = limit.and_then(|limit| started.checked_add(limit));
        if deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
            return Ok(None);
        }
        let [ready, _, _] = self.keys(queue);
        let notify = format!("{ready}:notify");
        // A dedicated connection keeps BLPOP from stalling unrelated Redis commands.
        let mut connection = self
            .connection
            .blocking_transport()
            .await
            .map_err(backend)?;
        loop {
            let remaining = deadline
                .map(|deadline| deadline.saturating_duration_since(tokio::time::Instant::now()));
            if remaining.is_some_and(|remaining| remaining.is_zero()) {
                return Ok(None);
            }
            let slice = remaining
                .unwrap_or(Duration::from_secs(1))
                .min(Duration::from_secs(1))
                .max(Duration::from_millis(1));
            // Bounded waits allow delayed jobs and expired reservations to be promoted.
            let mut command = ::redis::cmd("BLPOP");
            command.arg(&notify).arg(slice.as_secs_f64());
            let _: Option<(String, String)> = tokio::time::timeout(
                slice + Duration::from_secs(2),
                command.query_async(&mut connection),
            )
            .await
            .map_err(backend)?
            .map_err(backend)?;
            if !may_resume_reservation().await? {
                return Ok(None);
            }
            if let Some(job) = self.reserve(queue).await? {
                return Ok(Some(job));
            }
        }
    }
}

impl ClearableQueue for RedisQueue {
    fn clear<'a>(&'a self, queue: &'a str) -> QueueFuture<'a, QueueResult<u64>> {
        Box::pin(async move {
            let [ready, delayed, reserved] = self.keys(self.queue_name(Some(queue))?);
            let notify = format!("{ready}:notify");
            let command = script(CLEAR, &[&ready, &delayed, &reserved, &notify]);
            self.connection.query(&command).await.map_err(backend)
        })
    }
}

impl Contract for RedisQueue {
    fn clearable(&self) -> Option<&dyn ClearableQueue> {
        Some(self)
    }

    fn get_connection_name(&self) -> String {
        self.name.clone()
    }

    fn default_queue(&self) -> &str {
        &self.queue
    }

    fn size<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<u64>> {
        Box::pin(async move {
            let [ready, delayed, reserved] = self.keys(self.queue_name(queue)?);
            let command = script(SIZE, &[&ready, &delayed, &reserved]);
            self.connection.query(&command).await.map_err(backend)
        })
    }

    fn metrics<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<QueueMetrics>> {
        Box::pin(async move {
            let [ready, delayed, reserved] = self.keys(self.queue_name(queue)?);
            let command = script(METRICS, &[&ready, &delayed, &reserved]);
            let (pending, delayed, reserved, first): (u64, u64, u64, Option<Vec<u8>>) =
                self.connection.query(&command).await.map_err(backend)?;
            let oldest_pending = first
                .and_then(|raw| serde_json::from_slice::<RedisJob>(&raw).ok())
                .and_then(|job| job.created_at);
            Ok(QueueMetrics {
                size: pending + delayed + reserved,
                pending: Some(pending),
                delayed: Some(delayed),
                reserved: Some(reserved),
                oldest_pending,
            })
        })
    }

    fn push<'a>(
        &'a self,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(self.enqueue(Delay::For(Duration::ZERO), payload, queue))
    }

    fn later<'a>(
        &'a self,
        delay: Delay,
        payload: &'a Payload,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(self.enqueue(delay, payload, queue))
    }

    fn pop<'a>(
        &'a self,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        Box::pin(async move { self.pop_waiting(self.queue_name(queue)?, None).await })
    }
    fn pop_within<'a>(
        &'a self,
        queue: Option<&'a str>,
        wait: Duration,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        Box::pin(async move { self.pop_waiting(self.queue_name(queue)?, Some(wait)).await })
    }

    fn pop_now<'a>(
        &'a self,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>> {
        Box::pin(async move { self.reserve(self.queue_name(queue)?).await })
    }
}

struct RedisReservation {
    connection: Arc<Connection>,
    registry: Arc<Queue>,
    connection_name: String,
    queue: String,
    ready: String,
    delayed: String,
    reserved: String,
    raw: String,
    reserved_at: tokio::time::Instant,
}

impl RedisReservation {
    async fn run(&self, command: ::redis::Cmd) -> QueueResult<()> {
        let count = self
            .connection
            .query::<u32>(&command)
            .await
            .map_err(backend)?;
        if count == 1 {
            Ok(())
        } else {
            Err(QueueError::LostReservation)
        }
    }
}

impl Reservation for RedisReservation {
    fn redis_reserved_at(&self) -> Option<tokio::time::Instant> {
        Some(self.reserved_at)
    }

    fn advance_chain<'a>(
        &'a mut self,
        next: &'a Payload,
        queue: Option<&'a str>,
        delay: Option<Delay>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let target = queue.unwrap_or(&self.queue);
            RedisQueue::validate_queue_name(target)?;
            let ready = queue.map_or_else(
                || self.ready.clone(),
                |queue| self.connection.key(&format!("queues:{queue}")),
            );
            let delayed = format!("{ready}:delayed");
            let notify = format!("{ready}:notify");
            let manager = self.registry.manager().ok();
            let next = match &manager {
                Some(manager) => manager.prepare_redis_payload(next).await?,
                None => Cow::Borrowed(next),
            };
            let (command, id) = chain_enqueue_command(
                [&self.reserved, &ready, &delayed, &notify],
                &self.raw,
                &next,
                delay,
            )?;
            if let Some(manager) = &manager
                && matches!(
                    manager.observes_worker_event(RedisJobQueueing::NAME),
                    Ok(true)
                )
            {
                manager
                    .emit_control_event(RedisJobQueueing {
                        connection_name: self.connection_name.clone(),
                        queue: target.into(),
                        id: id.clone(),
                        job: next.as_ref().clone(),
                    })
                    .await?;
            }
            self.run(command).await?;
            if let Some(manager) = manager
                && matches!(manager.observes_worker_event(JobQueued::NAME), Ok(true))
            {
                manager
                    .observe_confirmed_event(JobQueued {
                        connection_name: self.connection_name.clone(),
                        queue: target.into(),
                        id,
                        job: next.into_owned(),
                    })
                    .await;
            }
            Ok(())
        })
    }

    fn delete(&mut self) -> QueueFuture<'_, QueueResult<()>> {
        Box::pin(async move {
            let mut command = script(ACK, &[&self.reserved]);
            command.arg(&self.raw);
            self.run(command).await
        })
    }

    fn release(&mut self, delay: Duration) -> QueueFuture<'_, QueueResult<()>> {
        Box::pin(async move {
            let mut command = script(RELEASE, &[&self.reserved, &self.delayed]);
            command.arg(&self.raw).arg(milliseconds(delay)?);
            self.run(command).await
        })
    }
}

fn created_at() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn chain_enqueue_command(
    keys: [&str; 4],
    reservation: &str,
    payload: &Payload,
    delay: Option<Delay>,
) -> QueueResult<(::redis::Cmd, String)> {
    let job = RedisJob {
        id: new_id()?,
        payload: payload.encode()?,
        attempts: 0,
        created_at: Some(created_at()),
    };
    let raw = serde_json::to_string(&job).map_err(|_| QueueError::InvalidPayload)?;
    let (mode, duration) = match delay.unwrap_or(Delay::For(Duration::ZERO)) {
        Delay::For(duration) => ("relative", duration),
        Delay::Until(time) => (
            "absolute",
            time.duration_since(UNIX_EPOCH).unwrap_or_default(),
        ),
    };
    let mut command = script(
        &format!("{ADVANCE_CHAIN_START}{CHAIN_ENQUEUE}{ADVANCE_CHAIN_END}"),
        &keys,
    );
    command
        .arg(reservation)
        .arg(raw)
        .arg(milliseconds(duration)?)
        .arg(mode);
    Ok((command, job.id))
}

fn script(body: &str, keys: &[&str]) -> ::redis::Cmd {
    let mut command = ::redis::cmd("EVAL");
    command
        .arg(format!("{CLOCK}{body}"))
        .arg(keys.len())
        .arg(keys);
    command
}

fn milliseconds(duration: Duration) -> QueueResult<i64> {
    // Redis scores and Lua numbers exactly represent integers through 2^53.
    let millis = duration.as_millis();
    if millis > 1_u128 << 52 {
        return Err(QueueError::InvalidPayload);
    }
    Ok(i64::try_from(millis).expect("bounded duration"))
}
