local generations = {}
for i, metadata in ipairs(KEYS) do
    local id = redis.call('GET', metadata)
    if not id then
        id = ARGV[1]
        redis.call('SET', metadata, id)
    end
    if #id ~= 32 or not string.match(id, '^[0-9a-f]+$') then
        return redis.error_reply('Invalid cache tag generation')
    end
    generations[i] = id
end
return generations
