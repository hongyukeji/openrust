"""真实 Webhook HTTP 节点、共享限流、Artisan 和 Worker。"""
import concurrent.futures
from contextlib import ExitStack, closing
import base64
import hashlib
import hmac
import http.client
import json
import os
from pathlib import Path
import runpy
import socket
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
routes = runpy.run_path(str(ROOT / "tests/webhooks_routes_acceptance.py"))
http_helpers = routes["http_helpers"]
helpers = routes["helpers"]
secret = routes["SECRET"]


def submit(env, profile="verified-quota", body=b"signed", valid=True, route="package"):
    signature = hmac.new(secret.encode(), body, hashlib.sha256).hexdigest() if valid else "0" * 64
    with closing(http.client.HTTPConnection("127.0.0.1", int(env["APP_PORT"]), timeout=15)) as client:
        client.request("POST", f"/api/{route}/{profile}", body,
                       {"Accept": "application/json", "Content-Type": "application/octet-stream",
                        "Signature": signature})
        response = client.getresponse()
        assert secret.encode() not in response.read()
        return response.status, response.getheader("Retry-After")


def check_worker_controls(binary, first, second, root, driver, backend, sql, artisan):
    worker = ("queue:work", "database", "--queue=webhook-receipts", "--once")

    def dispatch(env, profile, body):
        assert submit(env, profile, body) == (200, None)
        encoded = base64.b64encode(body).decode()
        return sql(f"SELECT id FROM webhook_calls WHERE name='{profile}' AND raw_body='{encoded}'")

    def inspect(identifier):
        return json.loads(artisan(second, "webhook-client:inspect", identifier, "--scope=webhook-acceptance"))

    def check_released(identifier):
        diagnostic = inspect(identifier)
        assert diagnostic["generation"] == 0 and diagnostic["state"] == "received", diagnostic
        attempts, available = map(int, sql(
            f"SELECT attempts,available_at FROM jobs WHERE payload LIKE '%{identifier}%'"
        ).split())
        assert attempts == 1 and available > int(time.time())

    def wake_job():
        routes["write_database"](driver, sql, "UPDATE jobs SET available_at=0")

    rated_first = dispatch(first, "worker-rate", b"first-rate-slot")
    rated_second = dispatch(second, "worker-rate", b"blocked-while-rate-limited")
    artisan(first, *worker)
    artisan(second, *worker)
    assert inspect(rated_first)["state"] == "completed"
    check_released(rated_second)
    artisan(first, "cache:clear")
    wake_job()
    artisan(second, *worker)
    assert inspect(rated_second)["state"] == "completed"

    held = dispatch(first, "worker-lock", b"hold-worker-lock")
    blocked = dispatch(second, "worker-lock", b"wait-for-shared-worker-lock")
    gate = root / "worker-lock"
    gate.mkdir()
    env = first | {"FIXTURE_QUEUE_LOCK_BARRIER": str(gate)}
    with (gate / "worker.log").open("w+") as log:
        process = subprocess.Popen([str(binary), "artisan", *worker], env=env, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 7
            while not (gate / "entered").exists():
                assert process.poll() is None and time.monotonic() < deadline
                time.sleep(.02)
            assert inspect(held)["state"] == "processing"
            artisan(second, *worker)
            assert process.poll() is None
            check_released(blocked)
        finally:
            (gate / "release").touch()
            try:
                assert process.wait(timeout=10) == 0
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
                raise
    assert inspect(held)["state"] == "completed"
    wake_job()
    artisan(second, *worker)
    assert inspect(blocked)["state"] == "completed"
    assert sql("SELECT COUNT(*) FROM jobs") == sql("SELECT COUNT(*) FROM failed_jobs") == "0"
    print(f"{driver}/{backend}: 原生 Job 限流和共享锁用真实 HTTP 与 6 次 Worker 验证，受控任务未领取接收租约。")


def main():
    driver, url = helpers["database_settings"]()
    binary = routes["args"].binary.resolve()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="webhook-rate-") as tmp, ExitStack() as stack:
        root = Path(tmp)
        if driver == "sqlite":
            os.environ["OPENRUST_SMS_SQLITE_PATH"] = str(root / "database.sqlite")
        shared_cache = {}
        cache_backend = "database"
        if os.environ.get("FIXTURE_RATE_USE_REDIS") == "1":
            topology = runpy.run_path(str(ROOT / "tests/webhooks_topology_acceptance.py"))
            _, redis_url, redis_version = stack.enter_context(topology["redis_backend"](True))
            cache_backend = "redis"
            shared_cache = {"CACHE_STORE": cache_backend, "REDIS_URL": redis_url}
            print(redis_version, flush=True)

        def sql(query):
            return helpers["database_rows"](helpers["database_container"](), query).strip()

        def node(name, revision, scope="webhook-acceptance", **settings):
            folder = root / name
            folder.mkdir()
            with socket.socket() as sock:
                sock.bind(("127.0.0.1", 0))
                port = sock.getsockname()[1]
            env = os.environ | {
                "APP_BASE_PATH": str(folder), "APP_ENV": "testing", "APP_DEBUG": "false",
                "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_URL": f"http://127.0.0.1:{port}",
                "DB_CONNECTION": driver, "DB_DATABASE": os.environ.get("OPENRUST_SMS_SQLITE_PATH", "openrust_acceptance"),
                "CACHE_STORE": "database", "CACHE_PREFIX": "isolated-webhook-verified-rate",
                "SESSION_DRIVER": "array", "QUEUE_CONNECTION": "database", "APP_DISABLED_RUNNERS": "queue,schedule",
                "APP_SUPPORTED_LOCALES": "", "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
                "APP_CONFIG_CACHE": str(folder / "config.json"), "APP_ROUTES_CACHE": str(folder / "routes.json"),
                "FIXTURE_SPATIE_SECRET": secret, "FIXTURE_STANDARD_SECRET": routes["STANDARD"],
                "FIXTURE_VERIFIED_RATE_SCOPE": scope, "FIXTURE_VERIFIED_RATE_REVISION": revision,
            } | shared_cache | settings
            if url:
                env["DB_URL"] = url
            else:
                env.pop("DB_URL", None)
            return env

        def artisan(env, *args):
            return routes["command"](binary, env, *args)

        first = node("first", "one")
        second = node("second", "two")
        other = node("other", "one", "webhook-custom")
        broken = node("broken", "one", CACHE_STORE="database", DB_CACHE_TABLE="missing_webhook_quota_cache")
        artisan(first, "migrate", "--force")
        for env in (second, other, broken):
            artisan(env, "config:cache")
            artisan(env, "route:cache")
        display = artisan(second, "config:show", "webhook-client")
        assert "verified_requests_per_minute" in display and secret not in display
        for env in (first, second, other, broken):
            log = stack.enter_context((Path(env["APP_BASE_PATH"]) / "http.log").open("w+"))
            server = http_helpers["serve"](binary, env, log)
            stack.callback(http_helpers["stop"], server)

        for _ in range(4):
            assert submit(first, valid=False) == (500, None)
            assert submit(second, body=b"ignore-for-quota") == (200, None)
        assert sql("SELECT COUNT(*) FROM webhook_calls") == "0"
        assert submit(broken) == (500, None)
        assert submit(first, route="received") == (500, None)
        assert sql("SELECT COUNT(*) FROM jobs") == "0"

        # 两个真实进程用同一缓存和范围，配置修订不同也共享计数。
        assert submit(first, body=b"initial") == (200, None)
        with concurrent.futures.ThreadPoolExecutor(max_workers=9) as threads:
            results = list(threads.map(
                lambda index: submit(first if index % 2 == 0 else second, body=f"valid-{index}".encode()),
                range(9),
            ))
        statuses = [status for status, _ in results]
        assert statuses.count(200) == 2 and statuses.count(429) == 7, results
        assert all(1 <= int(wait) <= 60 for status, wait in results if status == 429)
        assert sql("SELECT COUNT(*) FROM webhook_calls WHERE name='verified-quota'") == "3"

        assert submit(first, "verified-other", route="received") == (200, None)
        assert submit(second, "verified-other")[0] == 429
        assert submit(other) == (200, None)
        assert sql("SELECT COUNT(*) FROM webhook_calls") == "5"
        assert sql("SELECT COUNT(*) FROM jobs") == "5"

        for row in sql("SELECT id,scope FROM webhook_calls ORDER BY id").splitlines():
            identifier, scope = row.split("\t")
            diagnostic = json.loads(artisan(second, "webhook-client:inspect", identifier, "--scope", scope))
            assert diagnostic["id"] == identifier and diagnostic["state"] == "received"
        for _ in range(5):
            artisan(second, "queue:work", "database", "--queue=webhook-receipts", "--once")
        assert sql("SELECT COUNT(*) FROM jobs") == sql("SELECT COUNT(*) FROM failed_jobs") == "0"
        print(f"{driver}/{cache_backend}: 两接收进程共享 3 个配额，隔离 profile／范围、手动处理器、Retry-After、缓存失败和 5 次独立 Worker 通过")
        check_worker_controls(binary, first, second, root, driver, cache_backend, sql, artisan)


if __name__ == "__main__":
    main()
