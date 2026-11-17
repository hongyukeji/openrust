local metadata, pending, index = KEYS[1], KEYS[2], KEYS[3]
local previous, next = ARGV[1], ARGV[2]
if (redis.call('GET', metadata) or '') ~= previous then return false end
local pending_kind = redis.call('TYPE', pending).ok
if pending_kind ~= 'none' and pending_kind ~= 'set' then
    return redis.error_reply('Invalid cache tag retirement set')
end
if previous ~= '' and redis.call('EXISTS', index) == 1 then
    redis.call('SADD', pending, previous)
end
redis.call('SET', metadata, next)
local retired = {}
for _, id in ipairs(redis.call('SMEMBERS', pending)) do
    if #id == 32 and string.match(id, '^[0-9a-f]+$') then
        retired[#retired + 1] = id
    else
        redis.call('SREM', pending, id)
    end
end
return retired
