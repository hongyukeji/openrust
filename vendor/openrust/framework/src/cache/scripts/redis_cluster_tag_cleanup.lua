local registry, index, pending = KEYS[1], KEYS[2], KEYS[3]
local retirement, retire = ARGV[1], ARGV[2] == '1'
local pending_kind = redis.call('TYPE', pending).ok
if pending_kind ~= 'none' and pending_kind ~= 'set' then return redis.error_reply('Invalid cache tag retirement set') end
if retire and redis.call('SISMEMBER', pending, retirement) == 0 then
    return 0
end

local kind = redis.call('TYPE', registry).ok
if kind ~= 'none' and kind ~= 'set' then return redis.error_reply('Invalid cache tag registry') end
local index_kind = redis.call('TYPE', index).ok
if index_kind ~= 'none' and index_kind ~= 'zset' then return redis.error_reply('Invalid cache tag index') end
-- Invalid members are passed as data. They may refer to another slot or store.
for i = 3, #ARGV do redis.call('ZREM', index, ARGV[i]) end
for i = 4, #KEYS do
    local item = KEYS[i]
    if retire and redis.call('ZSCORE', index, item) then redis.call('DEL', item) end
    if redis.call('EXISTS', item) == 0 then redis.call('ZREM', index, item) end
end
if redis.call('ZCARD', index) == 0 then
    redis.call('SREM', registry, index)
    redis.call('SREM', pending, retirement)
    return 0
end
return 1
