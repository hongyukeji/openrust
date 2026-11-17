local count = (#KEYS - 2) / 2
local item, registry = KEYS[1], KEYS[2]
local operation, input, option = ARGV[1], ARGV[2], ARGV[3]
local indexes = {}
for i = 1, count do
    if redis.call('GET', KEYS[2 + i]) ~= ARGV[3 + i] then return false end
    indexes[i] = KEYS[2 + count + i]
end
if operation ~= 'get' then
    local kind = redis.call('TYPE', registry).ok
    if kind ~= 'none' and kind ~= 'set' then return redis.error_reply('Invalid cache tag registry') end
end
local result = tagged_item(operation, item, input, option, indexes)
if type(result) == 'table' and result.err then return result end
if operation ~= 'get' then
    for _, index in ipairs(indexes) do
        if redis.call('EXISTS', index) == 1 then redis.call('SADD', registry, index) end
    end
end
return {result}
