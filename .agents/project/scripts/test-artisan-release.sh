#!/usr/bin/env bash
# 在原生 Ubuntu x86_64 上验收源码打包和发布目录。
set -euo pipefail
umask 077

profile=${1:?指定 root 或 generated}
evidence=${2:?指定验收证据目录}
case "$profile" in root|generated) ;; *) exit 2 ;; esac
# shellcheck source=/dev/null
. /etc/os-release
[[ "$ID" == ubuntu && "$(uname -sm)" == 'Linux x86_64' ]]
case "$(docker info --format '{{.OSType}}/{{.Architecture}}')" in
    linux/x86_64|linux/amd64) ;;
    *) exit 1 ;;
esac
mkdir -p "$evidence"
evidence=$(cd "$evidence" && pwd -P)
repo=$(git rev-parse --show-toplevel)
scratch=$(mktemp -d "${RUNNER_TEMP:-/tmp}/openrust-artisan.XXXXXXXX")
app_pid=
mysql=
redis=
cleanup() {
    if [[ -n "$app_pid" ]] && kill -0 "$app_pid" 2>/dev/null; then
        kill -TERM "$app_pid"
        wait "$app_pid" || true
    fi
    if [[ -n "$mysql" ]]; then docker rm -f "$mysql" >/dev/null; fi
    if [[ -n "$redis" ]]; then docker rm -f "$redis" >/dev/null; fi
}
trap cleanup EXIT
export CARGO_TARGET_DIR="$scratch/target"
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0

{
    printf '%s\n' "$PRETTY_NAME" "$(uname -sm)"
    git -C "$repo" rev-parse HEAD 'HEAD^{tree}'
    rustc --version
} > "$evidence/platform.txt"

source_dir=$repo
name=openrust
if [[ "$profile" == generated ]]; then
    name=artisan-release-mysql
    source_dir="$scratch/$name"
    (
        cd "$repo"
        cargo run --locked -p cli -- new "$source_dir" --db mysql
    ) > "$evidence/generate.log" 2>&1
    (
        cd "$source_dir"
        cargo generate-lockfile
        cargo check --locked
        cargo test --locked
    ) > "$evidence/generated-tests.log" 2>&1
fi

redis=$(docker run -d --rm -p 127.0.0.1::6379 redis:8)
for ((i=0; i<45; i++)); do
    if docker exec "$redis" redis-cli ping 2>/dev/null | grep -qx PONG; then
        break
    fi
    sleep 1
done
docker exec "$redis" redis-cli ping | grep -qx PONG
docker exec "$redis" redis-cli info server | grep '^redis_version:' > "$evidence/redis-version.txt"
redis_port=$(docker port "$redis" 6379/tcp | sed 's/.*://')

package="$scratch/package"
(
    export REDIS_HOST=127.0.0.1 REDIS_PORT="$redis_port"
    cd "$source_dir"
    ./artisan list > "$evidence/source-list.txt"
    grep -Eq '^  build[[:space:]]+构建应用发布目录' "$evidence/source-list.txt"
    ./artisan help build > "$evidence/source-help.txt"
    ./artisan build --help > "$evidence/source-build-help.txt"
    cmp "$evidence/source-help.txt" "$evidence/source-build-help.txt"
    grep -q -- '--platform' "$evidence/source-help.txt"
    ./artisan build --output "$package"
) > "$evidence/build.log" 2>&1

cd "$package"
[[ -x "./$name" && -x ./artisan ]]
[[ -d public && -d resources && -d storage && -d bootstrap/cache ]]
[[ -z "$(ls -A storage)" && -z "$(ls -A bootstrap/cache)" ]]
test ! -e .env
readelf -h "./$name" > "$evidence/elf.txt"
grep -Eq 'Machine:[[:space:]]+Advanced Micro Devices X86-64' "$evidence/elf.txt"
sha256sum "./$name" > "$evidence/binary.sha256"

password=$(openssl rand -hex 24)
root_password=$(openssl rand -hex 24)
if [[ ${GITHUB_ACTIONS:-} == true ]]; then
    printf '::add-mask::%s\n::add-mask::%s\n' "$password" "$root_password"
fi
mysql=$(docker run -d --rm -e MYSQL_ROOT_PASSWORD="$root_password" \
    -e MYSQL_DATABASE=openrust_acceptance -e MYSQL_USER=openrust -e MYSQL_PASSWORD="$password" \
    -p 127.0.0.1::3306 mysql:8.4)
query() {
    docker exec -e MYSQL_PWD="$password" "$mysql" mysql --protocol=TCP \
        -h127.0.0.1 -uopenrust -NB openrust_acceptance -e "$1"
}
for ((i=0; i<90; i++)); do
    if query 'SELECT VERSION()' > "$evidence/mysql-version.txt" 2>/dev/null; then
        break
    fi
    sleep 1
done
grep -Eq '^8\.4\.' "$evidence/mysql-version.txt"
mysql_port=$(docker port "$mysql" 3306/tcp | sed 's/.*://')
http_port=$(python3 - <<'PY'
import socket
with socket.socket() as listener:
    listener.bind(('127.0.0.1', 0))
    print(listener.getsockname()[1])
PY
)
cat > .env <<ENV
APP_ENV=production
APP_DEBUG=false
APP_URL=http://127.0.0.1:$http_port
SERVER_PORT=$http_port
DB_CONNECTION=mysql
DB_HOST=127.0.0.1
DB_PORT=$mysql_port
DB_DATABASE=openrust_acceptance
DB_USERNAME=openrust
DB_PASSWORD=$password
REDIS_HOST=127.0.0.1
REDIS_PORT=$redis_port
CACHE_STORE=redis
QUEUE_CONNECTION=redis
ENV

./"$name" artisan > "$evidence/release-list.txt"
./artisan list > "$evidence/release-wrapper-list.txt"
cmp "$evidence/release-list.txt" "$evidence/release-wrapper-list.txt"
if grep -Eq '^  build[[:space:]]' "$evidence/release-list.txt"; then
    printf '%s\n' '发布命令列表含源码构建入口。' >&2
    exit 1
fi

table_count() {
    query "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema=DATABASE()"
}
before=$(table_count)
if ./"$name" artisan migrate --no-interaction > "$evidence/migrate-refused.log" 2>&1; then
    printf '%s\n' '生产迁移未确认却成功。' >&2
    exit 1
fi
grep -Ei -q 'confirm|production|确认|生产' "$evidence/migrate-refused.log"
after_refusal=$(table_count)
[[ "$before" == 0 && "$after_refusal" == "$before" ]]
./"$name" artisan migrate --force --no-interaction > "$evidence/migrate-force.log" 2>&1
after_force=$(table_count)
[[ "$after_force" -gt 0 ]]
./"$name" artisan migrate status > "$evidence/migrate-status.log" 2>&1
migrations=$(query 'SELECT COUNT(*) FROM migrations')
[[ "$migrations" -gt 0 ]]
./artisan migrate --force --no-interaction > "$evidence/migrate-repeat.log" 2>&1
[[ "$(table_count)" == "$after_force" && "$(query 'SELECT COUNT(*) FROM migrations')" == "$migrations" ]]

./"$name" > "$evidence/server.log" 2>&1 &
app_pid=$!
for ((i=0; i<60; i++)); do
    kill -0 "$app_pid"
    if curl --fail --silent --max-time 2 "http://127.0.0.1:$http_port/up" > "$evidence/up.html"; then
        break
    fi
    sleep 1
done
curl --fail --silent --max-time 3 "http://127.0.0.1:$http_port/up" > "$evidence/up.html"
curl --fail --silent --max-time 3 "http://127.0.0.1:$http_port/api" > "$evidence/api.json"
kill -TERM "$app_pid"
wait "$app_pid"
app_pid=
if curl --silent --fail --max-time 2 "http://127.0.0.1:$http_port/up" >/dev/null; then
    printf '%s\n' '应用退出后端口仍可访问。' >&2
    exit 1
fi
printf '{"profile":"%s","tables_before":%s,"tables_after_refusal":%s,"tables_after_force":%s,"migration_rows":%s,"http_up":200,"http_api":200,"sigterm_exit":0}\n' \
    "$profile" "$before" "$after_refusal" "$after_force" "$migrations" > "$evidence/result.json"
