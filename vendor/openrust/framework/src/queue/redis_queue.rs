use std::{
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    contracts::queue::{Job, Queue as Contract},
    redis::connections::Connection,
};

use super::{
    Delay, JobContext, Payload, Queue, QueueError, QueueFuture, QueueResult, backend,
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

const PUBLISH_BATCH: &str = r"
if redis.call('EXISTS', KEYS[3]) == 1 then return 0 end
for i = 1, 2 do
    local kind = redis.call('TYPE', KEYS[i]).ok
    if kind ~= 'none' and kind ~= 'list' then
        return redis.error_reply('Invalid batch queue key type')
    end
end
for _, job in ipairs(ARGV) do
    redis.call('RPUSH', KEYS[1], job)
    redis.call('RPUSH', KEYS[2], 1)
end
redis.call('SET', KEYS[3], 1)
return 1
";

const RESERVE: &str = r"
local function move_due(source)
    local jobs = redis.call('ZRANGEBYSCORE', source, '-inf', now, 'LIMIT', 0, 1000)
    for _, job in ipairs(jobs) do
        redis.call('RPUSH', KEYS[1], job)
        redis.call('RPUSH', KEYS[4], 1)
        redis.call('ZREM', source, job)
    end
end
move_due(KEYS[2])
move_due(KEYS[3])
local raw = redis.call('LINDEX', KEYS[1], 0)
if not raw then return false end
local job = cjson.decode(raw)
job.attempts = job.attempts + 1
local reserved = cjson.encode(job)
redis.call('ZADD', KEYS[3], now + tonumber(ARGV[1]), reserved)
redis.call('LPOP', KEYS[1])
redis.call('LPOP', KEYS[4])
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
const FORWARD_CHAIN_START: &str = "if redis.call('EXISTS', KEYS[1]) == 1 then return 0 end\n";
const FORWARD_CHAIN_END: &str = "redis.call('SET', KEYS[1], 1); return 1";

const SIZE: &str = r"
return redis.call('LLEN', KEYS[1]) + redis.call('ZCARD', KEYS[2]) + redis.call('ZCARD', KEYS[3])
";

#[derive(Serialize, Deserialize)]
struct RedisJob {
    id: String,
    // Lua changes reservation metadata without rounding JSON numbers inside the payload.
    payload: String,
    attempts: u32,
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
        let queue = queue_name(selected, &self.queue)?;
        if [":delayed", ":reserved", ":notify"]
            .iter()
            .any(|suffix| queue.ends_with(suffix))
        {
            return Err(QueueError::InvalidConfiguration);
        }
        Ok(queue)
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
        let job = RedisJob {
            id: new_id()?,
            payload: payload.encode()?,
            attempts: 0,
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
        self.connection
            .query::<u32>(&command)
            .await
            .map_err(backend)?;
        Ok(Some(job.id))
    }

    fn publication_key(&self, id: &str) -> String {
        self.connection.key(&format!("queue-batches:{id}"))
    }

    pub(super) async fn publish_batch(
        &self,
        id: &str,
        queue: &str,
        payloads: &[Payload],
    ) -> QueueResult<()> {
        let [ready, _, _] = self.keys(self.queue_name(Some(queue))?);
        let notify = format!("{ready}:notify");
        let marker = self.publication_key(id);
        let mut command = script(PUBLISH_BATCH, &[&ready, &notify, &marker]);
        for payload in payloads {
            let job = RedisJob {
                id: payload.uuid().into(),
                payload: payload.encode()?,
                attempts: 0,
            };
            command.arg(serde_json::to_string(&job).map_err(|_| QueueError::InvalidPayload)?);
        }
        self.connection
            .query::<u32>(&command)
            .await
            .map_err(backend)?;
        Ok(())
    }

    pub(super) async fn forward_chain(
        &self,
        delivery: &str,
        payload: &Payload,
        queue: Option<&str>,
        delay: Option<Delay>,
    ) -> QueueResult<()> {
        let [ready, delayed, _] = self.keys(self.queue_name(queue)?);
        let notify = format!("{ready}:notify");
        let marker = self
            .connection
            .key(&format!("queue-chain-delivery:{delivery}"));
        let command = chain_enqueue_command(
            FORWARD_CHAIN_START,
            FORWARD_CHAIN_END,
            [&marker, &ready, &delayed, &notify],
            delivery,
            payload,
            delay,
        )?;
        self.connection
            .query::<u32>(&command)
            .await
            .map_err(backend)?;
        Ok(())
    }

    pub(super) async fn expire_publication(&self, id: &str) {
        // Keep the marker until after SQL acknowledges delivery.
        let _ = self
            .connection
            .query::<bool>(
                ::redis::cmd("EXPIRE")
                    .arg(self.publication_key(id))
                    .arg(86400),
            )
            .await;
    }

    async fn reserve(&self, queue: &str) -> QueueResult<Option<Box<dyn Job>>> {
        let [ready, delayed, reserved] = self.keys(queue);
        let mut command = script(
            RESERVE,
            &[&ready, &delayed, &reserved, &format!("{ready}:notify")],
        );
        command.arg(
            self.retry_after
                .checked_mul(1000)
                .ok_or(QueueError::InvalidConfiguration)?,
        );
        let Some(raw) = self
            .connection
            .query::<Option<String>>(&command)
            .await
            .map_err(backend)?
        else {
            return Ok(None);
        };
        let job: RedisJob = serde_json::from_str(&raw).map_err(|_| QueueError::InvalidPayload)?;
        let payload = Payload::decode(&job.payload)?;
        let storage = RedisReservation {
            connection: self.connection.clone(),
            ready,
            delayed,
            reserved,
            raw,
        };
        Ok(Some(Box::new(ReservedJob {
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
        })))
    }
    async fn pop_waiting(&self, queue: &str) -> QueueResult<Option<Box<dyn Job>>> {
        let first = self.reserve(queue).await?;
        let Some(seconds) = self.block_for.filter(|_| first.is_none()) else {
            return Ok(first);
        };
        let deadline = (seconds != 0)
            .then(|| tokio::time::Instant::now() + Duration::from_secs(u64::from(seconds)));
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
                .min(Duration::from_secs(1));
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
            if let Some(job) = self.reserve(queue).await? {
                return Ok(Some(job));
            }
        }
    }
}

impl Contract for RedisQueue {
    fn get_connection_name(&self) -> &str {
        &self.name
    }

    fn size<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<u64>> {
        Box::pin(async move {
            let [ready, delayed, reserved] = self.keys(self.queue_name(queue)?);
            let command = script(SIZE, &[&ready, &delayed, &reserved]);
            self.connection.query(&command).await.map_err(backend)
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
        Box::pin(async move { self.pop_waiting(self.queue_name(queue)?).await })
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
    ready: String,
    delayed: String,
    reserved: String,
    raw: String,
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
    fn advance_chain<'a>(
        &'a mut self,
        next: &'a Payload,
        queue: Option<&'a str>,
        delay: Option<Delay>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let ready = queue.map_or_else(
                || self.ready.clone(),
                |queue| self.connection.key(&format!("queues:{queue}")),
            );
            let delayed = format!("{ready}:delayed");
            let notify = format!("{ready}:notify");
            let command = chain_enqueue_command(
                ADVANCE_CHAIN_START,
                ADVANCE_CHAIN_END,
                [&self.reserved, &ready, &delayed, &notify],
                &self.raw,
                next,
                delay,
            )?;
            self.run(command).await
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

fn chain_enqueue_command(
    before: &str,
    after: &str,
    keys: [&str; 4],
    reservation: &str,
    payload: &Payload,
    delay: Option<Delay>,
) -> QueueResult<::redis::Cmd> {
    let raw = serde_json::to_string(&RedisJob {
        id: new_id()?,
        payload: payload.encode()?,
        attempts: 0,
    })
    .map_err(|_| QueueError::InvalidPayload)?;
    let (mode, duration) = match delay.unwrap_or(Delay::For(Duration::ZERO)) {
        Delay::For(duration) => ("relative", duration),
        Delay::Until(time) => (
            "absolute",
            time.duration_since(UNIX_EPOCH).unwrap_or_default(),
        ),
    };
    let mut command = script(&format!("{before}{CHAIN_ENQUEUE}{after}"), &keys);
    command
        .arg(reservation)
        .arg(raw)
        .arg(milliseconds(duration)?)
        .arg(mode);
    Ok(command)
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
