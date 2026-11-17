local index, position, prefix, count = KEYS[1], ARGV[1], ARGV[2], ARGV[3]
local result = redis.call('ZSCAN', index, position, 'COUNT', count)
local now = redis.call('TIME')
local millis = tonumber(now[1]) * 1000 + math.floor(tonumber(now[2]) / 1000)
for i = 1, #result[2], 2 do
    local item = result[2][i]
    local expires = tonumber(result[2][i + 1])
    if (expires and expires >= 0 and expires <= millis)
        or string.sub(item, 1, #prefix) ~= prefix or redis.call('EXISTS', item) == 0 then
        redis.call('ZREM', index, item)
    end
end
return result[1]
