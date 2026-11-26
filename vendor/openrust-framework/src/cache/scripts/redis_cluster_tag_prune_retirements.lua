local registry, pending = KEYS[1], KEYS[2]
local registry_kind, pending_kind = redis.call('TYPE', registry).ok, redis.call('TYPE', pending).ok
if registry_kind ~= 'none' and registry_kind ~= 'set' then
    return redis.error_reply('Invalid cache tag registry')
end
if pending_kind ~= 'none' and pending_kind ~= 'set' then
    return redis.error_reply('Invalid cache tag retirement set')
end

-- Check every index before changing the registry or pending set.
local empty = {}
for i = 3, #KEYS do
    if redis.call('ZCARD', KEYS[i]) == 0 then empty[#empty + 1] = i end
end
for _, i in ipairs(empty) do
    redis.call('SREM', registry, KEYS[i])
    redis.call('SREM', pending, ARGV[i - 2])
end
