local base, nonce, operation, key, input, option =
    ARGV[1], ARGV[2], ARGV[3], ARGV[4], ARGV[5], ARGV[6]
local ids = {}
local indexes = {}
for i = 7, #ARGV do
    local tag = ARGV[i]
    local metadata = base .. 'generation:' .. tag
    local generation = redis.call('GET', metadata)
    if not generation then
        generation = nonce
        redis.call('SET', metadata, generation)
    end
    if #generation ~= 32 or not string.match(generation, '^[0-9a-f]+$') then
        return redis.error_reply('Invalid cache tag generation')
    end
    ids[#ids + 1] = tag .. ':' .. generation
    indexes[#indexes + 1] = base .. 'index:' .. tag .. ':' .. generation
end
local namespace = table.concat(ids, '|')
if operation == 'namespace' then return namespace end
local item = base .. 'value:' .. redis.sha1hex(#namespace .. ':' .. namespace .. key)
if operation == 'key' then return item end
if operation == 'get' then return redis.call('GET', item) end

-- Validate indexes before a mutation. Redis scripts do not roll back.
for _, index in ipairs(indexes) do
    local kind = redis.call('TYPE', index).ok
    if kind ~= 'none' and kind ~= 'zset' then
        return redis.error_reply('Invalid cache tag index')
    end
end

local result
if operation == 'pull' or operation == 'forget' then
    if operation == 'pull' then
        result = redis.call('GETDEL', item)
    else
        result = redis.call('DEL', item)
    end
    for _, index in ipairs(indexes) do redis.call('ZREM', index, item) end
    return result
end
if operation == 'put' or operation == 'add' then
    local args = {item, input}
    if option ~= '' then
        args[#args + 1] = 'EX'
        args[#args + 1] = option
    end
    if operation == 'add' then args[#args + 1] = 'NX' end
    if not redis.call('SET', unpack(args)) then return 0 end
    result = 1
elseif operation == 'touch' then
    result = redis.call('EXPIRE', item, input)
    if result == 0 then return 0 end
else
    local command = operation == 'increment' and 'INCRBY' or 'DECRBY'
    result = integer_counter(item, command, input)
    if type(result) == 'table' then return result end
end

local score = redis.call('PTTL', item)
if score >= 0 then
    local now = redis.call('TIME')
    score = tonumber(now[1]) * 1000 + math.floor(tonumber(now[2]) / 1000) + score
end
for _, index in ipairs(indexes) do redis.call('ZADD', index, score, item) end
return result
