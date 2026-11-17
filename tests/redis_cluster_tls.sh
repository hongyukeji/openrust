#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

name="openrust-redis-tls-$(python3 -c 'import secrets; print(secrets.token_hex(8))')"
image=$(docker image inspect redis:8 --format '{{.Id}}')
fixture=$(mktemp -d)
cleanup() {
    docker stop --time 3 "$name" >/dev/null 2>&1 || true
    if docker inspect "$name" >/dev/null 2>&1; then return 1; fi
    for file in ca.key ca.pem ca.srl server.key server.csr server.pem server.ext untrusted.key untrusted.pem; do
        rm -f "$fixture/$file"
    done
    rmdir "$fixture"
    printf '%s reclaimed\n' "$name"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP

openssl req -x509 -newkey rsa:2048 -nodes -days 2 -subj '/CN=OpenRust Redis test CA' \
    -keyout "$fixture/ca.key" -out "$fixture/ca.pem" 2>/dev/null
openssl req -newkey rsa:2048 -nodes -subj '/CN=localhost' \
    -keyout "$fixture/server.key" -out "$fixture/server.csr" 2>/dev/null
printf 'subjectAltName=IP:127.0.0.1,DNS:localhost\n' > "$fixture/server.ext"
openssl x509 -req -in "$fixture/server.csr" -days 2 -CA "$fixture/ca.pem" -CAkey "$fixture/ca.key" \
    -CAcreateserial -extfile "$fixture/server.ext" -out "$fixture/server.pem" 2>/dev/null
openssl req -x509 -newkey rsa:2048 -nodes -days 2 -subj '/CN=Other Redis test CA' \
    -keyout "$fixture/untrusted.key" -out "$fixture/untrusted.pem" 2>/dev/null

ports=$(python3 - <<'PY'
import random
import socket

available = []
for port in random.sample(range(18000, 27000), 200):
    try:
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", port))
    except OSError:
        continue
    available.append(str(port))
    if len(available) == 3:
        print(" ".join(available))
        break
else:
    raise RuntimeError("could not find three local ports")
PY
)
set -- $ports
first=$1 second=$2 third=$3
cid=$(docker run --detach --rm --name "$name" --tmpfs /data:rw,size=64m \
    --mount "type=bind,source=$fixture,target=/tls,readonly" \
    -p "127.0.0.1:$first:$first" -p "127.0.0.1:$second:$second" -p "127.0.0.1:$third:$third" \
    --entrypoint /bin/sh "$image" -ec '
    for port do
        redis-server --port 0 --tls-port "$port" --tls-cluster yes --tls-auth-clients no \
            --tls-cert-file /tls/server.pem --tls-key-file /tls/server.key --tls-ca-cert-file /tls/ca.pem \
            --bind 0.0.0.0 --protected-mode no --cluster-enabled yes \
            --cluster-config-file "/data/nodes-$port.conf" --cluster-announce-ip 127.0.0.1 \
            --requirepass cluster-fixture --cluster-node-timeout 2000 --save "" --appendonly no &
    done
    wait
' redis-fixture "$first" "$second" "$third")
redis_cli() {
    docker exec -e REDISCLI_AUTH=cluster-fixture "$cid" redis-cli --tls --cacert /tls/ca.pem "$@"
}
for port do
    tries=0
    until test "$(redis_cli -p "$port" ping 2>/dev/null)" = PONG; do
        tries=$((tries+1)); test "$tries" -lt 40; sleep .2
    done
done
redis_cli --cluster create "127.0.0.1:$first" "127.0.0.1:$second" "127.0.0.1:$third" --cluster-yes
docker exec "$cid" redis-server --version

export OPENRUST_TEST_REDIS_TLS_CONTAINER="$cid" OPENRUST_TEST_REDIS_TLS_PORT="$first"
export OPENRUST_TEST_REDIS_TLS_SOURCE="$second" OPENRUST_TEST_REDIS_TLS_DESTINATION="$third"
export OPENRUST_REQUIRE_REDIS_TLS=1
SSL_CERT_FILE="$fixture/untrusted.pem" OPENRUST_TEST_REDIS_TLS_UNTRUSTED=1 \
    cargo +1.94.0 test --test redis --locked --offline redis_cluster_tls_follows_hostless_redirects -- --exact
SSL_CERT_FILE="$fixture/ca.pem" \
    cargo +1.94.0 test --test redis --locked --offline redis_cluster_tls_follows_hostless_redirects -- --exact
