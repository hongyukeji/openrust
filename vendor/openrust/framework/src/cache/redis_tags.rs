//! Redis 标签值的到期索引和可恢复清理。

use redis::aio::ConnectionManager;
use std::time::Duration;

use super::{REDIS_TAG_EPOCH_KEY, Result, backend_failure, redis_tag_seed};

const BATCH_SIZE: usize = 128;

// 使用写入时捕获的代际登记索引，避免迟到的写入进入当前代际。
const WRITE: &str = "
    local epoch = redis.call('GET', KEYS[2])
    if not epoch then return 0 end
    local n = (#KEYS - 2) / 2
    for i = 1, n do
        if (redis.call('GET', KEYS[i + 2]) or epoch) ~= ARGV[i] then return 0 end
        local kind = redis.call('TYPE', KEYS[i + n + 2]).ok
        if kind ~= 'none' and kind ~= 'zset' then
            return redis.error_reply('invalid cache tag index')
        end
    end
    local clock = redis.call('TIME')
    local now = tonumber(clock[1]) * 1000 + math.floor(tonumber(clock[2]) / 1000)
    local expiry = now + tonumber(ARGV[n + 2])
    if expiry > 9007199254740991 then
        return redis.error_reply('cache tag expiry outside exact millisecond range')
    end
    redis.call('SET', KEYS[1], ARGV[n + 1], 'PXAT', string.format('%.0f', expiry))
    for i = n + 3, #KEYS do
        local index = KEYS[i]
        redis.call('ZREMRANGEBYSCORE', index, '-inf', now)
        redis.call('ZADD', index, expiry, KEYS[1])
        local last = redis.call('ZRANGE', index, -1, -1, 'WITHSCORES')
        redis.call('PEXPIREAT', index, string.format('%.0f', tonumber(last[2])))
    end
    return 1";

// 待清理集合的成员按十进制代际排序，避免 Lua 浮点数损失 64 位代际精度。
const RETIRE: &str = "
    local epoch = redis.call('GET', KEYS[2]) or ARGV[1]
    local current = redis.call('GET', KEYS[1])
    local generation = current or epoch
    if not generation:match('^[1-9]%d*$') or #generation > 19 or
       (#generation == 19 and generation >= '9223372036854775807') then
        return redis.error_reply('invalid cache tag generation')
    end
    local retired = string.rep('0', 19 - #generation) .. generation
    local index = ARGV[2] .. retired
    local kind = redis.call('TYPE', index).ok
    local ttl = redis.call('PTTL', index)
    local pending_ttl = redis.call('PTTL', KEYS[3])
    if (kind ~= 'none' and kind ~= 'zset') or ttl == -1 or pending_ttl == -1 or
       (pending_ttl >= 0 and redis.call('TYPE', KEYS[3]).ok ~= 'zset') then
        return redis.error_reply('invalid cache tag retirement state')
    end
    redis.call('SET', KEYS[2], epoch, 'NX')
    if not current then redis.call('SET', KEYS[1], epoch) end
    redis.call('INCR', KEYS[1])
    if ttl >= 0 then
        redis.call('ZADD', KEYS[3], 0, retired)
        redis.call('PEXPIRE', KEYS[3], string.format('%.0f', math.max(1, ttl, pending_ttl)))
    end
    return { epoch, retired }";

// 清理只到本次退役的代际。其他调用可以继续写入新代际或独立清理。
const DRAIN: &str = "
    if redis.call('GET', KEYS[1]) ~= ARGV[1] then return 0 end
    local retired = redis.call('ZRANGEBYLEX', KEYS[2], '-', '[' .. ARGV[2], 'LIMIT', 0, 1)[1]
    if not retired then return 0 end
    if #retired ~= 19 or not retired:match('^%d+$') then
        return redis.error_reply('invalid retired cache tag generation')
    end
    local index = ARGV[3] .. retired
    local values = redis.call('ZRANGE', index, 0, tonumber(ARGV[4]) - 1)
    for _, value in ipairs(values) do
        if #value ~= 79 or not value:match('^__cache_tag_v2:[0-9a-f]+$') then
            return redis.error_reply('invalid indexed cache tag value')
        end
    end
    if #values > 0 then
        redis.call('DEL', unpack(values))
        redis.call('ZREM', index, unpack(values))
    end
    if redis.call('ZCARD', index) == 0 then redis.call('ZREM', KEYS[2], retired) end
    return 1";

pub(super) async fn write(
    connection: &mut ConnectionManager,
    key: &str,
    value: Vec<u8>,
    ttl: Duration,
    generations: &[(String, u64)],
) -> Result<bool> {
    let mut command = redis::cmd("EVAL");
    command
        .arg(WRITE)
        .arg(2 + 2 * generations.len())
        .arg(key)
        .arg(REDIS_TAG_EPOCH_KEY);
    for (tag, _) in generations {
        command.arg(tag);
    }
    for (tag, generation) in generations {
        command.arg(format!("{}{generation:019}", index_prefix(tag)));
    }
    for (_, generation) in generations {
        command.arg(generation.to_string());
    }
    command.arg(value).arg(ttl.as_millis().to_string());
    let written: i32 = command
        .query_async(connection)
        .await
        .map_err(|_| backend_failure())?;
    Ok(written == 1)
}

pub(super) async fn invalidate(connection: &mut ConnectionManager, tag_key: &str) -> Result<()> {
    let prefix = index_prefix(tag_key);
    let pending = format!("{tag_key}:retired");
    let (epoch, cutoff): (String, String) = redis::cmd("EVAL")
        .arg(RETIRE)
        .arg(3)
        .arg(tag_key)
        .arg(REDIS_TAG_EPOCH_KEY)
        .arg(&pending)
        .arg(redis_tag_seed()?)
        .arg(&prefix)
        .query_async(connection)
        .await
        .map_err(|_| backend_failure())?;

    loop {
        let progressed: i32 = redis::cmd("EVAL")
            .arg(DRAIN)
            .arg(2)
            .arg(REDIS_TAG_EPOCH_KEY)
            .arg(&pending)
            .arg(&epoch)
            .arg(&cutoff)
            .arg(&prefix)
            .arg(BATCH_SIZE)
            .query_async(connection)
            .await
            .map_err(|_| backend_failure())?;
        if progressed == 0 {
            return Ok(());
        }
    }
}

fn index_prefix(tag_key: &str) -> String {
    format!("{tag_key}:index:")
}
