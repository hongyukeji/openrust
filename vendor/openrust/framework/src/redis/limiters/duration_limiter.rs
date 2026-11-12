use std::sync::Arc;

use crate::{
    cache::{
        CacheError, CacheResult,
        rate_limiting::counters::{Admission, Bucket, Quota, validate},
    },
    redis::connections::Connection,
};

const ADMIT: &str = r#"
for i, key in ipairs(KEYS) do
    local offset = (i - 1) * 3
    local count = tonumber(redis.call('GET', key) or '0')
    local ttl = redis.call('PTTL', key)
    if ttl == -1 then return redis.error_reply('limiter key has no expiration') end
    if count >= tonumber(ARGV[offset + 1]) then
        return {i, count, math.max(0, math.ceil(ttl / 1000))}
    end
end
for i, key in ipairs(KEYS) do
    local offset = (i - 1) * 3
    if ARGV[offset + 3] == '1' then
        if redis.call('EXISTS', key) == 0 then
            redis.call('SET', key, 1, 'PX', tonumber(ARGV[offset + 2]) * 1000)
        else
            redis.call('INCR', key)
        end
    end
end
local result = {0}
for i, key in ipairs(KEYS) do
    result[#result + 1] = tonumber(redis.call('GET', key) or '0')
    result[#result + 1] = math.max(0, math.ceil(redis.call('PTTL', key) / 1000))
end
return result
"#;

/// Applies fixed-duration counters in a single atomic Redis command.
pub struct DurationLimiter {
    connection: Arc<Connection>,
}

impl DurationLimiter {
    pub fn new(connection: Arc<Connection>) -> Self {
        Self { connection }
    }

    pub(crate) async fn clear(&self, key: &str) -> CacheResult<()> {
        self.connection
            .query::<u64>(::redis::cmd("DEL").arg(self.connection.key(key)))
            .await
            .map(|_| ())
            .map_err(|source| CacheError::Backend {
                source: Box::new(source),
            })
    }

    pub(crate) async fn admit(&self, quotas: &[Quota]) -> CacheResult<Admission> {
        validate(quotas)?;
        let mut command = ::redis::cmd("EVAL");
        command.arg(ADMIT).arg(quotas.len());
        for quota in quotas {
            command.arg(self.connection.key(&quota.key));
        }
        for quota in quotas {
            command
                .arg(quota.max)
                .arg(quota.decay)
                .arg(u8::from(quota.charge));
        }
        let result: Vec<u64> =
            self.connection
                .query(&command)
                .await
                .map_err(|source| CacheError::Backend {
                    source: Box::new(source),
                })?;
        match result.split_first() {
            Some((&0, buckets)) if buckets.len() == quotas.len() * 2 => Ok(Admission::Allowed(
                buckets
                    .chunks_exact(2)
                    .map(|pair| Bucket {
                        count: pair[0],
                        remaining_seconds: pair[1],
                    })
                    .collect(),
            )),
            Some((&index, [count, seconds])) if index > 0 && index <= quotas.len() as u64 => {
                Ok(Admission::Denied {
                    index: (index - 1) as usize,
                    bucket: Bucket {
                        count: *count,
                        remaining_seconds: *seconds,
                    },
                })
            }
            _ => Err(CacheError::InvalidValue),
        }
    }
}
