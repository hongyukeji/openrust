"""真实 Scheduler、Artisan、HTTP 与独立 Worker 验收审核补派。"""

import argparse
from concurrent.futures import ThreadPoolExecutor
import http.client
import json
import os
from pathlib import Path
import runpy
import socket
import sqlite3
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
DATABASE = runpy.run_path(str(ROOT / "tests/sms_worker.py"))
HTTP = runpy.run_path(str(ROOT / "tests/webhooks_acceptance.py"))
TOPOLOGY = runpy.run_path(str(ROOT / "tests/webhooks_topology_acceptance.py"))
QUEUE = "moderation-recovery"
REVIEW = "drivers:moderation-review"


def exercise(binary, driver, url, root, redis):
    container, redis_url, _ = redis
    backend = "redis" if container else "database"
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    sqlite = root / "recovery.sqlite"
    if driver == "sqlite":
        sqlite.touch()
        os.environ["OPENRUST_SMS_SQLITE_PATH"] = str(sqlite)
    env = os.environ | {
        "APP_BASE_PATH": str(root), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "DB_CONNECTION": driver, "QUEUE_CONNECTION": backend,
        "DB_DATABASE": str(sqlite) if driver == "sqlite" else "openrust_acceptance",
        "MODERATION_CREDENTIAL_DIRECTORY": str(root / "unused-credentials"),
        "MODERATION_FIXTURE_SIGHTENGINE_VIDEO": "http://127.0.0.1:1",
        "MODERATION_FIXTURE_PIN_ENDPOINT": "http://127.0.0.1:1",
        "MODERATION_FIXTURE_QUERY_EXTERNAL": "1", "MODERATION_FIXTURE_RECOVERY_QUEUE": backend,
        "MODERATION_FIXTURE_QUERY_INTERVAL": "20",
    }
    if url:
        env["DB_URL"] = url
    else:
        env.pop("DB_URL", None)
    if redis_url:
        env["REDIS_URL"] = redis_url
        env.pop("REDIS_PASSWORD", None)
        env.pop("REDIS_USERNAME", None)

    def artisan(*tokens, successful=True):
        process = subprocess.run([str(binary), "artisan", *tokens], env=env,
                                 capture_output=True, text=True, timeout=35)
        assert (process.returncode == 0) == successful, process.stdout + process.stderr
        return process.stdout

    def sql(query):
        if driver == "sqlite":
            with sqlite3.connect(sqlite) as connection:
                return "\n".join("\t".join(map(str, row)) for row in connection.execute(query))
        return DATABASE["database_rows"](DATABASE["database_container"](), query).strip()

    def request(intent):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            connection.request("POST", "/api/drivers/moderation/sightengine-video", headers={
                "Accept": "application/json", "X-Fixture-Review-Intent": intent,
                "X-Fixture-Version": "revision-2",
            })
            result = connection.getresponse()
            assert result.status == 200
            return json.loads(result.read())
        finally:
            connection.close()

    def cache_key():
        keys = TOPOLOGY["docker_command"]("exec", container, "redis-cli", "--raw", "--scan",
                                         "--pattern", f"*queues:{QUEUE}").splitlines()
        assert len(keys) <= 1, keys
        return next(iter(keys), None)

    def pending_raw():
        if container:
            key = cache_key()
            output = TOPOLOGY["docker_command"]("exec", container, "redis-cli", "--raw",
                                                "LRANGE", key, "0", "-1") if key else ""
        else:
            output = sql(f"SELECT payload FROM jobs WHERE queue = '{QUEUE}' ORDER BY id")
        return output.splitlines()

    def pending():
        jobs = [json.loads(row) for row in pending_raw()]
        return [json.loads(job["payload"]) for job in jobs] if container else jobs

    def recover(limit=1):
        return json.loads(artisan("moderation:recover", "app", "tenant", f"--database={driver}",
                                  f"--connection={backend}", f"--queue={QUEUE}",
                                  f"--limit={limit}"))["dispatched"]

    def find(review, tenant="tenant"):
        return json.loads(artisan(REVIEW, "find", "app", tenant, review["id"]))

    def consume():
        artisan("queue:work", backend, f"--queue={QUEUE}", "--once")

    artisan("migrate", "--force")
    artisan("config:cache")
    artisan("route:cache")
    artisan("moderation:recover", "app", "tenant", "--limit=0", successful=False)
    with (root / "http.log").open("w+") as log:
        app = HTTP["serve"](binary, env, log)
        try:
            review = request("recover-first")
            deferred = request("recover-later")
        finally:
            HTTP["stop"](app)
    other = json.loads(artisan(REVIEW, "stage", "app", "other", "recover-first", "revision-2"))
    # 控制截止和可执行时间，验证筛选与扫描上限。
    sql(f"UPDATE moderation_reviews SET deadline_at = {review['deadline_at'] + 30} WHERE id = '{deferred['id']}'")
    assert pending() == []
    scheduled = artisan("schedule:run")
    initially_queued = pending()
    assert len(initially_queued) == 1, scheduled
    first, = initially_queued
    assert first["name"] == "moderation.video.recover"
    assert first["data"] == {"application": "app", "tenant": "tenant", "database": driver,
                             "review": review["id"], "version": 1}
    assert "revision-2" not in json.dumps(first) and find(deferred)["state"] == "Pending"
    sql(f"UPDATE moderation_checks SET available_at = {review['deadline_at'] + 60} "
        f"WHERE id = '{deferred['check_id']}'")
    if container:
        TOPOLOGY["docker_command"]("exec", container, "redis-cli", "DEL", cache_key())
    else:
        sql(f"DELETE FROM jobs WHERE queue = '{QUEUE}'")
    assert pending() == [] and recover() == 0
    sql(f"UPDATE moderation_checks SET dispatch_lease_until = 1 WHERE id = '{review['check_id']}'")
    with ThreadPoolExecutor(max_workers=2) as processes:
        workers = [processes.submit(recover) for _ in range(2)]
        counts = [process.result() for process in workers]
    assert sorted(counts) == [0, 1]
    again, = pending()
    assert again["uuid"] != first["uuid"] and again["data"] == first["data"]
    if container:
        TOPOLOGY["docker_command"]("exec", container, "redis-cli", "RPUSH", cache_key(), pending_raw()[0])
    else:
        sql("INSERT INTO jobs(queue, payload, attempts, reserved_at, available_at, created_at) "
            f"SELECT queue, payload, 0, NULL, available_at, created_at FROM jobs WHERE queue = '{QUEUE}'")
    consume()
    submitted = find(review)
    assert submitted["version"] == 3 and submitted["submission"] == {
        "state": "Submitted", "task": "original:revision-2",
    }
    consume()
    assert find(review) == submitted and pending() == []
    assert recover() == 0
    sql(f"UPDATE moderation_checks SET available_at = 1 WHERE id = '{review['check_id']}'")
    artisan("schedule:run")
    resumed, = pending()
    assert resumed["data"]["version"] == 3
    consume()
    checked = find(review)
    assert checked["state"] == "Succeeded" and checked["decision"] == "allow"
    assert checked["query_attempts"] == 1 and checked["version"] == 5
    assert pending() == [] and recover() == 0
    assert find(other, "other")["state"] == "Pending" and find(other) is None
    assert sql("SELECT COUNT(*) FROM failed_jobs") == "0"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    driver, url = DATABASE["database_settings"]()
    use_redis = os.environ.get("MODERATION_TEST_RECOVERY_REDIS") == "1"
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-recovery-") as root, \
            TOPOLOGY["redis_backend"](use_redis) as redis:
        exercise(args.binary.resolve(), driver, url, Path(root), redis)
    print(f"{driver}/{('redis' if use_redis else 'database')}: Scheduler 补派、竞争领取、旧消息和 Worker 通过")
