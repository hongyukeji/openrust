local index, position, prefix, count = KEYS[1], ARGV[1], ARGV[2], ARGV[3]
local result = redis.call('ZSCAN', index, position, 'COUNT', count)
for i = 1, #result[2], 2 do
    local item = result[2][i]
    if string.sub(item, 1, #prefix) ~= prefix or redis.call('EXISTS', item) == 0 then
        redis.call('ZREM', index, item)
    end
end
return result[1]
