local function integer_counter(key, command, amount)
    local result = redis.pcall(command, key, amount)
    if type(result) ~= 'table' then return redis.call('GET', key) end
    if result.err ~= 'ERR value is not an integer or out of range' then return result end

    local previous = redis.call('GET', key)
    local integer = previous and string.match(previous, '^n(-?%d+):')
    if not integer then return result end

    redis.call('SET', key, integer, 'KEEPTTL')
    result = redis.pcall(command, key, amount)
    if type(result) == 'table' then
        redis.call('SET', key, previous, 'KEEPTTL')
        return result
    end
    return redis.call('GET', key)
end
