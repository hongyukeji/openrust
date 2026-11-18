local registry = KEYS[1]
-- Check and unlink atomically so a concurrent write can recreate its index safely.
for i = 2, #KEYS do
    if redis.call('EXISTS', KEYS[i]) == 0 then
        redis.call('SREM', registry, KEYS[i])
    end
end
