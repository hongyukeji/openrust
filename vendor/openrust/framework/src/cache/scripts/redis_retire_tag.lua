local generation, pending = KEYS[1], KEYS[2]
local base, next = ARGV[1], ARGV[2]
local previous = redis.call('GET', generation)
if previous and redis.call('EXISTS', base .. previous) == 1 then
    redis.call('SADD', pending, previous)
end
redis.call('SET', generation, next)
return redis.call('SMEMBERS', pending)
