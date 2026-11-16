local members, leases = KEYS[1], KEYS[2]
local operation, topic, app, channel, socket = unpack(ARGV, 1, 5)
local clock = redis.call('TIME')
local now = tonumber(clock[1]) * 1000 + math.floor(tonumber(clock[2]) / 1000)
local lease = tonumber(ARGV[7])

local function send(event, data, recipient, excluded)
    redis.call('PUBLISH', topic, cjson.encode({
        app_id = app, recipient = recipient, excluded = excluded,
        frame = { event = 'pusher_internal:' .. event, data = data, channel = channel }
    }))
end

local function remove(connection, departed)
    local member = redis.call('HGET', members, connection)
    if member then
        departed[cjson.decode(member)[1]] = true
        redis.call('HDEL', members, connection)
    end
    redis.call('ZREM', leases, connection)
end

local departed = {}
for _, connection in ipairs(redis.call('ZRANGEBYSCORE', leases, '-inf', now)) do
    remove(connection, departed)
end
if operation == 'leave' then
    remove(socket, departed)
end

local stale = {}
if operation == 'refresh' then
    for index = 8, #ARGV do
        local connection = ARGV[index]
        if redis.call('HEXISTS', members, connection) == 1 then
            redis.call('ZADD', leases, 'XX', now + lease, connection)
        else
            table.insert(stale, connection)
        end
    end
end

if operation == 'join' or next(departed) then
    -- Choose a deterministic representative when a user has multiple sockets.
    -- Keep user_info as raw JSON to preserve empty arrays and large integers.
    local by_user, representative = {}, {}
    local current = redis.call('HGETALL', members)
    for index = 1, #current, 2 do
        local connection, member = current[index], cjson.decode(current[index + 1])
        local user, info = member[1], member[2]
        if not representative[user] or connection < representative[user] then
            representative[user] = connection
            by_user[user] = info
        end
    end
    for user, _ in pairs(departed) do
        if not representative[user] then
            send('member_removed', cjson.encode({ user_id = user }))
        end
    end

    if operation == 'join' then
        local member = cjson.decode(ARGV[6])
        local user, info = member[1], member[2]
        local joined = not representative[user]
        redis.call('HSET', members, socket, ARGV[6])
        redis.call('ZADD', leases, now + lease, socket)
        if joined or socket <= representative[user] then
            by_user[user] = info
        end
        local ids, hash = {}, {}
        for id, _ in pairs(by_user) do
            table.insert(ids, id)
        end
        table.sort(ids)
        for _, id in ipairs(ids) do
            table.insert(hash, cjson.encode(id) .. ':' .. by_user[id])
        end
        local snapshot = string.format('{"presence":{"count":%d,"ids":%s,"hash":{%s}}}',
            #ids, cjson.encode(ids), table.concat(hash, ','))
        -- Acknowledge via Pub/Sub so the snapshot and all subsequent updates
        -- travel in the same Redis order.
        send('subscription_succeeded', snapshot, socket)
        if joined then
            send('member_added', '{"user_id":' .. cjson.encode(user) .. ',"user_info":' .. info .. '}', nil, socket)
        end
    end
end

-- Channels with no living node need no external garbage collector.
redis.call('PEXPIRE', members, lease * 2)
redis.call('PEXPIRE', leases, lease * 2)
return stale
