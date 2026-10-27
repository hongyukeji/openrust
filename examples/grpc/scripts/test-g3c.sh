#!/usr/bin/env bash
set -euo pipefail
export NO_PROXY="${NO_PROXY:+$NO_PROXY,}127.0.0.1,localhost"
export no_proxy="$NO_PROXY"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
example_dir="$repo_root/examples/grpc"
compose_file="$example_dir/compose.g3c.yaml"
project="openrust-g3c-${USER:-local}"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/openrust-g3c.XXXXXX")"
probe_pid=""

cleanup() {
  local exit_code=$?
  if [[ -n "$probe_pid" ]]; then kill -TERM "$probe_pid" 2>/dev/null || true; fi
  wait "$probe_pid" 2>/dev/null || true
  if (( exit_code != 0 )); then
    docker compose -p "$project" -f "$compose_file" logs --no-color envoy 2>/dev/null || true
    [[ ! -f "$work_dir/access.log" ]] || cat "$work_dir/access.log"
  fi
  docker compose -p "$project" -f "$compose_file" down --remove-orphans --rmi local >/dev/null 2>&1 || true
  docker image rm openrust-grpc-g3c-node:local >/dev/null 2>&1 || true
  rm -rf "$work_dir"
}
trap cleanup EXIT INT TERM

cd "$repo_root"
cargo build --locked --manifest-path examples/grpc/Cargo.toml --bin g3c-probe
docker compose -p "$project" -f "$compose_file" build g3c-node-a
docker compose -p "$project" -f "$compose_file" up -d
node_a_ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "${project}-g3c-node-a-1")
node_b_ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "${project}-g3c-node-b-1")

wait_for_envoy_hosts() {
  local wanted=$1
  local deadline=$((SECONDS + 45))
  while (( SECONDS < deadline )); do
    if python3 - "$wanted" <<'PY' 2>/dev/null
import json, sys, urllib.request

wanted = int(sys.argv[1])
with urllib.request.urlopen("http://127.0.0.1:9901/clusters?format=json", timeout=1) as response:
    state = json.load(response)
hosts = next(cluster["host_statuses"] for cluster in state["cluster_statuses"] if cluster["name"] == "openrust_grpc")
healthy = sum(
    not host["health_status"].get("failed_active_health_check", False)
    and not host["health_status"].get("pending_dynamic_removal", False)
    for host in hosts
)
raise SystemExit(0 if healthy == wanted else 1)
PY
    then return 0; fi
    sleep 0.25
  done
  curl -fsS 'http://127.0.0.1:9901/clusters?format=json'
  docker compose -p "$project" -f "$compose_file" logs --no-color
  return 1
}

wait_for_node_logs() {
  local deadline=$((SECONDS + 45))
  until docker compose -p "$project" -f "$compose_file" logs --no-color g3c-node-a g3c-node-b | rg -q 'NODE_READY='; do
    if (( SECONDS >= deadline )); then
      docker compose -p "$project" -f "$compose_file" logs --no-color
      return 1
    fi
    sleep 0.2
  done
  for service in g3c-node-a g3c-node-b; do
    docker compose -p "$project" -f "$compose_file" logs --no-color "$service" | rg -q 'NODE_READY='
  done
}

wait_for_node_logs
wait_for_envoy_hosts 2
G3C_RUN_ID=initial "$example_dir/target/debug/g3c-probe" distribution | tee "$work_dir/distribution.log"
sleep 3

"$example_dir/target/debug/g3c-probe" drain >"$work_dir/drain.log" 2>&1 &
probe_pid=$!
deadline=$((SECONDS + 15))
until grep -q STREAM_NODE= "$work_dir/drain.log"; do
  if ! kill -0 "$probe_pid" 2>/dev/null || (( SECONDS >= deadline )); then
    cat "$work_dir/drain.log"
    exit 1
  fi
  sleep 0.1
done
stream_node=$(grep -m1 STREAM_NODE= "$work_dir/drain.log" | cut -d= -f2)
case "$stream_node" in
  A) stopped=g3c-node-a; survivor=B ;;
  B) stopped=g3c-node-b; survivor=A ;;
  *) cat "$work_dir/drain.log"; exit 1 ;;
esac

if [[ "$stopped" == "g3c-node-a" ]]; then stopped_ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "${project}-g3c-node-a-1")
else stopped_ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "${project}-g3c-node-b-1"); fi
docker compose -p "$project" -f "$compose_file" stop --timeout 16 "$stopped" >"$work_dir/stop.log" 2>&1 &
stop_pid=$!
deadline=$((SECONDS + 8))
health_status=""
while (( SECONDS < deadline )); do
  health_status=$(docker compose -p "$project" -f "$compose_file" exec -T "$stopped" g3c-probe health 2>&1) || true
  printf '%s\n' "$health_status" >>"$work_dir/health-probes.log"
  if rg -q 'HEALTH_STATUS=NotServing' <<<"$health_status"; then break; fi
  sleep 0.05
done
if ! rg -q 'HEALTH_STATUS=NotServing' <<<"$health_status"; then
  cat "$work_dir/health-probes.log"
  docker compose -p "$project" -f "$compose_file" logs --no-color "$stopped"
  exit 1
fi
echo "$health_status" | tee "$work_dir/health.log"

deadline=$((SECONDS + 8))
until python3 - "$stopped_ip" <<'PY' 2>/dev/null
import json, sys, urllib.request

ip = sys.argv[1]
with urllib.request.urlopen("http://127.0.0.1:9901/clusters?format=json", timeout=1) as response:
    state = json.load(response)
hosts = next(cluster["host_statuses"] for cluster in state["cluster_statuses"] if cluster["name"] == "openrust_grpc")
unhealthy = any(
    host["address"].get("socket_address", {}).get("address") == ip
    and host["health_status"].get("failed_active_health_check", False)
    for host in hosts
)
raise SystemExit(0 if unhealthy else 1)
PY
do
  if (( SECONDS >= deadline )); then
    cat "$work_dir/stop.log"
    docker compose -p "$project" -f "$compose_file" logs --no-color "$stopped"
    curl -fsS 'http://127.0.0.1:9901/clusters?format=json'
    curl -fsS 'http://127.0.0.1:9901/stats?filter=health_check'
    exit 1
  fi
  sleep 0.2
done
echo "ENVOY_ACTIVE_HEALTH=unhealthy node=$stopped" | tee "$work_dir/health.log"
G3C_EXPECTED_NODE="$survivor" "$example_dir/target/debug/g3c-probe" survivor | tee "$work_dir/drain-route.log"
wait "$stop_pid"
wait_for_envoy_hosts 1
if ! wait "$probe_pid"; then cat "$work_dir/drain.log"; exit 1; fi
probe_pid=""
grep -E 'STREAM_NODE=|STREAM_INTERRUPTED=|RECONNECTED_NODE=' "$work_dir/drain.log"
grep -q STREAM_INTERRUPTED=true "$work_dir/drain.log"
grep -q "RECONNECTED_NODE=$survivor" "$work_dir/drain.log"

docker compose -p "$project" -f "$compose_file" start "$stopped"
wait_for_node_logs
wait_for_envoy_hosts 2
G3C_RUN_ID=rejoined "$example_dir/target/debug/g3c-probe" distribution | tail -1

docker compose -p "$project" -f "$compose_file" logs --no-log-prefix envoy | rg 'initial.*openrust.example.v1.Echo/Unary' >"$work_dir/access.log"
connection_count=$(awk '{print $1}' "$work_dir/access.log" | sort -u | wc -l | tr -d ' ')
[[ "$connection_count" == "1" ]]
upstreams=$(awk '{print $6}' "$work_dir/access.log" | sort -u)
rg -Fq "$node_a_ip:50061" <<<"$upstreams"
rg -Fq "$node_b_ip:50062" <<<"$upstreams"
docker stats --no-stream --format 'Envoy CPU={{.CPUPerc}} memory={{.MemUsage}}' "${project}-envoy-1"
echo "G3C_ACCEPTANCE=passed same_http2_connection_per_rpc_distribution health_drain long_stream_interruption same_channel_reconnect node_rejoin"
