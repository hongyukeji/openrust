local registry, index, pending = KEYS[1], KEYS[2], KEYS[3]
local retirement = ARGV[1]
if retirement ~= '' and redis.call('SISMEMBER', pending, retirement) == 0 then
    return 0
end

local kind = redis.call('TYPE', registry).ok
if kind ~= 'none' and kind ~= 'set' then return redis.error_reply('Invalid cache tag registry') end
local index_kind = redis.call('TYPE', index).ok
if index_kind ~= 'none' and index_kind ~= 'zset' then return redis.error_reply('Invalid cache tag index') end
for i = 4, #KEYS do
    local item = KEYS[i]
    if retirement ~= '' and redis.call('ZSCORE', index, item) then redis.call('DEL', item) end
    if redis.call('EXISTS', item) == 0 then redis.call('ZREM', index, item) end
end
if redis.call('ZCARD', index) == 0 then
    redis.call('SREM', registry, index)
    if retirement ~= '' then redis.call('SREM', pending, retirement) end
    return 0
end
return 1
