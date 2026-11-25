"""用两个独立编译的版本、共享数据库和真实队列验证 Webhook 滚动交接。"""

from contextlib import ExitStack
import hashlib
import hmac
import http.client
import json
import os
from pathlib import Path
import re
import runpy
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
# 共享验收入口要求第一个参数为当前制品。旧制品从环境读取。
topology = runpy.run_path(str(ROOT / "tests/webhooks_topology_acceptance.py"))
routes = topology["routes"]
helpers = topology["helpers"]
http_helpers = topology["http_helpers"]
send = topology["send"]
SCOPE = "--scope=" + send["SCOPE"]


def exercise(previous, current, driver, url, redis, redis_url):
    queue = "redis" if redis else "database"
    with ExitStack() as stack:
        root = Path(stack.enter_context(tempfile.TemporaryDirectory(
            dir=ROOT / "target/tmp", prefix="webhooks-rolling-")))
        if driver == "sqlite":
            os.environ["OPENRUST_SMS_SQLITE_PATH"] = str(root / "database.sqlite")

        def sql(statement):
            return helpers["database_rows"](helpers["database_container"](), statement).strip()

        lb = topology["start_server"](stack, topology["RoundRobin"])
        lb.ports = topology["reserve_ports"](2)
        lb.cursor = 0
        lb.mutex = threading.Lock()
        receiver = topology["start_server"](stack, send["Receiver"])
        receiver.mutex = threading.Lock()
        receiver.received, receiver.unexpected, receiver.client_certificates = {}, [], {}
        topology["serve_in_thread"](stack, receiver)
        common = os.environ | {
            "APP_ENV": "testing", "APP_DEBUG": "false", "APP_HOST": "127.0.0.1",
            "APP_URL": f"http://127.0.0.1:{lb.server_port}", "APP_SUPPORTED_LOCALES": "",
            "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            "DB_CONNECTION": driver, "DB_DATABASE": os.environ.get("OPENRUST_SMS_SQLITE_PATH", "openrust_acceptance"),
            "CACHE_STORE": "redis" if redis else "database", "CACHE_PREFIX": root.name + ":cache:",
            "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule", "QUEUE_CONNECTION": queue,
            "FIXTURE_SEND_QUEUE": queue, "FIXTURE_RECOVERY_QUEUE": queue,
            "FIXTURE_SPATIE_SECRET": routes["SECRET"], "FIXTURE_STANDARD_SECRET": routes["STANDARD"],
            "FIXTURE_SEND_TARGET": f"http://outgoing-fixture.invalid:{receiver.server_port}/receiver",
            "FIXTURE_SEND_ADDRESS": f"127.0.0.1:{receiver.server_port}",
        }
        if url:
            common["DB_URL"] = url
        else:
            common.pop("DB_URL", None)
        if redis:
            common |= {"REDIS_URL": redis_url, "REDIS_PREFIX": root.name + ":", "REDIS_CACHE_DB": "0"}

        def environment(edition, node):
            local = root / f"{edition}-{node}"
            local.mkdir()
            return common | {"APP_BASE_PATH": str(local), "APP_PORT": str(lb.ports[node]),
                             "APP_CONFIG_CACHE": str(local / "configuration.json"),
                             "APP_ROUTES_CACHE": str(local / "routes.json")}

        old = [environment("previous", node) for node in range(2)]
        new = [environment("current", node) for node in range(2)]

        def route_to(*active):
            with lb.mutex:
                lb.active = active
                lb.cursor = 0

        def command(binary, env, *tokens):
            return routes["command"](binary, env, *tokens)

        def boot(binary, env):
            command(binary, env, "config:cache")
            command(binary, env, "route:cache")
            log = stack.enter_context((Path(env["APP_BASE_PATH"]) / "http.log").open("w+"))
            app = http_helpers["serve"](binary, env, log)
            stack.callback(http_helpers["stop"], app)
            return app

        def post(node, route, body, status):
            client = http.client.HTTPConnection("127.0.0.1", lb.server_port, timeout=20)
            headers = {
                "Accept": "application/json", "Content-Type": "application/octet-stream",
                "Signature": hmac.new(routes["SECRET"].encode(), body, hashlib.sha256).hexdigest(),
            }
            try:
                client.request("POST", f"/api/{route}", body, headers)
                response = client.getresponse()
                data = response.read()
                assert response.status == status, (route, status, response.status, data[:240])
                assert response.getheader("X-Fixture-Node") == str(node)
                return json.loads(data)
            finally:
                client.close()

        def submit(node, key, body=send["BODY"], status=202):
            reply = post(node, f"outbound/normal/{key}", body, status)
            if status == 202:
                assert re.fullmatch("[0-9a-f]{64}", reply["id"])
                return reply["id"]

        def receive(node, body, profile="leased"):
            assert post(node, "package/" + profile, body, 200) == {"message": "ok"}

        def receipt_work(binary, env, name, body, connection="database"):
            marker = Path(env["APP_BASE_PATH"]) / f"{name}.json"
            incoming_queue = "webhook-receipts" if connection == "database" else "webhook-recovery"
            command(binary, env | {"WEBHOOK_TEST_RECEIPT_OUTPUT": str(marker)},
                    "queue:work", connection, f"--queue={incoming_queue}", "--once")
            record = json.loads(marker.read_text())
            assert bytes(record["body"]) == body and record["exception"] is None
            assert re.fullmatch("[0-9a-f-]{36}", record["id"])
            assert sql(f"SELECT count(processed_at) FROM webhook_calls WHERE id='{record['id']}'") == "1"
            return record["id"]

        def delivery_work(binary, env):
            command(binary, env, "queue:work", queue, "--queue=webhook-outgoing", "--once")

        def queued(binary, env, identifier):
            command(binary, env, "webhook-server:send", identifier, SCOPE, "--queue")

        def history(identifier):
            return sql("SELECT number,outcome,coalesce(http_status,0) FROM webhook_delivery_attempts "
                       f"WHERE delivery_id='{identifier}' ORDER BY number")

        def verify(key, expected_count):
            calls = receiver.received[key]
            assert len(calls) == expected_count
            for path, headers, body in calls:
                assert path == "/receiver" and headers["webhook-id"] == key and body == send["BODY"]
                assert hmac.compare_digest(headers["Signature"], hmac.new(send["SECRET"], body, hashlib.sha256).hexdigest())

        command(previous, old[0], "migrate", "--force")
        schema_check = "SELECT COUNT(*) FROM migrations WHERE migration LIKE '%idempotency_expiry'"
        assert sql(schema_check) == "0"
        original = [boot(previous, env) for env in old]
        topology["serve_in_thread"](stack, lb)
        stable = submit(0, "rolling-stable")
        receive(1, b"old HTTP to current Worker\x00\xff")
        retry = submit(0, "retry")
        command(previous, old[0], "webhook-server:send", retry, SCOPE)
        original_attempt = history(retry)
        assert original_attempt == "1\thttp_failure\t503"
        retry_at, deadline = map(int, sql(
            f"SELECT available_at,deadline FROM webhook_deliveries WHERE id='{retry}'").split("\t"))
        assert 0 < retry_at - time.time() <= 16 and deadline > retry_at
        queued(previous, old[0], stable)

        # 兼容迁移由下一版执行一次，原两个 HTTP 进程保持存活。
        command(current, new[1], "migrate", "--force")
        assert sql(schema_check) == "2"
        assert all(app.poll() is None for app in original)
        assert submit(1, "rolling-stable") == stable
        route_to(0)
        http_helpers["stop"](original[1])
        assert submit(0, "rolling-stable") == stable
        first_upgrade = boot(current, new[1])
        route_to(0, 1)

        # 两个版本共同写入、读取同一个主库，并消费对方版本登记的任务。
        assert submit(0, "rolling-stable") == stable
        assert submit(1, "rolling-stable") == stable
        submit(0, "rolling-stable", b"different body", 409)
        receipt_work(current, new[1], "read-old-receipt", b"old HTTP to current Worker\x00\xff")
        delivery_work(current, new[1])
        assert history(stable) == "1\tsucceeded\t204"
        verify("rolling-stable", 1)
        receive(1, b"current HTTP to old Worker\x00\xff")
        receipt_work(previous, old[0], "read-current-receipt", b"current HTTP to old Worker\x00\xff")
        assert submit(0, "rolling-stable") == stable
        backwards = submit(1, "rolling-current-job")
        assert submit(0, "rolling-current-job") == backwards
        assert submit(1, "rolling-stable") == stable
        queued(current, new[1], backwards)
        delivery_work(previous, old[0])
        assert history(backwards) == "1\tsucceeded\t204"
        verify("rolling-current-job", 1)

        if redis:
            receive(0, b"persisted old route", "recovery")
            command(current, new[1], "webhook-client:recover", "--scope=webhook-recovery-scope")
            receipt_work(current, new[1], "read-old-redis-receipt", b"persisted old route", "redis")
            receive(1, b"persisted current route", "recovery")
            command(current, new[1], "webhook-client:recover", "--scope=webhook-recovery-scope")
            receipt_work(previous, old[0], "read-current-redis-receipt", b"persisted current route", "redis")

        # 切换剩余节点，继续在原投递 ID 上完成旧版留下的退避和尝试历史。
        assert original[0].poll() is None and first_upgrade.poll() is None
        route_to(1)
        http_helpers["stop"](original[0])
        assert submit(1, "rolling-stable") == stable
        boot(current, new[0])
        route_to(0, 1)
        assert submit(0, "rolling-stable") == stable
        assert submit(1, "rolling-current-job") == backwards
        assert history(retry) == original_attempt
        assert sql(f"SELECT attempts,available_at,deadline FROM webhook_deliveries WHERE id='{retry}'") == f"1\t{retry_at}\t{deadline}"
        wait = retry_at - time.time()
        if wait > 0:
            time.sleep(wait + .25)
        command(current, new[0], "webhook-server:recover", SCOPE)
        delivery_work(current, new[1])
        verify("retry", 2)
        assert history(retry) == "1\thttp_failure\t503\n2\tsucceeded\t204"
        for table in ("jobs", "failed_jobs", "webhook_dispatch_intents"):
            assert sql(f"SELECT COUNT(*) FROM {table}") == "0", table
        if redis:
            for name in ("webhook-outgoing", "webhook-recovery"):
                key = common["REDIS_PREFIX"] + "queues:" + name
                for operation, suffix in (("LLEN", ""), ("ZCARD", ":delayed"), ("ZCARD", ":reserved")):
                    assert topology["docker_command"]("exec", redis, "redis-cli", operation, key + suffix) == "0"
        assert not receiver.unexpected
        print(f"{driver}/{queue}: 两制品滚动切换、兼容迁移、双向 Worker 交接、幂等 ID 及退避历史通过", flush=True)


if __name__ == "__main__":
    current = routes["args"].binary.resolve()
    previous = Path(os.environ["OPENRUST_TEST_WEBHOOK_PREVIOUS_BINARY"]).resolve()
    assert previous.is_file() and current.is_file() and not previous.samefile(current)
    with previous.open("rb") as old_file, current.open("rb") as new_file:
        assert hashlib.file_digest(old_file, "sha256").digest() != hashlib.file_digest(new_file, "sha256").digest()
    database, database_url = helpers["database_settings"]()
    use_redis = os.environ.get("OPENRUST_TEST_WEBHOOK_TOPOLOGY_REDIS") == "1"
    assert not use_redis or database != "sqlite"
    with topology["redis_backend"](use_redis) as (container, endpoint, version):
        if version:
            print(version, flush=True)
        exercise(previous, current, database, database_url, container, endpoint)
