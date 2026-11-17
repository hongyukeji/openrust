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
return tagged_item(operation, item, input, option, indexes)
