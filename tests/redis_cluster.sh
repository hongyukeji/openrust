#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

name="openrust-redis-cluster-$(python3 -c 'import secrets; print(secrets.token_hex(8))')"
image=$(docker image inspect redis:8 --format '{{.Id}}')
ports=$(python3 - <<'PY'
import random,socket
picked=[]
for port in random.sample(range(18000,30000), 200):
    try:
        with socket.socket() as sock:
            sock.bind(('127.0.0.1',port))
        picked.append(str(port))
        if len(picked)==3:
            print(' '.join(picked)); break
    except OSError:
        pass
else:
    raise RuntimeError('could not find three local ports')
PY
)
set -- $ports
first=$1 second=$2 third=$3
cleanup() {
    docker stop --time 3 "$name" >/dev/null 2>&1 || true
    if docker inspect "$name" >/dev/null 2>&1; then return 1; fi
    printf '%s reclaimed\n' "$name"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP
cid=$(docker run --detach --rm --name "$name" --tmpfs /data:rw,size=64m \
    -p "127.0.0.1:$first:$first" -p "127.0.0.1:$second:$second" -p "127.0.0.1:$third:$third" \
    --entrypoint /bin/sh "$image" -ec '
    for port do
        redis-server --port "$port" --bind 0.0.0.0 --protected-mode no --cluster-enabled yes \
            --cluster-config-file "/data/nodes-$port.conf" --cluster-announce-ip 127.0.0.1 \
            --requirepass cluster-fixture --cluster-node-timeout 2000 --save "" --appendonly no &
    done
    wait
' redis-fixture "$first" "$second" "$third")
for port in "$first" "$second" "$third"; do
    tries=0
    until test "$(docker exec -e REDISCLI_AUTH=cluster-fixture "$cid" redis-cli -p "$port" ping 2>/dev/null)" = PONG; do
        tries=$((tries+1)); test "$tries" -lt 40; sleep .2
    done
done
docker exec -e REDISCLI_AUTH=cluster-fixture "$cid" redis-cli --cluster create "127.0.0.1:$first" "127.0.0.1:$second" "127.0.0.1:$third" --cluster-yes
tries=0
until docker exec -e REDISCLI_AUTH=cluster-fixture "$cid" redis-cli -p "$first" cluster info | grep 'cluster_state:ok' >/dev/null; do
    tries=$((tries+1)); test "$tries" -lt 40; sleep .2
done
docker exec "$cid" redis-server --version
export OPENRUST_TEST_REDIS_CLUSTER_PORT="$first" OPENRUST_REQUIRE_REDIS_CLUSTER=1
cargo +1.94.0 test --test redis --locked --offline redis_cluster_routes_commands_and_cache_locks_across_shards -- --exact
