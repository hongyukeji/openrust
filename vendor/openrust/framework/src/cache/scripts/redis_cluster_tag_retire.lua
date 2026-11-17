local metadata, pending, index = KEYS[1], KEYS[2], KEYS[3]
local previous, next = ARGV[1], ARGV[2]
if (redis.call('GET', metadata) or '') ~= previous then return false end
if previous ~= '' and redis.call('EXISTS', index) == 1 then
    redis.call('SADD', pending, previous)
end
redis.call('SET', metadata, next)
return redis.call('SMEMBERS', pending)
