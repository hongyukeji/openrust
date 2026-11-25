"""用隔离 HTTP 接收器实测持久投递、Artisan 和独立 Worker。"""
import concurrent.futures
import email.utils
import hashlib
import hmac
import http.client
import http.server
import json
import os
from pathlib import Path
import re
import runpy
import socket
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
routes = runpy.run_path(str(ROOT / "tests/webhooks_routes_acceptance.py"))
helpers = routes["helpers"]
http_helpers = routes["http_helpers"]
SCOPE = "webhook-outbound-test"
SECRET = b"outbound-isolated-test-key"
BODY = b"\x00sealed\x80\xff"


class Receiver(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        body = self.rfile.read(int(self.headers["Content-Length"]))
        key = self.headers["webhook-id"]
        owner = self.server
        with owner.mutex:
            deliveries = owner.received.setdefault(key, [])
            deliveries.append((self.path, self.headers, body))
            attempt = len(deliveries)
        if key in ("concurrent", "crash", "stale", "timeout") and attempt == 1:
            owner.entered[key].set()
            if not owner.release[key].wait(timeout=12):
                return
        status = 204
        if (key in ("retry", "retry-date") and attempt == 1) or key in ("exhausted", "excluded", "sync-throws", "worker-throws"):
            status = 503
        if key == "redirect":
            status = 302
        self.send_response(status)
        if key == "retry" and attempt == 1:
            self.send_header("Retry-After", "15")
        if key == "retry-date" and attempt == 1:
            self.send_header("Retry-After", email.utils.formatdate(owner.retry_at, usegmt=True))
        if key == "redirect":
            self.send_header("Location", "/unapproved")
        self.send_header("Content-Length", "0")
        try:
            self.end_headers()
        except (BrokenPipeError, ConnectionResetError):
            pass

    def do_GET(self):
        with self.server.mutex:
            self.server.unexpected.append(self.path)
        self.send_error(405)


def exercise(binary, env, log, receiver, driver, sql):
    def command(*tokens, current=env, success=True):
        result = subprocess.run(
            [str(binary), "artisan", *tokens], env=current,
            capture_output=True, text=True, timeout=25,
        )
        assert (result.returncode == 0) == success, result.stdout + result.stderr
        return result.stdout

    def submit(key, mode="normal", body=BODY, expected=202):
        connection = http.client.HTTPConnection("127.0.0.1", int(env["APP_PORT"]), timeout=20)
        try:
            connection.request("POST", f"/api/outbound/{mode}/{key}", body,
                               {"Accept": "application/json"})
            response = connection.getresponse()
            result = response.read()
            assert response.status == expected, (response.status, result)
            if expected in (202, 502, 503):
                identifier = json.loads(result)["id"]
                assert re.fullmatch("[0-9a-f]{64}", identifier)
                return identifier
            if expected == 200:
                assert json.loads(result) == {"skipped": True}
        finally:
            connection.close()

    def send(*tokens, scope=SCOPE, **kwargs):
        return command("webhook-server:send", *tokens, "--scope", scope, **kwargs)

    def enqueue(identifier):
        send(identifier, "--queue")

    def read(identifier):
        return sql(f"SELECT state,attempts FROM webhook_deliveries WHERE id='{identifier}'")

    def attempts(identifier):
        return sql(
            "SELECT number,outcome,coalesce(http_status,0) FROM webhook_delivery_attempts "
            f"WHERE delivery_id='{identifier}' ORDER BY number"
        )

    def update(identifier, values):
        routes["write_database"](
            driver, sql, f"UPDATE webhook_deliveries SET {values} WHERE id='{identifier}'"
        )

    def captured(key, count=1):
        with receiver.mutex:
            hits = list(receiver.received.get(key, []))
        assert len(hits) == count, (key, len(hits), count)
        for path, headers, body in hits:
            assert path == "/receiver"
            assert headers["Host"] == f"outgoing-fixture.invalid:{receiver.server_port}"
            assert headers["webhook-id"] == key
            assert int(headers["Timestamp"]) > 0
            assert body == BODY
            assert hmac.compare_digest(
                headers["Signature"], hmac.new(SECRET, BODY, hashlib.sha256).hexdigest()
            )

    worker = ["queue:work", "database", "--queue=webhook-outgoing", "--once", "--tries=7"]

    rolled_back = submit("rolled-back", "rollback")
    assert read(rolled_back) == ""
    assert '"unavailable"' in send(rolled_back)
    submit("inline", "inline", expected=422)
    assert sql("SELECT COUNT(*) FROM webhook_deliveries") == "0"

    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
        submitted = list(executor.map(submit, ("concurrent", "concurrent")))
    assert submitted[0] == submitted[1]
    concurrent_id = submitted[0]
    submit("concurrent", body=b"different", expected=409)
    assert sql("SELECT COUNT(*) FROM webhook_deliveries") == "1"
    assert '"unavailable"' in send(concurrent_id, scope="wrong-scope")
    captured("concurrent", 0)
    send(concurrent_id, "--queue", "--database=fixture-foreign", success=False)
    assert sql("SELECT COUNT(*) FROM jobs") == "0"
    enqueue(concurrent_id)
    enqueue(concurrent_id)
    first_wire = json.loads(sql("SELECT payload FROM jobs ORDER BY id LIMIT 1"))
    assert first_wire["data"] == concurrent_id
    assert first_wire["options"]["max_tries"] == 1
    assert SECRET.decode() not in sql("SELECT snapshot FROM webhook_deliveries")
    process = subprocess.Popen([str(binary), "artisan", *worker], env=env, stdout=log, stderr=log)
    try:
        assert receiver.entered["concurrent"].wait(timeout=10)
        assert read(concurrent_id) == "running\t1"
        command(*worker)
        captured("concurrent")
        receiver.release["concurrent"].set()
        assert process.wait(timeout=12) == 0
    finally:
        receiver.release["concurrent"].set()
        if process.poll() is None:
            process.kill()
            process.wait(timeout=10)
    assert read(concurrent_id) == "succeeded\t1"
    assert attempts(concurrent_id) == "1\tsucceeded\t204"
    assert sql("SELECT COUNT(*) FROM jobs") == "0"
    assert '"unavailable"' in send(concurrent_id)
    captured("concurrent")

    retry = submit("retry")
    assert '"pending"' in send(retry)
    assert read(retry) == "pending\t1"
    assert int(sql(
        "SELECT d.available_at - a.finished_at "
        "FROM webhook_deliveries d JOIN webhook_delivery_attempts a ON a.delivery_id=d.id "
        f"WHERE d.id='{retry}' AND a.number=1"
    )) >= 15
    assert '"unavailable"' in send(retry)
    update(retry, "available_at=0")
    assert f'{retry} "succeeded"' in send("--limit=1")
    assert attempts(retry) == "1\thttp_failure\t503\n2\tsucceeded\t204"
    captured("retry", 2)

    receiver.retry_at = int(time.time()) + 30
    retry_date = submit("retry-date")
    assert '"pending"' in send(retry_date)
    assert int(sql(f"SELECT available_at FROM webhook_deliveries WHERE id='{retry_date}'")) == receiver.retry_at
    captured("retry-date")

    excluded = submit("excluded", "exclude-503")
    assert '"failed"' in send(excluded)
    assert attempts(excluded) == "1\thttp_failure\t503"
    captured("excluded")

    exhausted = submit("exhausted")
    assert '"pending"' in send(exhausted)
    assert '"failed"' in send(exhausted)
    assert '"unavailable"' in send(exhausted)
    assert attempts(exhausted) == "1\thttp_failure\t503\n2\thttp_failure\t503"
    captured("exhausted", 2)

    revoked = submit("revoked")
    assert '"failed"' in send(revoked, current=env | {"FIXTURE_SEND_REVOKED": "1"})
    assert attempts(revoked) == "1\trejected\t0"
    captured("revoked", 0)

    expired = submit("expired", "expired")
    update(expired, "deadline=0")
    send(expired)
    assert read(expired) == "expired\t0" and not attempts(expired)
    captured("expired", 0)

    redirect = submit("redirect")
    assert '"pending"' in send(redirect)
    assert attempts(redirect) == "1\thttp_failure\t302"
    with receiver.mutex:
        assert not receiver.unexpected
    captured("redirect")

    timeout = submit("timeout", "timeout")
    try:
        assert '"pending"' in send(timeout)
        assert attempts(timeout) == "1\tunknown\t0"
        captured("timeout")
    finally:
        receiver.release["timeout"].set()

    stale = submit("stale")
    process = subprocess.Popen(
        [str(binary), "artisan", "webhook-server:send", stale, "--scope", SCOPE],
        env=env, stdout=log, stderr=log,
    )
    try:
        assert receiver.entered["stale"].wait(timeout=10)
        update(stale, "lease_until=0")
        assert '"succeeded"' in send(stale)
        receiver.release["stale"].set()
        assert process.wait(timeout=10) != 0
        assert attempts(stale) == "1\tunknown\t0\n2\tsucceeded\t204"
        captured("stale", 2)
    finally:
        receiver.release["stale"].set()
        if process.poll() is None:
            process.kill()
            process.wait(timeout=10)

    crash = submit("crash")
    enqueue(crash)
    process = subprocess.Popen([str(binary), "artisan", *worker], env=env, stdout=log, stderr=log)
    try:
        assert receiver.entered["crash"].wait(timeout=10)
        assert attempts(crash) == "1\trunning\t0"
        process.kill()
        process.wait(timeout=10)
        assert '"unavailable"' in send(crash)
        update(crash, "lease_until=0")
        assert '"succeeded"' in send(crash)
        assert attempts(crash) == "1\tunknown\t0\n2\tsucceeded\t204"
        captured("crash", 2)
    finally:
        receiver.release["crash"].set()
        if process.poll() is None:
            process.kill()
            process.wait(timeout=10)


    before = sql("SELECT COUNT(*) FROM webhook_deliveries")
    for mode in ("async-if", "async-unless", "sync-if", "sync-unless"):
        submit(f"skip-{mode}", f"builder-{mode}-skip", expected=200)
        captured(f"skip-{mode}", 0)
    submit("missing-job", "builder-async-missing", expected=422)
    assert sql("SELECT COUNT(*) FROM webhook_deliveries") == before
    assert sql("SELECT COUNT(*) FROM jobs WHERE queue='webhook-outgoing' AND reserved_at IS NULL") == "0"

    for mode in ("async-if", "async-unless"):
        key = f"supported-{mode}"
        identifier = submit(key, f"builder-{mode}")
        assert read(identifier) == "pending\t0"
        captured(key, 0)
        command(*worker)
        assert read(identifier) == "succeeded\t1"
        captured(key)
    for mode in ("sync-if", "sync-unless"):
        key = f"supported-{mode}"
        identifier = submit(key, f"builder-{mode}")
        assert read(identifier) == "succeeded\t1"
        captured(key)

    sync_failure = submit("sync-throws", "builder-sync-throw", expected=502)
    assert read(sync_failure) == "failed\t1"
    captured("sync-throws")
    native_failure = submit("worker-throws", "builder-async-throw")
    command(*worker)
    assert read(native_failure) == "failed\t1"
    assert sql("SELECT COUNT(*) FROM failed_jobs WHERE queue='webhook-outgoing'") == "1"
    captured("worker-throws")

    listener_failure = submit("dispatch-listener-error", "builder-async-if", expected=503)
    assert read(listener_failure) == "pending\t0"
    assert sql("SELECT COUNT(*) FROM jobs WHERE queue='webhook-outgoing' AND reserved_at IS NULL") == "0"
    captured("dispatch-listener-error", 0)
    assert '"succeeded"' in send(listener_failure)
    captured("dispatch-listener-error")

    listener_attempt = submit("attempt-listener-error", "builder-sync-unless")
    assert read(listener_attempt) == "succeeded\t1"
    assert '"unavailable"' in send(listener_attempt)
    captured("attempt-listener-error")

    recorded = Path(env["FIXTURE_SEND_EVENTS"]).read_text()
    assert SECRET.decode() not in recorded and "fixture-sensitive-listener-sentinel" not in recorded
    events = [json.loads(line) for line in recorded.splitlines()]
    def activity(key):
        return [event for event in events if event["message_id"] == key]

    queued = activity("supported-async-if")
    assert [event["kind"] for event in queued] == ["DispatchingWebhookCallEvent", "WebhookCallSucceededEvent"], queued
    assert queued[0]["state"] == "pending" and queued[0]["jobs"] == 0 and queued[0]["attempt"] is None, queued[0]
    assert queued[1]["state"] == "succeeded" and queued[1]["attempt"] == {
        "number": 1, "status": 204, "outcome": "succeeded", "progress": "succeeded",
    }
    for event in queued:
        assert event["id"] and event["scope"] == SCOPE and event["method"] == "POST"
        assert event["body_length"] == len(BODY) and event["url"] == env["FIXTURE_SEND_TARGET"]
        assert set(event["headers"]) == {"content-type", "webhook-id"}, event["headers"]
        assert event["meta"] == {"trace": "fixture"} and event["tags"] == ["audit"]
    assert [event["kind"] for event in activity("supported-sync-if")] == ["WebhookCallSucceededEvent"]
    assert [event["kind"] for event in activity("worker-throws")] == [
        "DispatchingWebhookCallEvent", "WebhookCallFailedEvent", "FinalWebhookCallFailedEvent",
    ]
    retry_events = activity("retry")
    assert [event["kind"] for event in retry_events] == ["WebhookCallFailedEvent", "WebhookCallSucceededEvent"]
    assert [event["attempt"]["number"] for event in retry_events] == [1, 2]
    assert [event["state"] for event in retry_events] == ["pending", "succeeded"]
    assert [event["kind"] for event in activity("stale")] == ["WebhookCallSucceededEvent"]
    assert not [event for event in events if event["message_id"].startswith("skip-")]
    log.flush()
    logs = [Path(log.name), *(Path(env["APP_BASE_PATH"]) / "storage/logs").glob("*.log")]
    for path in logs:
        output = path.read_bytes()
        assert SECRET not in output and b"fixture-sensitive-listener-sentinel" not in output


def main():
    binary = routes["args"].binary.resolve()
    driver, url = helpers["database_settings"]()
    receiver = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Receiver)
    receiver.mutex = threading.Lock()
    receiver.received = {}
    receiver.unexpected = []
    receiver.entered = {name: threading.Event() for name in ("crash", "concurrent", "stale", "timeout")}
    receiver.release = {name: threading.Event() for name in receiver.entered}
    peer = threading.Thread(target=receiver.serve_forever, daemon=True)
    peer.start()
    try:
        with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="webhook-send-") as root:
            with socket.socket() as sock:
                sock.bind(("127.0.0.1", 0))
                app_port = sock.getsockname()[1]
            if driver == "sqlite":
                os.environ["OPENRUST_SMS_SQLITE_PATH"] = str(Path(root) / "database.sqlite")
            env = os.environ | {
                "APP_BASE_PATH": root, "APP_ENV": "testing", "APP_DEBUG": "false",
                "APP_HOST": "127.0.0.1", "APP_PORT": str(app_port),
                "APP_URL": f"http://127.0.0.1:{app_port}", "DB_CONNECTION": driver,
                "DB_DATABASE": os.environ.get("OPENRUST_SMS_SQLITE_PATH", "openrust_acceptance"),
                "CACHE_STORE": "file", "SESSION_DRIVER": "array", "QUEUE_CONNECTION": "database",
                "APP_DISABLED_RUNNERS": "queue,schedule", "APP_SUPPORTED_LOCALES": "",
                "APP_CONFIG_CACHE": str(Path(root) / "config.json"),
                "APP_ROUTES_CACHE": str(Path(root) / "routes.json"),
                "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
                "FIXTURE_SPATIE_SECRET": routes["SECRET"], "FIXTURE_STANDARD_SECRET": routes["STANDARD"],
                "FIXTURE_SEND_TARGET": f"http://outgoing-fixture.invalid:{receiver.server_port}/receiver",
                "FIXTURE_SEND_ADDRESS": f"127.0.0.1:{receiver.server_port}",
                "FIXTURE_SEND_EVENTS": str(Path(root) / "delivery-events.jsonl"),
                "HTTP_PROXY": "http://127.0.0.1:1", "NO_PROXY": "",
            }
            if url:
                env["DB_URL"] = url
            else:
                env.pop("DB_URL", None)

            def sql(query):
                return helpers["database_rows"](helpers["database_container"](), query).strip()

            routes["command"](binary, env, "migrate", "--force")
            routes["command"](binary, env, "config:cache")
            routes["command"](binary, env, "route:cache")
            with (Path(root) / "processes.log").open("w+") as log:
                app = http_helpers["serve"](binary, env, log)
                try:
                    exercise(binary, env, log, receiver, driver, sql)
                except BaseException:
                    log.flush()
                    log.seek(0)
                    print(log.read(), flush=True)
                    raise
                finally:
                    http_helpers["stop"](app)
        print(f"{driver}: HTTP 签名和字节、事务回滚、重复领取、重试预算、撤销、超时、崩溃恢复、Builder 分支与投递事件通过")
    finally:
        receiver.shutdown()
        receiver.server_close()
        peer.join(timeout=5)


if __name__ == "__main__":
    main()
