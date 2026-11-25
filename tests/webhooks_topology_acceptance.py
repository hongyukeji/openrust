"""用同一个验收应用验证单节点、独立节点目录与共享后端。"""

from concurrent.futures import ThreadPoolExecutor
from contextlib import ExitStack, contextmanager, suppress
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
import secrets
import select
import socket
import socketserver
import subprocess
import tempfile
import threading
import time
import uuid
from urllib.parse import urlsplit


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
ATTACHMENT_SCOPE = "webhook-acceptance"
S3_IMAGE = "chrislusf/seaweedfs@sha256:ccad23e18639c8b98cee17e7100b95830a06077b56d8e5b006d6dcf8ec5e94a0"


class RoundRobin(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        with self.server.mutex:
            active = getattr(self.server, "active", range(len(self.server.ports)))
            node = active[self.server.cursor % len(active)]
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


class BackendGate(socketserver.ThreadingTCPServer):
    """断开应用的现有及新连接，保留隔离后端供验收断言读取。"""

    daemon_threads = True

    def __init__(self, port):
        super().__init__(("127.0.0.1", 0), BackendRelay)
        self.upstream = ("127.0.0.1", port)
        self.mutex = threading.Lock()
        self.sockets = set()
        self.offline = False

    def disconnect(self):
        with self.mutex:
            self.offline = True
            for client in self.sockets:
                with suppress(OSError):
                    client.shutdown(socket.SHUT_RDWR)

    @contextmanager
    def interrupted(self):
        self.disconnect()
        try:
            yield
        finally:
            with self.mutex:
                self.offline = False


class BackendRelay(socketserver.BaseRequestHandler):
    def handle(self):
        gate, client = self.server, self.request
        with gate.mutex:
            if gate.offline:
                return
            gate.sockets.add(client)
        try:
            with socket.create_connection(gate.upstream, timeout=3) as backend:
                backend.settimeout(None)
                peers = {client: backend, backend: client}
                while True:
                    ready, _, _ = select.select(list(peers), [], [])
                    for source in ready:
                        data = source.recv(65536)
                        if not data:
                            return
                        peers[source].sendall(data)
        except OSError:
            return
        finally:
            with gate.mutex:
                gate.sockets.remove(client)


def interruptible_backend(stack, url):
    parsed = urlsplit(url)
    assert parsed.hostname == "127.0.0.1" and parsed.port
    gate = stack.enter_context(BackendGate(parsed.port))
    serve_in_thread(stack, gate)
    stack.callback(gate.disconnect)
    authority, _ = parsed.netloc.rsplit(":", 1)
    return gate, parsed._replace(netloc=f"{authority}:{gate.server_address[1]}").geturl()


def docker_command(*tokens):
    return subprocess.check_output(["docker", *tokens], text=True, timeout=30).strip()


@contextmanager
def redis_backend(enabled):
    if not enabled:
        yield None, None, None
        return
    image = docker_command("image", "inspect", "redis:8", "--format", "{{.Id}}")
    name = f"openrust-webhook-topology-{uuid.uuid4().hex}"
    docker_command("run", "-d", "--rm", "--name", name, "--tmpfs", "/data",
                  "--publish", "127.0.0.1::6379", image,
                  "redis-server", "--save", "", "--appendonly", "no")
    try:
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if docker_command("exec", name, "redis-cli", "ping") == "PONG":
                break
            time.sleep(.1)
        else:
            raise AssertionError("Redis 夹具未就绪")
        port = docker_command("port", name, "6379/tcp").rsplit(":", 1)[1]
        version = docker_command("exec", name, "redis-server", "--version")
        yield name, f"redis://127.0.0.1:{port}", version
    finally:
        docker_command("rm", "-f", name)


@contextmanager
def object_storage(enabled):
    if not enabled:
        yield None, None
        return
    name = f"openrust-webhook-storage-{uuid.uuid4().hex}"
    access, secret = secrets.token_hex(12), secrets.token_hex(20)
    docker_env = os.environ | {"AWS_ACCESS_KEY_ID": access, "AWS_SECRET_ACCESS_KEY": secret}
    image = docker_command("image", "inspect", S3_IMAGE, "--format", "{{.Id}}")
    subprocess.check_output([
        "docker", "run", "-d", "--rm", "--name", name,
        "--tmpfs", "/data", "--publish", "127.0.0.1::8333",
        "--env", "AWS_ACCESS_KEY_ID", "--env", "AWS_SECRET_ACCESS_KEY",
        image, "mini", "-dir=/data",
    ], env=docker_env, timeout=45)
    try:
        port = int(docker_command("port", name, "8333/tcp").rsplit(":", 1)[1])
        endpoint = f"http://127.0.0.1:{port}"
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            probe = http.client.HTTPConnection("127.0.0.1", port, timeout=2)
            try:
                probe.request("GET", "/")
                if probe.getresponse().status == 403:
                    break
            except OSError:
                pass
            finally:
                probe.close()
            time.sleep(.2)
        else:
            raise AssertionError("SeaweedFS 夹具未就绪")

        bucket = "webhook-attachments"

        def request(method, key="", *, content=None, status=200, signed=True, query=""):
            headers = f'user = "{access}:{secret}"\naws-sigv4 = "aws:amz:us-east-1:s3"\n'
            with ExitStack() as stack:
                command = ["curl", "--silent", "--show-error", "--noproxy", "*", "--max-time", "10",
                           "--request", method, "--write-out", "\n%{http_code}"]
                if signed:
                    command += ["--config", "-"]
                if content is not None:
                    data = stack.enter_context(tempfile.NamedTemporaryFile(dir=ROOT / "target/tmp"))
                    data.write(content)
                    data.flush()
                    command += ["--data-binary", f"@{data.name}"]
                address = f"{endpoint}/{bucket}" + (f"/{key}" if key else "")
                if query:
                    address += "?" + query
                result = subprocess.run(command + [address], input=headers.encode() if signed else b"",
                                        capture_output=True, timeout=12, check=True)
                body, code = result.stdout.rsplit(b"\n", 1)
                assert int(code) == status, (method, key, code, body[:300])
                return body

        request("PUT", content=b"")
        request("PUT", query="ownershipControls", status=200, content=(
            b'<OwnershipControls xmlns="http://s3.amazonaws.com/doc/2006-03-01/">'
            b'<Rule><ObjectOwnership>ObjectWriter</ObjectOwnership></Rule></OwnershipControls>'
        ))
        yield {
            "AWS_ACCESS_KEY_ID": access, "AWS_SECRET_ACCESS_KEY": secret, "AWS_SESSION_TOKEN": "",
            "AWS_DEFAULT_REGION": "us-east-1", "AWS_BUCKET": bucket,
            "AWS_ENDPOINT": endpoint, "AWS_USE_PATH_STYLE_ENDPOINT": "true",
            "FIXTURE_ATTACHMENT_DISK": "s3",
        }, request
    finally:
        docker_command("rm", "-f", name)


def exercise(binary, driver, url, redis, redis_url, interruption, storage_env, object_request, ownership):
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
        database_gate = redis_gate = storage_gate = None
        if storage_env:
            storage_gate, endpoint = interruptible_backend(stack, storage_env["AWS_ENDPOINT"])
            common |= storage_env | {"AWS_ENDPOINT": endpoint}
        if interruption:
            database_gate, url = interruptible_backend(stack, url)
            separator = "&" if urlsplit(url).query else "?"
            url += separator + "acquire_timeout=2"
            if redis:
                redis_gate, redis_url = interruptible_backend(stack, redis_url)
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

        def start(node, crash=False, **extra):
            log = stack.enter_context((Path(nodes[node]["APP_BASE_PATH"]) / "http.log").open("a+"))
            env = nodes[node] | extra | ({"FIXTURE_CRASH_AFTER_DURABLE_COMMIT": "1"} if crash else {})
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
                return int(docker_command("exec", redis, "redis-cli", "LLEN",
                                         common["REDIS_PREFIX"] + f"queues:{name}"))
            return int(sql(f"SELECT COUNT(*) FROM jobs WHERE queue='{name}'"))

        def work(node, name, connection=queue, marker=None):
            extra = {} if marker is None else {"WEBHOOK_TEST_RECEIPT_OUTPUT": str(marker)}
            artisan(node, "queue:work", connection, f"--queue={name}", "--once", "--tries=1", **extra)

        def receipt(profile, body=None):
            query = f"SELECT id FROM webhook_calls WHERE name='{profile}'"
            if body is not None:
                query += f" AND raw_body='{base64.b64encode(body).decode()}'"
            identifier = sql(query)
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

        def attach(status=200, body=ATTACHMENT):
            return post("package/attachment-disk", body, status,
                        "multipart/form-data; boundary=disk")[0]

        def object_key(identifier):
            assert re.fullmatch("[0-9a-f-]{36}", identifier), identifier
            scope = hashlib.sha256(ATTACHMENT_SCOPE.encode()).hexdigest()
            return f"webhook-client/{scope}/{identifier}.multipart"

        def stage_cleanup():
            sql("UPDATE webhook_attachment_staging SET created_at=1")
            command_output = artisan(1, "webhook-client:prune", "--attachment-staging",
                                     "--scope=" + ATTACHMENT_SCOPE)
            assert "0 个待重试" in command_output, command_output
            assert sql("SELECT COUNT(*) FROM webhook_attachment_staging") == "0"

        if storage_gate:
            process = start(0, FIXTURE_ATOMIC_CRASH="after-enqueue")
            assert attach(502) == 0 and process.wait(timeout=10) == 78
            assert sql("SELECT COUNT(*) FROM webhook_calls WHERE name='attachment-disk'") == "0"
            assert ready_jobs("database", "webhook-receipts") == 0
            orphan = object_key(sql("SELECT id FROM webhook_attachment_staging"))
            assert object_request("GET", orphan) == ATTACHMENT
            stage_cleanup()
            object_request("GET", orphan, status=404)
            lb.cursor = 0

        first = start(0, crash=True)
        if count == 2:
            start(1)

        assert attach() == 0
        attachment_id = receipt("attachment-disk")
        if count == 2:
            assert attach() == 1
            assert receipt("attachment-disk") == attachment_id
        assert ready_jobs("database", "webhook-receipts") == 1
        if storage_gate:
            key = object_key(attachment_id)
            assert object_request("GET", key) == ATTACHMENT
            object_request("GET", key, status=403, signed=False)
            reference = sql(f"SELECT attachment_reference FROM webhook_calls WHERE id='{attachment_id}'")
            assert json.loads(reference) == {
                "disk": "s3", "length": len(ATTACHMENT), "digest": hashlib.sha256(ATTACHMENT).hexdigest(),
            }
            assert sql(f"SELECT length(raw_body) FROM webhook_calls WHERE id='{attachment_id}'") == "0"

            def no_read():
                result = subprocess.run([str(binary), "artisan", "webhooks:record", attachment_id, ATTACHMENT_SCOPE],
                                        env=nodes[1], capture_output=True, timeout=25)
                assert result.returncode != 0 and b"sample.bin" not in result.stdout

            object_request("PUT", key, content=b"!" + ATTACHMENT[1:])
            try:
                no_read()
            finally:
                object_request("PUT", key, content=ATTACHMENT)
            retry_body = ATTACHMENT.replace(b"stored", b"offline")
            with storage_gate.interrupted():
                no_read()
                failures = parallel(lambda _: attach(500, retry_body))
                assert sorted(failures) == [0, 1], failures
                assert receipt("attachment-disk") == attachment_id
                assert ready_jobs("database", "webhook-receipts") == 1
                assert sql("SELECT COUNT(*) FROM webhook_attachment_staging") == "2"
            stage_cleanup()
        else:
            object_path, = shared_disk.rglob("*.multipart")
            assert object_path.read_bytes() == ATTACHMENT
        attachment_marker = Path(nodes[-1]["APP_BASE_PATH"]) / "attachment.json"
        work(count - 1, "webhook-receipts", "database", attachment_marker)
        record = json.loads(attachment_marker.read_text())
        assert record["id"] == attachment_id and bytes(record["body"]) == ATTACHMENT
        assert bytes(record["attachments"]["files"]["0"]["bytes"]) == b"\x00\xffsample"
        assert ready_jobs("database", "webhook-receipts") == 0
        if storage_gate:
            sql(f"UPDATE webhook_calls SET created_at=1,processed_at=1 WHERE id='{attachment_id}'")
            assert "已清理 1 条" in artisan(0, "webhook-client:prune", "--scope=" + ATTACHMENT_SCOPE)
            assert sql(f"SELECT COUNT(*) FROM webhook_calls WHERE id='{attachment_id}'") == "0"
            assert object_request("GET", key) == ATTACHMENT
            stage_cleanup()
            object_request("GET", key, status=404)
            assert attach(body=retry_body) == 0 and attach(body=retry_body) == 1
            retried = receipt("attachment-disk")
            assert retried != attachment_id and ready_jobs("database", "webhook-receipts") == 1
            assert object_request("GET", object_key(retried)) == retry_body
            retry_marker = attachment_marker.with_name("attachment-retry.json")
            work(1, "webhook-receipts", "database", retry_marker)
            record = json.loads(retry_marker.read_text())
            assert record["id"] == retried and bytes(record["body"]) == retry_body
            assert not list(shared_disk.rglob("*.multipart"))
            print(f"{driver}/{queue}: SeaweedFS 私有对象、跨节点读取、断连与篡改拒绝、崩溃补偿及到期删除通过", flush=True)

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

        def check_ownership():
            if not ownership:
                return
            send_scope = "--scope=" + send["SCOPE"]
            outgoing = ("queue:work", queue, "--queue=webhook-outgoing", "--once")
            incoming = ("queue:work", "database", "--queue=webhook-receipts", "--once")
            first_root = Path(nodes[0]["APP_BASE_PATH"])
            second_root = Path(nodes[1]["APP_BASE_PATH"])

            def identifier(key, mode="normal"):
                _, result = post(f"outbound/{mode}/{key}", send["BODY"], 202)
                value = result["id"]
                assert re.fullmatch("[0-9a-f]{64}", value)
                return value

            def state(value):
                return sql(f"SELECT state,attempts FROM webhook_deliveries WHERE id='{value}'")

            @contextmanager
            def worker(*tokens, **extra):
                process = subprocess.Popen([str(binary), "artisan", *tokens], env=nodes[0] | extra,
                                           stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
                try:
                    yield process
                finally:
                    if process.poll() is None:
                        process.kill()
                        process.wait(timeout=5)
                    process.stdout.close()

            def entered(marker, process):
                deadline = time.monotonic() + 8
                while not marker.exists() and process.poll() is None and time.monotonic() < deadline:
                    time.sleep(.02)
                assert marker.exists() and process.poll() is None, marker.name

            crashed_id = identifier("scheduler-crash")
            scheduled = {"FIXTURE_SEND_SCHEDULER_SINGLE_ITEM": "1"}
            remaining = 60 - time.time() % 60
            if remaining < 9:
                time.sleep(remaining + .15)
            crash = subprocess.run(
                [str(binary), "artisan", "schedule:run"],
                env=nodes[0] | scheduled | {"FIXTURE_CRASH_OUTBOUND_WAKE": "1",
                                           "FIXTURE_SEND_EVENTS": str(first_root / "events.jsonl")},
                capture_output=True, text=True, timeout=20,
            )
            assert crash.returncode == 82, (crash.returncode, crash.stdout, crash.stderr)
            wake_at = int(sql(f"SELECT next_wake_at FROM webhook_deliveries WHERE id='{crashed_id}'"))
            assert 0 < wake_at - time.time() <= 61
            assert state(crashed_id) == "pending\t0" and ready_jobs(queue, "webhook-outgoing") == 0
            assert '"candidates":' not in artisan(1, "schedule:run", **scheduled)
            assert '"candidates":0' in artisan(1, "webhook-server:recover", send_scope, "--pretend")

            artisan(1, "webhooks:fixture-endpoint", "create", "cluster-permit")
            obsolete = identifier("endpoint-late-pause", "endpoint-cluster-permit")
            artisan(1, "webhook-server:send", obsolete, send_scope, "--queue")
            assert ready_jobs(queue, "webhook-outgoing") == 1
            marker = first_root / "endpoint-policy-entered"
            release = first_root / "endpoint-policy-release"
            with worker(*outgoing, FIXTURE_ENDPOINT_WAIT_FOR_PERMISSION="1") as active:
                entered(marker, active)
                assert state(obsolete) == "running\t1"
                replacement = artisan(1, "webhooks:fixture-endpoint", "replace", "cluster-permit", "--rotated")
                assert "revision 2" in replacement
                release.touch()
                output, _ = active.communicate(timeout=12)
                assert active.returncode == 0, output
            assert state(obsolete) == "cancelled\t1" and "endpoint-late-pause" not in receiver.received
            assert ready_jobs(queue, "webhook-outgoing") == 0
            fresh_key = "topology-current-endpoint"
            current = identifier(fresh_key, "endpoint-cluster-permit")
            artisan(0, "webhook-server:send", current, send_scope, "--queue")
            artisan(1, *outgoing)
            assert state(current) == "succeeded\t1"
            (_, headers, body), = receiver.received[fresh_key]
            assert body == send["BODY"]
            assert hmac.compare_digest(headers["Signature"], hmac.new(send["ROTATED"], body, hashlib.sha256).hexdigest())

            missing_key = "topology-missing-endpoint-credential"
            missing = identifier(missing_key, "endpoint-cluster-permit")
            artisan(0, "webhook-server:send", missing, send_scope, "--queue")
            artisan(1, *outgoing, FIXTURE_REPLAY_REVOKE_CREDENTIAL="1")
            assert state(missing) == "failed\t1" and missing_key not in receiver.received
            assert sql("SELECT number,outcome FROM webhook_delivery_attempts "
                       f"WHERE delivery_id='{missing}'") == "1\trejected"
            assert ready_jobs(queue, "webhook-outgoing") == 0

            post("package/leased", b"leased-stale", 200)
            receipt_id = receipt("leased")
            sql("INSERT INTO jobs (queue,payload,attempts,reserved_at,available_at,created_at) "
                "SELECT queue,payload,0,NULL,available_at,created_at FROM jobs WHERE queue='webhook-receipts'")
            old_marker = first_root / "old-owner.json"
            replacement_marker = second_root / "replacement-owner.json"
            gated, old_release = first_root / "old-owner-ready", first_root / "old-owner-release"

            def generations():
                return sql("SELECT generation,outcome FROM webhook_processing_attempts "
                           f"WHERE call_id='{receipt_id}' ORDER BY generation")

            with worker(*incoming, FIXTURE_OLD_ENTERED=str(gated), FIXTURE_OLD_GATE=str(old_release),
                        WEBHOOK_TEST_RECEIPT_OUTPUT=str(old_marker)) as old:
                entered(gated, old)
                assert generations() == "1\trunning"
                # 保持旧处理函数等待，在数据库中模拟其领取过期。
                sql(f"UPDATE webhook_calls SET processing_until=0 WHERE id='{receipt_id}'")
                work(1, "webhook-receipts", "database", replacement_marker)
                assert old.poll() is None and generations() == "1\texpired\n2\tcompleted"
                old_release.touch()
                output, _ = old.communicate(timeout=10)
                assert old.returncode == 0, output
            assert generations() == "1\texpired\n2\tcompleted" and not old_marker.exists()
            assert json.loads(replacement_marker.read_text())["id"] == receipt_id
            record = json.loads(artisan(1, "webhooks:record", receipt_id, ATTACHMENT_SCOPE))
            assert record["exception"] is None, record["exception"]
            remaining_job = sql("SELECT available_at,coalesce(reserved_at,0) FROM jobs "
                                "WHERE queue='webhook-receipts'")
            if remaining_job:
                available_at, reserved_at = map(int, remaining_job.split("\t"))
                wait = max(available_at, reserved_at + 8) - time.time()
                assert wait <= 9
                if wait > 0:
                    time.sleep(wait + .25)
                acknowledged = second_root / "duplicate-ack.json"
                work(1, "webhook-receipts", "database", acknowledged)
                assert not acknowledged.exists() and generations() == "1\texpired\n2\tcompleted"
            assert ready_jobs("database", "webhook-receipts") == 0

            wait = wake_at - time.time()
            if wait > 0:
                time.sleep(wait + .25)
            output = artisan(1, "schedule:run", **scheduled)
            assert '"candidates":1,"queued":1' in output, output
            assert ready_jobs(queue, "webhook-outgoing") == 1
            artisan(1, *outgoing)
            assert state(crashed_id) == "succeeded\t1" and ready_jobs(queue, "webhook-outgoing") == 0
            (_, headers, body), = receiver.received["scheduler-crash"]
            assert body == send["BODY"]
            assert hmac.compare_digest(headers["Signature"], hmac.new(send["SECRET"], body, hashlib.sha256).hexdigest())
            print(f"{driver}/{queue}: Scheduler 退出后跨节点恢复、端点与凭据失效保护、旧领取回归隔离通过", flush=True)

        check_ownership()

        def check_interruption():
            if database_gate is None:
                return

            def failing_command(node, *tokens):
                result = subprocess.run([str(binary), "artisan", *tokens], env=nodes[node],
                                        capture_output=True, text=True, timeout=35)
                assert result.returncode != 0, (tokens, result.stdout, result.stderr)
                return result.stdout

            def pending_delivery(key):
                _, result = post(f"outbound/normal/{key}", send["BODY"], 202)
                identifier = result["id"]
                assert re.fullmatch("[0-9a-f]{64}", identifier)
                return identifier

            def delivery_state(identifier):
                return sql(f"SELECT state,attempts FROM webhook_deliveries WHERE id='{identifier}'")

            def verify_delivered(identifier, key):
                assert delivery_state(identifier) == "succeeded\t1"
                (_, headers, body), = receiver.received[key]
                assert body == send["BODY"] and headers["webhook-id"] == key
                assert hmac.compare_digest(headers["Signature"], hmac.new(send["SECRET"], body, hashlib.sha256).hexdigest())

            database_key = "topology-db-outage"
            database_delivery = pending_delivery(database_key)
            original_calls = sql("SELECT COUNT(*) FROM webhook_calls")
            original_deliveries = sql("SELECT COUNT(*) FROM webhook_deliveries")
            with database_gate.interrupted():
                affected = parallel(lambda _: post("package/default", b"database unavailable", 500)[0])
                assert sorted(affected) == list(range(count)), affected
                post("outbound/normal/topology-outage-new", send["BODY"], 500)
                failing_command(1, "webhook-server:send", database_delivery, "--scope=" + send["SCOPE"])
                assert sql("SELECT COUNT(*) FROM webhook_calls") == original_calls
                assert sql("SELECT COUNT(*) FROM webhook_deliveries") == original_deliveries
                assert ready_jobs("database", "webhook-receipts") == 0
                assert delivery_state(database_delivery) == "pending\t0"
                assert database_key not in receiver.received

            returned = parallel(lambda index: post("package/default", f"restored-{index}".encode(), 200)[0])
            assert sorted(returned) == list(range(count)), returned
            assert ready_jobs("database", "webhook-receipts") == count
            restored_bodies = set()
            for node in range(count):
                marker = attachment_marker.with_name(f"database-restored-{node}.json")
                work(node, "webhook-receipts", "database", marker)
                restored_bodies.add(bytes(json.loads(marker.read_text())["body"]))
            assert restored_bodies == {b"restored-0", b"restored-1"}
            assert ready_jobs("database", "webhook-receipts") == 0
            artisan(0, "webhook-server:send", database_delivery, "--scope=" + send["SCOPE"])
            verify_delivered(database_delivery, database_key)
            print(f"{driver}/{queue}: 主写库失联时两节点拒绝接收和新登记，既有投递保持零尝试，恢复后原 HTTP 进程继续工作", flush=True)

            if redis_gate is None:
                return
            redis_key = "topology-redis-outage"
            redis_delivery = pending_delivery(redis_key)
            receive_sweep = ("webhook-client:recover", "--scope=" + RECOVERY_SCOPE, "--limit=1")
            send_sweep = ("webhook-server:recover", "--scope=" + send["SCOPE"], "--limit=1")
            prior_recovery = int(sql("SELECT COUNT(*) FROM webhook_calls WHERE name='recovery'"))
            with redis_gate.interrupted():
                recovered_nodes = parallel(lambda _: post("package/recovery", b"redis unavailable", 200)[0])
                assert sorted(recovered_nodes) == list(range(count))
                assert int(sql("SELECT COUNT(*) FROM webhook_calls WHERE name='recovery'")) == prior_recovery + 1
                redis_receipt = sql("SELECT call_id FROM webhook_dispatch_intents")
                assert re.fullmatch("[0-9a-f-]{36}", redis_receipt)
                failing_command(0, "schedule:run")
                assert "1 条待重试" in failing_command(1, *receive_sweep)
                output = failing_command(0, *send_sweep)
                assert '"queued":0' in output and '"failed":1' in output, output
                assert ready_jobs("redis", "webhook-recovery") == ready_jobs("redis", "webhook-outgoing") == 0
                assert ready_jobs("database", "webhook-recovery") == ready_jobs("database", "webhook-outgoing") == 0
                assert delivery_state(redis_delivery) == "pending\t0"
                assert redis_key not in receiver.received

            # 保留真实冷却窗口，让两个恢复命令从持久时钟自然取得领取资格。
            ready_at = max(int(sql("SELECT available_at FROM webhook_dispatch_intents")),
                           int(sql(f"SELECT coalesce(next_wake_at,0) FROM webhook_deliveries WHERE id='{redis_delivery}'")))
            wait = ready_at - time.time()
            assert wait <= 61
            if wait > 0:
                time.sleep(wait + .15)
            parallel(lambda node: artisan(node, *receive_sweep))
            parallel(lambda node: artisan(node, *send_sweep))
            assert ready_jobs("redis", "webhook-recovery") == ready_jobs("redis", "webhook-outgoing") == 1
            marker = recovery_marker.with_name("redis-restored.json")
            work(0, "webhook-recovery", marker=marker)
            record = json.loads(marker.read_text())
            assert record["id"] == redis_receipt and bytes(record["body"]) == b"redis unavailable"
            work(1, "webhook-outgoing")
            verify_delivered(redis_delivery, redis_key)
            assert ready_jobs("redis", "webhook-recovery") == ready_jobs("redis", "webhook-outgoing") == 0
            print(f"{driver}/redis: Redis 失联时持久收件、缓存选举拒绝、无数据库回退，还原后各领取一次", flush=True)

        check_interruption()

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
        leased = receipt("leased", b"leased-crash")
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
    interruption = os.environ.get("OPENRUST_TEST_WEBHOOK_TOPOLOGY_OUTAGE") == "1"
    use_storage = os.environ.get("OPENRUST_TEST_WEBHOOK_TOPOLOGY_S3") == "1"
    ownership = os.environ.get("OPENRUST_TEST_WEBHOOK_TOPOLOGY_OWNERSHIP") == "1"
    assert database != "sqlite" or not (use_redis or interruption or use_storage or ownership)
    with redis_backend(use_redis) as (container, endpoint, version), \
            object_storage(use_storage) as (storage_env, object_request):
        if version:
            print(version, flush=True)
        exercise(routes["args"].binary.resolve(), database, database_url, container, endpoint,
                 interruption, storage_env, object_request, ownership)
