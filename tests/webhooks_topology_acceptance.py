"""用同一个验收应用验证单节点、独立节点目录与共享后端。"""

from concurrent.futures import ThreadPoolExecutor
from contextlib import ExitStack, contextmanager
import base64
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
import uuid


ROOT = Path(__file__).resolve().parents[1]
send = runpy.run_path(str(ROOT / "tests/webhooks_send_acceptance.py"))
routes = send["routes"]
helpers = routes["helpers"]
http_helpers = routes["http_helpers"]
RECOVERY_SCOPE = "webhook-recovery-scope"
ATTACHMENT = (
    b'--disk\r\nContent-Disposition: form-data; name="message"\r\n\r\nstored\r\n'
    b'--disk\r\nContent-Disposition: form-data; name="files[]"; filename="sample.bin"\r\n'
    b'Content-Type: application/octet-stream\r\n\r\n\x00\xffsample\r\n--disk--\r\n'
)
BODY = b"shared receipt\x00\xff"


class RoundRobin(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        with self.server.mutex:
            node = self.server.cursor % len(self.server.ports)
            self.server.cursor += 1
        upstream = http.client.HTTPConnection("127.0.0.1", self.server.ports[node], timeout=10)
        try:
            body = self.rfile.read(int(self.headers["Content-Length"]))
            upstream.request("POST", self.path, body, dict(self.headers.items()))
            response = upstream.getresponse()
            status, body = response.status, response.read()
        except (OSError, http.client.RemoteDisconnected):
            status, body = 502, b""
        finally:
            upstream.close()
        self.send_response(status)
        self.send_header("X-Fixture-Node", str(node))
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def start_server(stack, handler):
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    stack.callback(server.server_close)
    return server


def serve_in_thread(stack, server):
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    stack.callback(thread.join, 5)
    stack.callback(server.shutdown)


def reserve_ports(count):
    with ExitStack() as stack:
        sockets = [stack.enter_context(socket.socket()) for _ in range(count)]
        for listener in sockets:
            listener.bind(("127.0.0.1", 0))
        return [listener.getsockname()[1] for listener in sockets]


def redis_command(*tokens):
    return subprocess.check_output(["docker", *tokens], text=True, timeout=30).strip()


@contextmanager
def redis_backend(enabled):
    if not enabled:
        yield None, None, None
        return
    image = redis_command("image", "inspect", "redis:8", "--format", "{{.Id}}")
    name = f"openrust-webhook-topology-{uuid.uuid4().hex}"
    redis_command("run", "-d", "--rm", "--name", name, "--tmpfs", "/data",
                  "--publish", "127.0.0.1::6379", image,
                  "redis-server", "--save", "", "--appendonly", "no")
    try:
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if redis_command("exec", name, "redis-cli", "ping") == "PONG":
                break
            time.sleep(.1)
        else:
            raise AssertionError("Redis 夹具未就绪")
        port = redis_command("port", name, "6379/tcp").rsplit(":", 1)[1]
        version = redis_command("exec", name, "redis-server", "--version")
        yield name, f"redis://127.0.0.1:{port}", version
    finally:
        redis_command("rm", "-f", name)


def exercise(binary, driver, url, redis, redis_url):
    count = 1 if driver == "sqlite" else 2
    queue = "redis" if redis else "database"
    with ExitStack() as stack:
        root = Path(stack.enter_context(tempfile.TemporaryDirectory(
            dir=ROOT / "target/tmp", prefix="webhooks-topology-")))
        if driver == "sqlite":
            os.environ["OPENRUST_SMS_SQLITE_PATH"] = str(root / "database.sqlite")
        sql = lambda statement: helpers["database_rows"](helpers["database_container"](), statement).strip()

        lb = start_server(stack, RoundRobin)
        lb.ports = reserve_ports(count)
        lb.cursor = 0
        lb.mutex = threading.Lock()
        receiver = start_server(stack, send["Receiver"])
        receiver.mutex = threading.Lock()
        receiver.received, receiver.unexpected, receiver.client_certificates = {}, [], {}
        serve_in_thread(stack, receiver)
        shared_disk = root / "shared-private"
        shared_disk.mkdir(mode=0o700)
        common = os.environ | {
            "APP_ENV": "testing", "APP_DEBUG": "false", "APP_HOST": "127.0.0.1",
            "APP_URL": f"http://127.0.0.1:{lb.server_port}", "APP_SUPPORTED_LOCALES": "",
            "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            "DB_CONNECTION": driver, "DB_DATABASE": os.environ.get("OPENRUST_SMS_SQLITE_PATH", "openrust_acceptance"),
            "CACHE_STORE": "redis" if redis else "database", "CACHE_PREFIX": root.name + ":cache:",
            "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
            "QUEUE_CONNECTION": queue, "FIXTURE_RECOVERY_QUEUE": queue, "FIXTURE_SEND_QUEUE": queue,
            "DB_QUEUE_RETRY_AFTER": "8",
            "FILESYSTEM_LOCAL_ROOT": str(shared_disk), "FIXTURE_SPATIE_SECRET": routes["SECRET"],
            "FIXTURE_STANDARD_SECRET": routes["STANDARD"], "FIXTURE_SEND_SCHEDULE": "1",
            "FIXTURE_SEND_SINGLE_SCHEDULER": "1",
            "FIXTURE_SEND_TARGET": f"http://outgoing-fixture.invalid:{receiver.server_port}/receiver",
            "FIXTURE_SEND_ADDRESS": f"127.0.0.1:{receiver.server_port}",
        }
        if url:
            common["DB_URL"] = url
        else:
            common.pop("DB_URL", None)
        if redis:
            common |= {"REDIS_URL": redis_url, "REDIS_PREFIX": root.name + ":", "REDIS_CACHE_DB": "0"}
        nodes = []
        for index, port in enumerate(lb.ports):
            directory = root / f"node-{index}"
            directory.mkdir()
            nodes.append(common | {
                "APP_BASE_PATH": str(directory), "APP_PORT": str(port),
                "APP_CONFIG_CACHE": str(directory / "config.json"),
                "APP_ROUTES_CACHE": str(directory / "routes.json"),
            })

        def artisan(node, *tokens, **extra):
            return routes["command"](binary, nodes[node] | extra, *tokens)

        def start(node, crash=False):
            log = stack.enter_context((Path(nodes[node]["APP_BASE_PATH"]) / "http.log").open("a+"))
            env = nodes[node] | ({"FIXTURE_CRASH_AFTER_DURABLE_COMMIT": "1"} if crash else {})
            process = http_helpers["serve"](binary, env, log)
            stack.callback(http_helpers["stop"], process)
            return process

        def post(route, body, status, content_type="application/octet-stream", extra_headers=None):
            connection = http.client.HTTPConnection("127.0.0.1", lb.server_port, timeout=20)
            signature = hmac.new(routes["SECRET"].encode(), body, hashlib.sha256).hexdigest()
            try:
                connection.request("POST", f"/api/{route}", body, {
                    "Accept": "application/json", "Content-Type": content_type, "Signature": signature,
                } | (extra_headers or {}))
                response = connection.getresponse()
                data = response.read()
                assert response.status == status, (route, status, response.status, data[:280])
                return int(response.getheader("X-Fixture-Node")), json.loads(data) if data else None
            finally:
                connection.close()

        def ready_jobs(connection, name):
            if connection == "redis":
                return int(redis_command("exec", redis, "redis-cli", "LLEN",
                                         common["REDIS_PREFIX"] + f"queues:{name}"))
            return int(sql(f"SELECT COUNT(*) FROM jobs WHERE queue='{name}'"))

        def work(node, name, connection=queue, marker=None):
            extra = {} if marker is None else {"WEBHOOK_TEST_RECEIPT_OUTPUT": str(marker)}
            artisan(node, "queue:work", connection, f"--queue={name}", "--once", "--tries=1", **extra)

        def receipt(profile):
            identifier = sql(f"SELECT id FROM webhook_calls WHERE name='{profile}'")
            assert re.fullmatch("[0-9a-f-]{36}", identifier), identifier
            return identifier

        def parallel(call):
            with ThreadPoolExecutor(max_workers=count) as pool:
                return list(pool.map(call, range(count)))

        artisan(0, "migrate", "--force")
        uncached = start(0)
        serve_in_thread(stack, lb)
        assert post("package/default", BODY, 200)[0] == 0
        plain_marker = Path(nodes[0]["APP_BASE_PATH"]) / "uncached.json"
        work(0, "webhook-receipts", "database", plain_marker)
        assert bytes(json.loads(plain_marker.read_text())["body"]) == BODY
        http_helpers["stop"](uncached)
        lb.cursor = 0
        for node in range(count):
            artisan(node, "config:cache")
            artisan(node, "route:cache")
            assert "webhook-server:recover" in artisan(node, "schedule:list")
        first = start(0, crash=True)
        if count == 2:
            start(1)

        def attach():
            return post("package/attachment-disk", ATTACHMENT, 200,
                        "multipart/form-data; boundary=disk")[0]

        assert attach() == 0
        attachment_id = receipt("attachment-disk")
        if count == 2:
            assert attach() == 1
            assert receipt("attachment-disk") == attachment_id
        assert ready_jobs("database", "webhook-receipts") == 1
        object_path, = shared_disk.rglob("*.multipart")
        assert object_path.read_bytes() == ATTACHMENT
        attachment_marker = Path(nodes[-1]["APP_BASE_PATH"]) / "attachment.json"
        work(count - 1, "webhook-receipts", "database", attachment_marker)
        record = json.loads(attachment_marker.read_text())
        assert record["id"] == attachment_id and bytes(record["body"]) == ATTACHMENT
        assert bytes(record["attachments"]["files"]["0"]["bytes"]) == b"\x00\xffsample"
        assert ready_jobs("database", "webhook-receipts") == 0

        sent_at = str(int(time.time()))
        event = "topology-shared-event"
        event_body = b'{"source":"topology"}'
        secret = base64.b64decode(routes["STANDARD"][6:])
        signature = hmac.new(secret, f"{event}.{sent_at}.".encode() + event_body, hashlib.sha256).digest()
        headers = {"webhook-id": event, "webhook-timestamp": sent_at,
                   "webhook-signature": "v1," + base64.b64encode(signature).decode()}
        event_nodes = parallel(lambda _: post("package/event-dedup", event_body, 200,
                                              "application/json", extra_headers=headers)[0])
        assert sorted(event_nodes) == list(range(count)), event_nodes
        event_id = receipt("event-dedup")
        assert ready_jobs("database", "webhook-receipts") == 1
        event_marker = attachment_marker.with_name("event.json")
        work(count - 1, "webhook-receipts", "database", event_marker)
        record = json.loads(event_marker.read_text())
        assert record["id"] == event_id and bytes(record["body"]) == event_body

        assert post("package/recovery", BODY, 502)[0] == 0
        assert first.wait(timeout=10) == 80
        recovered = receipt("recovery")
        assert ready_jobs(queue, "webhook-recovery") == 0
        if count == 1:
            start(0)
        assert post("package/recovery", BODY, 200)[0] == count - 1
        assert receipt("recovery") == recovered
        if count == 2:
            start(0)
        parallel(lambda node: artisan(node, "webhook-client:recover", "--scope=" + RECOVERY_SCOPE, "--limit=1"))
        assert ready_jobs(queue, "webhook-recovery") == 1
        recovery_marker = Path(nodes[-1]["APP_BASE_PATH"]) / "recovered.json"
        work(count - 1, "webhook-recovery", marker=recovery_marker)
        record = json.loads(recovery_marker.read_text())
        assert record["id"] == recovered and bytes(record["body"]) == BODY
        assert ready_jobs(queue, "webhook-recovery") == 0

        keys = [f"topology-send-{node}" for node in range(count)]
        ids = []
        for node, key in enumerate(keys):
            actual, result = post(f"outbound/normal/{key}", send["BODY"], 202)
            assert actual == node
            assert re.fullmatch("[0-9a-f]{64}", result["id"])
            ids.append(result["id"])
        assert ready_jobs(queue, "webhook-outgoing") == 0
        # 同一 UTC 分钟内竞争调度选举，避开分钟切换。
        remaining = 60 - time.time() % 60
        if remaining < 6:
            time.sleep(remaining + .1)
        scheduler_outputs = parallel(lambda node: artisan(node, "schedule:run"))
        reports = [json.loads(line[line.index("{"):]) for output in scheduler_outputs
                   for line in output.splitlines() if "{" in line]
        assert reports == [{"candidates": count, "queued": count, "retired": 0,
                            "skipped": 0, "failed": 0}], scheduler_outputs
        assert ready_jobs(queue, "webhook-outgoing") == count
        parallel(lambda node: work(node, "webhook-outgoing"))
        assert ready_jobs(queue, "webhook-outgoing") == 0
        for key, identifier in zip(keys, ids):
            assert sql(f"SELECT state,attempts FROM webhook_deliveries WHERE id='{identifier}'") == "succeeded\t1"
            (_, headers, body), = receiver.received[key]
            assert body == send["BODY"]
            assert hmac.compare_digest(headers["Signature"], hmac.new(send["SECRET"], body, hashlib.sha256).hexdigest())

        def deliver(node, identifier, scope=send["SCOPE"]):
            return artisan(node, "webhook-server:send", identifier, "--scope=" + scope,
                           FIXTURE_SEND_LIMITS="1")

        limits = [post(f"outbound/normal/topology-limit-{i}", send["BODY"], 202)[1]["id"] for i in range(3)]
        assert all(re.fullmatch("[0-9a-f]{64}", identifier) for identifier in limits)
        with ThreadPoolExecutor(max_workers=count) as pool:
            tasks = [pool.submit(deliver, index % count, identifier) for index, identifier in enumerate(limits)]
            for task in tasks:
                task.result()
        states = [sql(f"SELECT state,attempts FROM webhook_deliveries WHERE id='{identifier}'")
                  for identifier in limits]
        assert sorted(states) == ["pending\t0", "succeeded\t1", "succeeded\t1"], states
        for i, outcome in enumerate(states):
            assert len(receiver.received.get(f"topology-limit-{i}", [])) == int(outcome[-1])
        _, other = post("outbound/limit-other-scope/topology-other-scope", send["BODY"], 202)
        assert '"succeeded"' in deliver(count - 1, other["id"], "webhook-outbound-limit-alternate")
        assert len(receiver.received["topology-other-scope"]) == 1
        assert not receiver.unexpected

        post("package/leased", b"leased-crash", 200)
        leased = receipt("leased")
        assert ready_jobs("database", "webhook-receipts") == 1
        crashed = subprocess.run(
            [str(binary), "artisan", "queue:work", "database", "--queue=webhook-receipts", "--once"],
            env=nodes[0] | {"FIXTURE_LEASE_CRASH": "1"}, capture_output=True, timeout=20,
        )
        assert crashed.returncode == 79, (crashed.returncode, crashed.stdout, crashed.stderr)

        def attempts():
            return sql("SELECT generation,outcome FROM webhook_processing_attempts "
                       f"WHERE call_id='{leased}' ORDER BY generation")

        assert attempts() == "1\trunning"
        time.sleep(8.2)
        lease_marker = attachment_marker.with_name("lease-recovery.json")
        work(count - 1, "webhook-receipts", "database", lease_marker)
        assert attempts() == "1\texpired\n2\tcompleted"
        assert json.loads(lease_marker.read_text())["id"] == leased
        assert ready_jobs("database", "webhook-receipts") == 0
        print(f"{driver}/{queue}: {count} 个 HTTP 节点，缓存前后 HTTP、独立 Artisan/Worker、共享附件、HTTP 与 Worker 退出后恢复、调度选举、投递及共享限额通过")


if __name__ == "__main__":
    database, database_url = helpers["database_settings"]()
    use_redis = os.environ.get("OPENRUST_TEST_WEBHOOK_TOPOLOGY_REDIS") == "1"
    assert not use_redis or database != "sqlite"
    with redis_backend(use_redis) as (container, endpoint, version):
        if version:
            print(version, flush=True)
        exercise(routes["args"].binary.resolve(), database, database_url, container, endpoint)
