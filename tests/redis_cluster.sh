#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

name="openrust-redis-cluster-$(python3 -c 'import secrets; print(secrets.token_hex(8))')"
image=$(docker image inspect redis:8 --format '{{.Id}}')
ports=$(python3 - <<'PY'
import random,socket
picked=[]
for port in random.sample(range(18000,27000), 200):
    try:
        with socket.socket() as sock:
            sock.bind(('127.0.0.1',port))
        picked.append(str(port))
        if len(picked)==6:
            print(' '.join(picked)); break
    except OSError:
        pass
else:
    raise RuntimeError('could not find six local ports')
PY
)
set -- $ports
first=$1 second=$2 third=$3 fourth=$4 fifth=$5 sixth=$6
fixture=$(mktemp -d)
proxy_pid=

cleanup() {
    if test -n "$proxy_pid"; then
        kill "$proxy_pid" 2>/dev/null || true
        wait "$proxy_pid" 2>/dev/null || true
    fi
    docker stop --time 3 "$name" >/dev/null 2>&1 || true
    python3 - "$fixture" <<'PY_CLEAN'
from pathlib import Path
import sys
fixture = Path(sys.argv[1])
for name in ("ready", "acknowledgment-dropped"):
    (fixture / name).unlink(missing_ok=True)
fixture.rmdir()
PY_CLEAN
    if docker inspect "$name" >/dev/null 2>&1; then return 1; fi
    printf '%s reclaimed\n' "$name"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP
cid=$(docker run --detach --rm --name "$name" --tmpfs /data:rw,size=64m \
    -p "127.0.0.1::$first" -p "127.0.0.1:$second:$second" -p "127.0.0.1:$third:$third" \
    -p "127.0.0.1:$fourth:$fourth" -p "127.0.0.1:$fifth:$fifth" -p "127.0.0.1:$sixth:$sixth" \
    --entrypoint /bin/sh "$image" -ec '
    for port do
        redis-server --port "$port" --bind 0.0.0.0 --protected-mode no --cluster-enabled yes \
            --cluster-config-file "/data/nodes-$port.conf" --cluster-announce-ip 127.0.0.1 \
            --requirepass cluster-fixture --masterauth cluster-fixture --cluster-node-timeout 2000 --save "" --appendonly no &
    done
    wait
' redis-fixture "$first" "$second" "$third" "$fourth" "$fifth" "$sixth")
for port do
    tries=0
    until test "$(docker exec -e REDISCLI_AUTH=cluster-fixture "$cid" redis-cli -p "$port" ping 2>/dev/null)" = PONG; do
        tries=$((tries+1)); test "$tries" -lt 40; sleep .2
    done
done
docker exec -e REDISCLI_AUTH=cluster-fixture "$cid" redis-cli --cluster create \
    "127.0.0.1:$first" "127.0.0.1:$second" "127.0.0.1:$third" \
    "127.0.0.1:$fourth" "127.0.0.1:$fifth" "127.0.0.1:$sixth" \
    --cluster-replicas 1 --cluster-yes
backend=$(docker port "$cid" "$first/tcp")
python3 -B tests/redis_cluster_proxy.py "$first" "${backend##*:}" "$fixture" &
proxy_pid=$!
tries=0
until test -f "$fixture/ready"; do
    kill -0 "$proxy_pid"
    tries=$((tries+1)); test "$tries" -lt 40; sleep .1
done
tries=0
until docker exec -e REDISCLI_AUTH=cluster-fixture "$cid" redis-cli -p "$first" cluster info | grep 'cluster_state:ok' >/dev/null; do
    tries=$((tries+1)); test "$tries" -lt 40; sleep .2
done
docker exec "$cid" redis-server --version
export OPENRUST_TEST_REDIS_CLUSTER_PORT="$first" OPENRUST_REQUIRE_REDIS_CLUSTER=1
export OPENRUST_TEST_REDIS_CLUSTER_CONTAINER="$cid" OPENRUST_TEST_REDIS_CLUSTER_FIXTURE="$fixture"
cargo +1.94.0 test --test redis --locked --offline redis_cluster_routes_commands_and_cache_locks_across_shards -- --exact
cargo +1.94.0 test --test redis --locked --offline redis_cluster_migrates_and_recovers_without_replaying_a_write -- --exact
