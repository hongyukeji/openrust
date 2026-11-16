local pending, index = KEYS[1], KEYS[2]
local generation, prefix, limit = ARGV[1], ARGV[2], tonumber(ARGV[3])
if redis.call('SISMEMBER', pending, generation) == 0 then return 0 end
local batch = redis.call('ZRANGE', index, 0, limit - 1)
for _, item in ipairs(batch) do
    local digest = string.sub(item, #prefix + 1)
    if string.sub(item, 1, #prefix) ~= prefix or #digest ~= 40 or
        not string.match(digest, '^[0-9a-f]+$') then
        return redis.error_reply('Invalid cache tag entry')
    end
end
if #batch > 0 then
    redis.call('DEL', unpack(batch))
    redis.call('ZREM', index, unpack(batch))
end
if #batch < limit then
    redis.call('SREM', pending, generation)
    return 0
end
return 1
