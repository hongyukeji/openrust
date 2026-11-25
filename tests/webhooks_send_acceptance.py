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
import select
import socket
import socketserver
import ssl
import subprocess
import tempfile
import threading
import time
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]
routes = runpy.run_path(str(ROOT / "tests/webhooks_routes_acceptance.py"))
helpers = routes["helpers"]
http_helpers = routes["http_helpers"]
SCOPE = "webhook-outbound-test"
SECRET = b"outbound-isolated-test-key"
ROTATED = b"outbound-isolated-rotated-key"
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
            path_attempt = 1 + sum(path == self.path for path, _, _ in deliveries)
            deliveries.append((self.path, self.headers, body))
            if isinstance(self.connection, ssl.SSLSocket):
                owner.client_certificates[key] = self.connection.getpeercert(binary_form=True)
            attempt = len(deliveries)
        if key in ("concurrent", "crash", "stale", "timeout", "endpoint-inflight") and attempt == 1:
            owner.entered[key].set()
            if not owner.release[key].wait(timeout=12):
                return
        status = 204
        if (key in ("retry", "retry-date") and attempt == 1) or key in ("exhausted", "excluded", "sync-throws", "worker-throws"):
            status = 503
        if key.startswith("custom-stop") or key == "custom-continue" and attempt == 1:
            status = 503
        if (key.startswith("replay-failed-") or key == "endpoint-replay-source") and attempt == 1:
            status = 503
        if self.path == "/receiver?endpoint=backup" and path_attempt == 1 and b'"status":"retry"' in body:
            status = 503
        if self.path == "/receiver?endpoint=backup" and path_attempt <= 3 and b'"status":"fail-original"' in body:
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
        response_body = b"fixture-remote-response" if key.startswith("custom-") and status == 503 else b""
        self.send_header("Content-Length", str(len(response_body)))
        try:
            self.end_headers()
            if response_body:
                self.wfile.write(response_body)
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

    def captured(key, count=1, rotated=()):
        with receiver.mutex:
            hits = list(receiver.received.get(key, []))
        assert len(hits) == count, (key, len(hits), count)
        for index, (path, headers, body) in enumerate(hits):
            assert path == "/receiver"
            assert headers["Host"] == urlsplit(env["FIXTURE_SEND_TARGET"]).netloc
            assert headers["webhook-id"] == key
            assert headers.get("Proxy-Authorization") is None
            assert int(headers["Timestamp"]) > 0
            assert body == BODY
            assert hmac.compare_digest(
                headers["Signature"], hmac.new(ROTATED if index in rotated else SECRET, BODY, hashlib.sha256).hexdigest()
            )

    worker = ["queue:work", env.get("FIXTURE_SEND_QUEUE", "database"), "--queue=webhook-outgoing", "--once", "--tries=7"]

    if profile := env.get("FIXTURE_SEND_TLS"):
        cases = {
            "server": [
                ("tls-default", "normal", "unknown"),
                ("tls-ca", "normal", "succeeded"),
                ("tls-wrong-ca", "normal", "unknown"),
                ("tls-insecure-denied", "tls-insecure", "rejected"),
                ("tls-insecure", "tls-insecure", "succeeded"),
                ("tls-verified-again", "tls-verified-again", "unknown"),
                ("redirect", "normal", "http_failure"),
            ],
            "mutual": [
                ("tls-no-client", "normal", "unknown"),
                ("tls-client", "normal", "succeeded"),
                ("tls-client-wrong", "normal", "unknown"),
                ("tls-client-expired", "normal", "unknown"),
                ("tls-client-mismatched", "normal", "rejected"),
                ("tls-client-unverified", "tls-insecure", "rejected"),
            ],
            "proxy": [
                ("proxy-missing", "proxy", "rejected"),
                ("proxy-wrong-name", "proxy", "rejected"),
                ("proxy-wrong-password", "proxy", "unknown"),
                ("proxy-good", "proxy", "succeeded"),
                ("redirect", "proxy", "http_failure"),
                ("proxy-direct", "normal", "succeeded"),
            ],
            "proxy-ip": [("proxy-good", "proxy", "succeeded")],
            "proxy-mutual": [
                ("tls-client", "proxy", "succeeded"),
                ("tls-no-client", "proxy", "unknown"),
            ],
            "proxy-wrong-host": [("tls-ca", "proxy", "unknown")],
            "wrong-host": [("tls-ca", "normal", "unknown")],
            "expired": [("tls-ca", "normal", "unknown")],
        }[profile]
        for key, mode, outcome in cases:
            identifier = submit(key, mode)
            enqueue(identifier)
            queue = sql("SELECT payload FROM jobs WHERE queue='webhook-outgoing'")
            assert identifier in queue and "PRIVATE KEY" not in queue and "BEGIN CERTIFICATE" not in queue
            assert "fixture-user" not in queue and "local p@ss" not in queue
            command(*worker)
            status = {"succeeded": 204, "http_failure": 302}.get(outcome, 0)
            assert attempts(identifier) == f"1\t{outcome}\t{status}", (key, attempts(identifier))
            snapshot = sql(f"SELECT snapshot FROM webhook_deliveries WHERE id='{identifier}'")
            assert "PRIVATE KEY" not in snapshot and "FIXTURE_SEND_TLS_DIR" not in snapshot
            if mode == "proxy":
                assert json.loads(snapshot)["proxy"] == "fixture-egress"
                assert "fixture-user" not in snapshot and "local p@ss" not in snapshot
                assert env["FIXTURE_SEND_PROXY_ADDRESS"] not in snapshot
            else:
                assert "proxy" not in json.loads(snapshot)
            if mode == "tls-insecure":
                assert json.loads(snapshot)["unverified_tls"] is True
            else:
                assert "unverified_tls" not in json.loads(snapshot)
            if status:
                captured(key)
            else:
                assert key not in receiver.received, key
        if profile in ("mutual", "proxy-mutual"):
            certificate = Path(env["FIXTURE_SEND_TLS_DIR"]) / "client.pem"
            assert receiver.client_certificates == {
                "tls-client": ssl.PEM_cert_to_DER_cert(certificate.read_text())
            }
        assert not receiver.unexpected
        expected_sni = None if profile == "proxy-ip" else "outgoing-fixture.invalid"
        assert receiver.tls_names and set(receiver.tls_names) == {expected_sni}
        if profile.startswith("proxy"):
            destination = ("127.0.0.1", receiver.server_port)
            proxy = receiver.proxy
            assert proxy.targets == [destination] * (2 if profile in ("proxy", "proxy-mutual") else 1), proxy.targets
            assert proxy.rejected == (1 if profile == "proxy" else 0)
        print(f"{driver}: TLS {profile} HTTP、Artisan、Worker 及固定连接通过")
        return

    def admin(operation, name, *options):
        return command("webhooks:fixture-endpoint", operation, name, *options)

    def publish(key, payload=None, mode="publish", expected=202):
        connection = http.client.HTTPConnection("127.0.0.1", int(env["APP_PORT"]), timeout=20)
        try:
            connection.request("POST", f"/api/outbound/{mode}/{key}",
                json.dumps(payload if payload is not None else {"status": "retry"}).encode(),
                {"Accept": "application/json", "Content-Type": "application/json"})
            response = connection.getresponse()
            result = json.loads(response.read())
            assert response.status == expected, (response.status, result)
            if expected == 202:
                assert re.fullmatch("[0-9a-f]{64}", result["message_id"])
                assert all(re.fullmatch("[0-9a-f]{64}", item) for item in result["delivery_ids"])
            return result
        finally:
            connection.close()

    if env.get("FIXTURE_SEND_DIAGNOSTICS"):
        def inspect(kind, identifier, expected=200, scope=SCOPE, auth=True, **options):
            connection = http.client.HTTPConnection("127.0.0.1", int(env["APP_PORT"]), timeout=20)
            headers = {f"x-fixture-{name}": str(value) for name, value in options.items()}
            if auth:
                headers["x-fixture-inspector"] = "fixture-operator"
            try:
                connection.request("GET", f"/api/outbound-inspect/{scope}/{kind}/{identifier}",
                                   headers=headers)
                response = connection.getresponse()
                raw = response.read()
                assert response.status == expected, (response.status, raw)
                return json.loads(raw)
            finally:
                connection.close()

        def inspect_cli(identifier, *args, scope=SCOPE, success=True):
            result = command("webhook-server:inspect", identifier, "--scope", scope, *args,
                             success=success)
            return json.loads(result) if success else None

        private_payload = b"diagnostic-fixture-hidden-content"
        identifier = submit("retry", body=private_payload)
        pending = inspect("delivery", identifier)
        assert pending == inspect_cli(identifier)
        assert pending == inspect_cli(identifier, f"--database={driver}")
        assert set(pending) == {"id", "state", "attempts", "maximum_tries", "available_at",
                                "deadline", "lease_until"}
        assert pending["state"] == "pending" and pending["attempts"] == 0
        assert pending["id"] == identifier and pending["lease_until"] is None
        assert pending["available_at"] < pending["deadline"]
        empty = {"data": [], "next_after": None}
        assert inspect("attempts", identifier) == empty
        assert inspect_cli(identifier, scope="another-tenant") is None
        assert inspect_cli(identifier, "--attempts", scope="another-tenant") == empty
        assert inspect("delivery", identifier, expected=403, auth=False) == {}
        assert inspect("delivery", identifier, expected=403, scope="another-tenant") == {}
        assert inspect("delivery", "0" * 64, expected=404) == {}
        assert inspect("delivery", "invalid", expected=422) == {}
        assert inspect("attempts", identifier, expected=422, limit=101) == {}
        inspect_cli(identifier, "--attempts", "--limit=0", success=False)
        inspect_cli(identifier, "--after=1", success=False)

        enqueue(identifier)
        command(*worker)
        assert inspect("delivery", identifier)["state"] == "pending"
        update(identifier, "available_at=0")
        enqueue(identifier)
        command(*worker)
        complete = inspect("delivery", identifier)
        assert complete == inspect_cli(identifier)
        assert complete["state"] == "succeeded" and complete["attempts"] == 2
        first = inspect("attempts", identifier, limit=1)
        assert first == inspect_cli(identifier, "--attempts", "--limit=1")
        assert first["next_after"] == 1
        assert len(first["data"]) == 1
        failed = first["data"][0]
        assert set(failed) == {"number", "outcome", "http_status", "started_at", "finished_at"}
        assert (failed["number"], failed["outcome"], failed["http_status"]) == (1, "http_failure", 503)
        assert failed["finished_at"] >= failed["started_at"]
        second = inspect("attempts", identifier, limit=1, after=first["next_after"])
        assert second == inspect_cli(identifier, "--attempts", "--limit=1", "--after=1")
        assert second["next_after"] is None and len(second["data"]) == 1
        assert (second["data"][0]["number"], second["data"][0]["http_status"]) == (2, 204)
        assert inspect("attempts", identifier, after=2) == empty
        assert inspect("attempts", identifier, after=4294967295) == empty
        assert inspect_cli(identifier, "--attempts", scope="another-tenant") == empty

        missing = publish("diag-no-subs", {"secret": private_payload.decode()})
        admin("create", "primary")
        admin("subscribe", "primary", "--topic=invoice.paid", "--schema=1")
        published = publish("diag-message", {"secret": private_payload.decode()})
        assert len(published["delivery_ids"]) == 1
        for receipt in (missing, published):
            mid = receipt["message_id"]
            metadata = inspect("message", mid)
            assert metadata == inspect_cli(mid, "--message")
            assert set(metadata) == {"id", "event_type", "schema_version", "created_at", "delivery_ids"}
            assert metadata["id"] == mid and metadata["delivery_ids"] == receipt["delivery_ids"]
            assert (metadata["event_type"], metadata["schema_version"]) == ("invoice.paid", 1)
            assert inspect_cli(mid, "--message", scope="another-tenant") is None
            assert inspect("message", mid, expected=403, auth=False) == {}
            assert private_payload.decode() not in json.dumps(metadata)
        assert inspect("delivery", published["delivery_ids"][0])["state"] == "pending"
        print(f"{driver}: 出站诊断 HTTP、Artisan、Worker、分页、范围隔离与脱敏通过")
        return

    if env.get("FIXTURE_SEND_FANOUT"):
        def verify_requests(message, expected_paths, payload=None, schema=1, topic="invoice.paid"):
            with receiver.mutex:
                hits = list(receiver.received.get(message["message_id"], []))
            assert sorted(path for path, _, _ in hits) == sorted(expected_paths)
            for path, headers, body in hits:
                assert headers["webhook-id"] == message["message_id"]
                assert headers["Content-Type"] == "application/json"
                assert json.loads(body) == {"schema_version": schema, "type": topic,
                                          "data": payload if payload is not None else {"status": "retry"}}
                secret = ROTATED if path.endswith("?endpoint=backup") else SECRET
                assert hmac.compare_digest(headers["Signature"], hmac.new(secret, body, hashlib.sha256).hexdigest())
            assert len({body for _, _, body in hits}) <= 1

        admin("create", "primary")
        admin("create", "backup", "--backup", "--rotated")
        admin("create", "elsewhere", "--scope=wrong-scope")
        admin("subscribe", "elsewhere", "--scope=wrong-scope")
        admin("subscribe", "primary")
        admin("subscribe", "primary")
        admin("subscribe", "backup")
        assert sql("SELECT COUNT(*) FROM webhook_subscriptions") == "3"
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
            publications = list(executor.map(publish, ("same-message", "same-message")))
        assert publications[0] == publications[1]
        first = publications[0]
        assert len(first["delivery_ids"]) == 2 and len(set(first["delivery_ids"])) == 2
        assert sql("SELECT COUNT(*) FROM webhook_messages") == "1"
        assert sql("SELECT COUNT(*) FROM webhook_deliveries") == "2"
        for identifier in first["delivery_ids"]:
            assert read(identifier) == "pending\t0"
            snapshot = json.loads(sql(f"SELECT snapshot FROM webhook_deliveries WHERE id='{identifier}'"))
            assert snapshot["body"] == "" and snapshot["message_ref"]["id"] == first["message_id"]
        publish("same-message", {"status": "changed"}, expected=409)
        admin("unsubscribe", "primary")
        assert publish("same-message") == first
        verify_requests(first, [])
        command("webhook-server:recover", "--scope", SCOPE)
        command(*worker)
        command(*worker)
        pending_id, = [identifier for identifier in first["delivery_ids"] if read(identifier) == "pending\t1"]
        succeeded_id, = [identifier for identifier in first["delivery_ids"] if read(identifier) == "succeeded\t1"]
        verify_requests(first, ["/receiver", "/receiver?endpoint=backup"])
        update(pending_id, "available_at=0")
        assert '"succeeded"' in send(pending_id)
        assert attempts(succeeded_id) == "1\tsucceeded\t204"
        assert attempts(pending_id) == "1\thttp_failure\t503\n2\tsucceeded\t204"
        verify_requests(first, ["/receiver", "/receiver?endpoint=backup", "/receiver?endpoint=backup"])

        later = publish("new-message", {"status": "paid"})
        assert len(later["delivery_ids"]) == 1
        assert '"succeeded"' in send(later["delivery_ids"][0])
        verify_requests(later, ["/receiver?endpoint=backup"], {"status": "paid"})
        empty = publish("version-two", mode="publish-schema-two")
        assert not empty["delivery_ids"]
        admin("subscribe", "primary", "--schema=2")
        assert publish("version-two", mode="publish-schema-two") == empty
        versioned = publish("new-version-two", {"status": "paid"}, mode="publish-schema-two")
        assert len(versioned["delivery_ids"]) == 1
        assert '"succeeded"' in send(versioned["delivery_ids"][0])
        verify_requests(versioned, ["/receiver"], {"status": "paid"}, schema=2)

        replay_body = {"status": "fail-original"}
        original = publish("replay-source-message", replay_body)
        original_id, = original["delivery_ids"]
        for attempt in range(3):
            update(original_id, "available_at=0")
            assert ('"failed"' if attempt == 2 else '"pending"') in send(original_id)
        result = command("webhook-server:replay", original_id, "--scope", SCOPE,
                         "--actor=fixture-operator", "--reason=test-fanout")
        replay_id = json.loads(re.search(r"\{[^\n]+\}", result)[0])["delivery_id"]
        snapshot = json.loads(sql(f"SELECT snapshot FROM webhook_deliveries WHERE id='{replay_id}'"))
        assert snapshot["body"] == "" and snapshot["message_ref"]["id"] == original["message_id"]
        assert '"succeeded"' in send(replay_id) and read(original_id) == "failed\t3"
        verify_requests(original, ["/receiver?endpoint=backup"] * 4, replay_body)

        for endpoint in ("primary", "backup"):
            admin("subscribe", endpoint, "--topic=invoice.concurrent")
        root = Path(env["APP_BASE_PATH"])
        ready = root / "publication-ready"
        release = root / "publication-release"
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
            pending = executor.submit(publish, "holding", {"status": "paid"}, "publish-hold")
            try:
                deadline = time.monotonic() + 8
                while not ready.exists() and time.monotonic() < deadline and not pending.done():
                    time.sleep(.02)
                assert ready.exists()
                unpublished = ready.read_text()
                assert re.fullmatch("[0-9a-f]{64}", unpublished)
                assert sql(f"SELECT COUNT(*) FROM webhook_messages WHERE id='{unpublished}'") == "0"
                mutation = executor.submit(admin, "unsubscribe", "primary", "--topic=invoice.concurrent")
                time.sleep(.1)
            finally:
                release.touch()
            held = pending.result()
            mutation.result()
        assert held["message_id"] == unpublished and len(held["delivery_ids"]) == 2
        assert publish("holding", {"status": "paid"}, "publish-concurrent") == held
        second = publish("after-unsubscribe", {"status": "paid"}, "publish-concurrent")
        assert len(second["delivery_ids"]) == 1
        for identifier in held["delivery_ids"] + second["delivery_ids"]:
            assert '"succeeded"' in send(identifier)
        verify_requests(held, ["/receiver", "/receiver?endpoint=backup"], {"status": "paid"}, topic="invoice.concurrent")
        verify_requests(second, ["/receiver?endpoint=backup"], {"status": "paid"}, topic="invoice.concurrent")

        total = sql("SELECT COUNT(*) FROM webhook_deliveries")
        aborted = publish("outer-rollback", mode="publish-rollback")
        assert not any(read(identifier) for identifier in aborted["delivery_ids"])
        assert sql("SELECT COUNT(*) FROM webhook_deliveries") == total
        assert sql(f"SELECT COUNT(*) FROM webhook_messages WHERE id='{aborted['message_id']}'") == "0"

        scope_hash = hashlib.sha256(SCOPE.encode()).hexdigest()
        targets = sorted(("savepoint-first", "savepoint-second"), key=lambda name:
            hashlib.sha256(f"webhook-endpoint:{scope_hash}:{name}".encode()).hexdigest())
        admin("create", targets[0])
        admin("create", targets[1], "--untrusted")
        for name in targets:
            admin("subscribe", name, "--topic=invoice.savepoint")
        publish("partial-savepoint", mode="publish-savepoint", expected=422)
        assert sql("SELECT COUNT(*) FROM webhook_messages WHERE topic='invoice.savepoint'") == "0"
        assert sql("SELECT COUNT(*) FROM webhook_deliveries") == total

        before_payload = sql("SELECT COUNT(*) FROM webhook_messages")
        publish("oversized-envelope", {"value": "x" * (1024 * 1024)}, expected=422)
        assert sql("SELECT COUNT(*) FROM webhook_messages") == before_payload
        admin("bulk", "bound", "--topic=invoice.overflow")
        publish("overflow", mode="publish-overflow", expected=422)
        assert sql("SELECT COUNT(*) FROM webhook_messages WHERE topic='invoice.overflow'") == "0"
        assert sql("SELECT COUNT(*) FROM webhook_deliveries") == total

        missing = publish("damaged-message", {"status": "paid"})
        delivery, = missing["delivery_ids"]
        routes["write_database"](driver, sql,
            f"UPDATE webhook_messages SET body='invalid' WHERE id='{missing['message_id']}'")
        send(delivery, success=False)
        assert read(delivery) == "pending\t0" and not attempts(delivery)
        verify_requests(missing, [])

        print(f"{driver}: 消息幂等、固定订阅集合、同事务回滚、扇出上限及共享正文通过", flush=True)
        return

    if env.get("FIXTURE_SEND_LIMITS"):
        scope_hash = hashlib.sha256(SCOPE.encode()).hexdigest()

        def buckets(kind):
            return sql(f"SELECT used FROM webhook_rate_buckets WHERE kind='{kind}' "
                       f"AND scope_key='{scope_hash}' ORDER BY used")

        def reset(*kinds):
            selected = ",".join(f"'{kind}'" for kind in kinds)
            routes["write_database"](driver, sql,
                f"UPDATE webhook_rate_buckets SET reset_at=0 WHERE kind IN ({selected})")

        def available(identifier):
            return int(sql(f"SELECT available_at FROM webhook_deliveries WHERE id='{identifier}'"))

        command("webhooks:fixture-endpoint", "create", "one")
        command("webhooks:fixture-endpoint", "create", "two")
        keys = ("limit-concurrent-one", "limit-concurrent-two")
        identifiers = [submit(key, "endpoint-one") for key in keys]
        peer_root = Path(env["APP_BASE_PATH"]) / "limits-peer"
        peer_root.mkdir()
        peer = env | {"APP_BASE_PATH": str(peer_root),
                      "APP_CONFIG_CACHE": str(peer_root / "config.json"),
                      "APP_ROUTES_CACHE": str(peer_root / "routes.json")}
        command("config:cache", current=peer)
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
            calls = [executor.submit(send, identifier, current=node)
                     for identifier, node in zip(identifiers, (env, peer))]
            outcomes = [call.result() for call in calls]
        assert sum('"succeeded"' in output for output in outcomes) == 1, outcomes
        assert sum('"pending"' in output for output in outcomes) == 1, outcomes
        waiting_index = next(i for i, output in enumerate(outcomes) if '"pending"' in output)
        waiting_id, waiting_key = identifiers[waiting_index], keys[waiting_index]
        assert read(waiting_id) == "pending\t0" and not attempts(waiting_id), (read(waiting_id), attempts(waiting_id))
        assert buckets("scope") == "1" and buckets("endpoint") == "1"
        assert available(waiting_id) == int(sql(
            "SELECT reset_at FROM webhook_rate_buckets WHERE kind='endpoint'"
        ))
        captured(waiting_key, 0)
        captured(keys[1 - waiting_index])

        direct = submit("limit-direct")
        assert '"succeeded"' in send(direct)
        assert buckets("scope") == "2"
        isolated = submit("limit-isolated", "limit-other-scope")
        assert '"succeeded"' in send(isolated, scope="webhook-outbound-limit-alternate")
        captured("limit-isolated")
        assert buckets("scope") == "2"
        blocked = submit("limit-second-endpoint", "endpoint-two")
        assert '"pending"' in send(blocked)
        assert read(blocked) == "pending\t0" and not attempts(blocked)
        assert buckets("endpoint") == "1"
        captured("limit-second-endpoint", 0)
        output = command("webhook-server:recover", "--scope", SCOPE, "--pretend")
        assert json.loads(re.search(r"\{[^\n]+\}", output)[0])["candidates"] == 0

        short = submit("limit-short-deadline")
        closes_at = int(sql("SELECT reset_at FROM webhook_rate_buckets WHERE kind='scope' "
                            f"AND scope_key='{scope_hash}'")) - 15
        update(short, f"deadline={closes_at}")
        assert '"pending"' in send(short) and available(short) == closes_at
        update(short, "deadline=0")
        send(short)
        assert read(short) == "expired\t0" and not attempts(short)
        captured("limit-short-deadline", 0)

        reset("scope", "endpoint")
        for identifier in (waiting_id, blocked):
            update(identifier, "available_at=0")
        command("webhook-server:recover", "--scope", SCOPE)
        command(*worker)
        command(*worker)
        assert read(waiting_id) == "succeeded\t1" and read(blocked) == "succeeded\t1"
        assert buckets("scope") == "2" and buckets("endpoint") == "1\n1"
        captured(waiting_key)
        captured("limit-second-endpoint")

        reset("scope")
        command("webhooks:fixture-endpoint", "replace", "one", "--rotated")
        version = submit("limit-version-replacement", "endpoint-one")
        assert '"pending"' in send(version)
        assert read(version) == "pending\t0" and not attempts(version)
        assert buckets("scope") == "2"
        captured("limit-version-replacement", 0)
        reset("endpoint")
        update(version, "available_at=0")
        assert '"succeeded"' in send(version)
        assert buckets("scope") == "1"
        captured("limit-version-replacement", rotated=(0,))

        reset("scope")
        retried = submit("retry")
        assert '"pending"' in send(retried) and read(retried) == "pending\t1"
        filler = submit("limit-retry-filler")
        assert '"succeeded"' in send(filler)
        update(retried, "available_at=0")
        assert '"pending"' in send(retried) and read(retried) == "pending\t1"
        assert attempts(retried) == "1\thttp_failure\t503"
        captured("retry")
        reset("scope")
        update(retried, "available_at=0")
        assert '"succeeded"' in send(retried)
        assert attempts(retried) == "1\thttp_failure\t503\n2\tsucceeded\t204"
        captured("retry", 2)

        reset("scope")
        replay_keys = ("replay-failed-limit-one", "replay-failed-limit-two")
        replay_sources = [submit(key, "fail-once") for key in replay_keys]
        for identifier in replay_sources:
            assert '"failed"' in send(identifier)
        total = int(sql("SELECT COUNT(*) FROM webhook_deliveries"))
        work = ["webhook-server:replay", "--scope", SCOPE,
                "--actor=fixture-operator", "--reason=test-limits"]
        def register_replay(identifier):
            return subprocess.run([str(binary), "artisan", *work, identifier],
                                  env=env, capture_output=True, text=True, timeout=20)
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
            results = list(executor.map(register_replay, replay_sources))
        assert sum(result.returncode == 0 for result in results) == 1, results
        assert "限额" in next(result.stdout + result.stderr for result in results if result.returncode)
        admitted = json.loads(re.search(r"\{[^\n]+\}", next(
            result.stdout for result in results if result.returncode == 0
        ))[0])
        assert int(sql("SELECT COUNT(*) FROM webhook_deliveries")) == total + 1
        assert sql("SELECT COUNT(*) FROM webhook_delivery_replays") == "1" and buckets("replay") == "1"
        child = admitted["delivery_id"]
        replay_key = replay_keys[replay_sources.index(admitted["original_delivery_id"])]
        assert '"pending"' in send(child)
        assert read(child) == "pending\t0" and not attempts(child)
        captured(replay_key)
        reset("scope")
        update(child, "available_at=0")
        enqueue(child)
        command(*worker)
        captured(replay_key, 2, rotated=(1,))
        assert read(child) == "succeeded\t1" and buckets("scope") == "1"
        assert read(admitted["original_delivery_id"]) == "failed\t1"

        print(f"{driver}: 跨进程发送限额、端点版本、范围隔离、窗口恢复及重放限额通过", flush=True)
        return

    if env.get("FIXTURE_SEND_ENDPOINTS"):
        def endpoint(operation, name, *options, success=True):
            return command("webhooks:fixture-endpoint", operation, name, *options, success=success)

        endpoint("create", "paused")
        endpoint("create", "paused", success=False)
        waiting = submit("endpoint-paused-worker", "endpoint-paused")
        expiring = submit("endpoint-pause-expiry", "endpoint-paused")
        enqueue(waiting)
        endpoint("pause", "paused")
        command(*worker)
        assert read(waiting) == "pending\t0" and not attempts(waiting)
        assert '"unavailable"' in send(waiting)
        report = command("webhook-server:recover", "--scope", SCOPE)
        assert json.loads(re.search(r"\{[^\n]+\}", report)[0])["skipped"] >= 1
        captured("endpoint-paused-worker", 0)
        update(expiring, "deadline=0")
        send(expiring)
        assert read(expiring) == "expired\t0"
        endpoint("resume", "paused")
        assert '"succeeded"' in send(waiting) and read(waiting) == "succeeded\t1"
        captured("endpoint-paused-worker")
        assert '"unavailable"' in send(expiring)
        captured("endpoint-pause-expiry", 0)

        endpoint("create", "rotate")
        stale = submit("endpoint-old-revision", "endpoint-rotate")
        assert "revision 2" in endpoint("replace", "rotate", "--rotated")
        endpoint("pause", "rotate", success=False)
        assert '"cancelled"' in send(stale)
        assert read(stale) == "cancelled\t1"
        captured("endpoint-old-revision", 0)
        fresh = submit("endpoint-new-revision", "endpoint-rotate")
        assert '"succeeded"' in send(fresh)
        captured("endpoint-new-revision", rotated=(0,))

        endpoint("create", "cross-scope", "--scope=other-endpoint-scope")
        submit("endpoint-scope-denied", "endpoint-cross-scope", expected=409)
        endpoint("create", "override")
        submit("endpoint-reference-denied", "endpoint-override", expected=409)
        captured("endpoint-reference-denied", 0)

        endpoint("create", "race")
        late = submit("endpoint-late-pause", "endpoint-race")
        root = Path(env["APP_BASE_PATH"])
        entered = root / "endpoint-policy-entered"
        release = root / "endpoint-policy-release"
        process = subprocess.Popen(
            [str(binary), "artisan", "webhook-server:send", late, "--scope", SCOPE],
            env=env | {"FIXTURE_ENDPOINT_WAIT_FOR_PERMISSION": "1"},
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
        )
        try:
            deadline = time.monotonic() + 8
            while not entered.exists() and time.monotonic() < deadline and process.poll() is None:
                time.sleep(.02)
            assert entered.exists() and read(late) == "running\t1"
            endpoint("pause", "race")
            release.touch()
            result, _ = process.communicate(timeout=12)
            assert process.returncode == 0 and '"pending"' in result, result
            assert read(late) == "pending\t0" and not attempts(late)
            captured("endpoint-late-pause", 0)
        finally:
            release.touch()
            if process.poll() is None:
                process.kill()
                process.wait(timeout=10)
        endpoint("resume", "race")
        assert '"succeeded"' in send(late)
        captured("endpoint-late-pause")

        inflight = submit("endpoint-inflight", "endpoint-race")
        process = subprocess.Popen(
            [str(binary), "artisan", "webhook-server:send", inflight, "--scope", SCOPE],
            env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
        )
        try:
            assert receiver.entered["endpoint-inflight"].wait(timeout=8)
            assert read(inflight) == "running\t1"
            endpoint("delete", "race")
            receiver.release["endpoint-inflight"].set()
            result, _ = process.communicate(timeout=12)
            assert process.returncode == 0 and '"cancelled"' in result, result
            assert read(inflight) == "cancelled\t1" and attempts(inflight) == "1\tsucceeded\t204"
            captured("endpoint-inflight")
        finally:
            receiver.release["endpoint-inflight"].set()
            if process.poll() is None:
                process.kill()
                process.wait(timeout=10)
        endpoint("resume", "race", success=False)
        endpoint("create", "race", success=False)
        assert '"unavailable"' in send(inflight)
        captured("endpoint-inflight")

        endpoint("create", "replay")
        original = submit("endpoint-replay-source", "endpoint-replay")
        assert '"failed"' in send(original)
        replay = ("webhook-server:replay", original, "--scope", SCOPE,
                  "--actor=fixture-operator", "--reason=test-endpoint")
        endpoint("pause", "replay")
        command(*replay, success=False)
        endpoint("resume", "replay")
        result = command(*replay)
        replayed_id = json.loads(re.search(r"\{[^\n]+\}", result)[0])["delivery_id"]
        enqueue(replayed_id)
        endpoint("pause", "replay")
        command(*worker)
        assert read(replayed_id) == "pending\t0"
        endpoint("resume", "replay")
        assert '"succeeded"' in send(replayed_id)
        captured("endpoint-replay-source", 2)
        endpoint("delete", "replay")
        command(*replay, success=False)

        command("migrate:rollback", "--path=vendor/openrust/webhook-server/database/migrations/"
                "2026_11_25_000000_create_webhook_endpoints_table.rs", "--force", success=False)
        print(f"{driver}: 端点暂停恢复、版本变化、跨范围、删除竞态及重放当前许可通过", flush=True)
        return

    if env.get("FIXTURE_SEND_REPLAY"):
        def replay(identifier, *options, scope=SCOPE, actor="fixture-operator", current=env, success=True):
            tokens = (identifier, "--scope", scope, "--actor", actor, "--reason", "test-replay", *options)
            output = command("webhook-server:replay", *tokens, current=current, success=success)
            if success:
                return json.loads(re.search(r"\{[^\n]+\}", output)[0])

        def failed_source(key):
            identifier = submit(key, "fail-once")
            assert '"failed"' in send(identifier)
            assert attempts(identifier) == "1\thttp_failure\t503"
            captured(key)
            return identifier

        source = failed_source("replay-failed-main")
        replay(source, scope="wrong-scope", success=False)
        replay(source, actor="untrusted", success=False)
        replay(source, current=env | {"FIXTURE_REPLAY_UNREGISTERED": "1"}, success=False)
        replay(source, current=env | {"FIXTURE_SEND_REVOKED": "1"}, success=False)
        replay(source, current=env | {"FIXTURE_REPLAY_NO_ADMISSION": "1"}, success=False)
        assert sql("SELECT COUNT(*) FROM webhook_delivery_replays") == "0"

        work = [str(binary), "artisan", "webhook-server:replay", source,
                f"--scope={SCOPE}", "--actor=fixture-operator", "--reason=test-concurrent"]
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
            tasks = [executor.submit(subprocess.run, work, env=env, capture_output=True,
                                     text=True, timeout=20) for _ in range(2)]
            concurrent_results = [task.result() for task in tasks]
        assert sum(result.returncode == 0 for result in concurrent_results) == 1, concurrent_results
        response = next(result.stdout for result in concurrent_results if result.returncode == 0)
        first = json.loads(re.search(r"\{[^\n]+\}", response)[0])
        assert first["original_delivery_id"] == source and first["generation"] == 1
        first_id = first["delivery_id"]
        assert first_id != source and read(first_id) == "pending\t0"
        assert sql("SELECT actor,reason,original_credential,replay_credential FROM webhook_delivery_replays") == (
            "fixture-operator\ttest-concurrent\tfixture-outbound-ref\tfixture-rotated-ref"
        )
        replay(source, success=False)
        replay(first_id, success=False)
        assert attempts(source) == "1\thttp_failure\t503"
        captured("replay-failed-main")
        command("webhook-server:recover", "--scope", SCOPE)
        command(*worker, current=env | {"FIXTURE_REPLAY_NO_ADMISSION": "1"})
        assert read(first_id) == "succeeded\t1"
        assert read(source) == "failed\t1"
        captured("replay-failed-main", 2, rotated=(1,))

        second = replay(source)
        assert second["generation"] == 2 and second["delivery_id"] != first_id
        second_id = second["delivery_id"]
        assert '"failed"' in send(second_id, current=env | {"FIXTURE_REPLAY_FORBIDDEN": "1"})
        assert attempts(second_id) == "1\trejected\t0"
        replay(source, success=False)
        captured("replay-failed-main", 2, rotated=(1,))

        revoked = failed_source("replay-failed-credential")
        revoked_child = replay(revoked)["delivery_id"]
        assert '"failed"' in send(revoked_child, current=env | {"FIXTURE_REPLAY_REVOKE_CREDENTIAL": "1"})
        assert attempts(revoked_child) == "1\trejected\t0"
        captured("replay-failed-credential")

        changed = failed_source("replay-failed-policy-change")
        changed_child = replay(changed)["delivery_id"]
        assert '"failed"' in send(changed_child, current=env | {"FIXTURE_REPLAY_KEEP_ORIGINAL": "1"})
        assert attempts(changed_child) == "1\trejected\t0"
        captured("replay-failed-policy-change")

        expired = failed_source("replay-failed-expiry")
        expired_child = replay(expired)["delivery_id"]
        assert sql(f"SELECT maximum_tries, deadline - available_at FROM webhook_deliveries WHERE id='{expired_child}'") == "1\t60"
        update(expired_child, "deadline=0")
        send(expired_child)
        assert read(expired_child) == "expired\t0"
        captured("replay-failed-expiry")

        success_source = submit("replay-success")
        replay(success_source, "--include-succeeded", actor="fixture-supervisor", success=False)
        assert '"succeeded"' in send(success_source)
        replay(success_source, actor="fixture-supervisor", success=False)
        replay(success_source, "--include-succeeded", success=False)
        elevated = replay(success_source, "--include-succeeded", actor="fixture-supervisor")["delivery_id"]
        assert '"succeeded"' in send(elevated)
        assert read(success_source) == "succeeded\t1"
        captured("replay-success", 2, rotated=(1,))

        missing = failed_source("replay-failed-cleared")
        update(missing, "snapshot='{}'")
        replay(missing, success=False)
        captured("replay-failed-cleared")

        migration = ("--path=vendor/openrust/webhook-server/database/migrations/"
                     "2026_11_25_000000_extend_webhook_deliveries_for_replay.rs")
        command("migrate:rollback", migration, "--force", success=False)
        assert sql("SELECT COUNT(*) FROM webhook_delivery_replays") == "6"
        print(f"{driver}: 人工重放授权、并发代次、凭据轮换、重试预算与原历史保护通过", flush=True)
        return

    if env.get("FIXTURE_SEND_SCHEDULE"):
        def recover(*options, scope=SCOPE, current=env, success=True):
            result = command("webhook-server:recover", "--scope", scope, *options,
                             current=current, success=success)
            return json.loads(re.search(r"\{[^\n]+\}", result)[0])

        redis_container = env.get("OPENRUST_TEST_WEBHOOK_REDIS_CONTAINER")
        redis_key = env.get("REDIS_PREFIX", "") + "queues:webhook-outgoing"

        def redis_cli(*tokens):
            return subprocess.check_output(
                ["docker", "exec", redis_container, "redis-cli", "--raw", *tokens],
                text=True, timeout=10,
            ).strip()

        def redis_items(identifier):
            return [wire for wire in redis_cli("LRANGE", redis_key, "0", "-1").splitlines()
                    if json.loads(json.loads(wire)["payload"])["data"] == identifier]

        def queued_for(identifier):
            if redis_container:
                return len(redis_items(identifier))
            return int(sql(
                "SELECT COUNT(*) FROM jobs WHERE queue='webhook-outgoing' "
                f"AND payload LIKE '%{identifier}%' AND reserved_at IS NULL"
            ))

        def marker(identifier):
            return sql(f"SELECT coalesce(next_wake_at,0) FROM webhook_deliveries WHERE id='{identifier}'")

        assert "webhook-server:recover" in command("schedule:list")
        registered = submit("recover-unenqueued")
        assert marker(registered) == "0" and queued_for(registered) == 0
        assert recover("--pretend") == {"candidates": 1, "queued": 0, "retired": 0, "skipped": 0, "failed": 0}
        assert recover(scope="wrong-scope")["candidates"] == 0
        assert marker(registered) == "0" and queued_for(registered) == 0
        second_root = Path(env["APP_BASE_PATH"]) / "scheduler-peer"
        second_root.mkdir()
        peer = env | {
            "APP_BASE_PATH": str(second_root),
            "APP_CONFIG_CACHE": str(second_root / "config.json"),
            "APP_ROUTES_CACHE": str(second_root / "routes.json"),
        }
        command("config:cache", current=peer)
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
            results = [executor.submit(command, "schedule:run", current=node) for node in (env, peer)]
            for result in results:
                result.result()
        assert marker(registered) != "0" and queued_for(registered) == 1
        assert recover()["candidates"] == 0 and read(registered) == "pending\t0"
        captured("recover-unenqueued", 0)
        command(*worker)
        assert read(registered) == "succeeded\t1"
        captured("recover-unenqueued")

        lost = submit("recover-lost", "builder-async-if")
        assert queued_for(lost) == 1 and marker(lost) != "0"
        if redis_container:
            wire, = redis_items(lost)
            assert redis_cli("LREM", redis_key, "1", wire) == "1"
        else:
            routes["write_database"](
                driver, sql, f"DELETE FROM jobs WHERE queue='webhook-outgoing' AND payload LIKE '%{lost}%'"
            )
        assert queued_for(lost) == 0 and recover()["candidates"] == 0
        update(lost, "next_wake_at=0")
        assert recover()["queued"] == 1 and queued_for(lost) == 1
        command(*worker)
        assert read(lost) == "succeeded\t1"
        captured("recover-lost")

        retry = submit("retry")
        assert recover()["queued"] == 1
        command(*worker)
        assert read(retry) == "pending\t1" and marker(retry) == "0"
        assert recover()["candidates"] == 0
        update(retry, "available_at=0")
        command("schedule:run")
        assert queued_for(retry) == 1
        command(*worker)
        assert attempts(retry) == "1\thttp_failure\t503\n2\tsucceeded\t204"
        captured("retry", 2)

        scheduler_crash = submit("scheduler-crash")
        result = subprocess.run(
            [str(binary), "artisan", "webhook-server:recover", f"--scope={SCOPE}"],
            env=env | {"FIXTURE_CRASH_OUTBOUND_WAKE": "1"}, capture_output=True, text=True, timeout=20,
        )
        assert result.returncode == 82, result
        assert queued_for(scheduler_crash) == 0 and read(scheduler_crash) == "pending\t0"
        assert marker(scheduler_crash) != "0" and recover()["candidates"] == 0
        update(scheduler_crash, "next_wake_at=0")
        assert recover()["queued"] == 1
        command(*worker)
        captured("scheduler-crash")

        failed_dispatch = submit("dispatch-listener-error")
        report = recover(success=False)
        assert report["failed"] == 1 and report["queued"] == 0
        assert marker(failed_dispatch) != "0" and queued_for(failed_dispatch) == 0
        assert recover()["candidates"] == 0 and read(failed_dispatch) == "pending\t0"
        update(failed_dispatch, "deadline=0")
        assert recover()["retired"] == 1 and read(failed_dispatch) == "expired\t0"
        captured("dispatch-listener-error", 0)

        broker = submit("broker-unavailable")
        unavailable = env | {
            "APP_CONFIG_CACHE": str(Path(env["APP_BASE_PATH"]) / "missing-broker-config.json"),
            "DB_QUEUE_TABLE": "outbound_acceptance_missing_queue",
        }
        if redis_container:
            unavailable["REDIS_URL"] = "redis://127.0.0.1:1"
        assert recover(current=unavailable, success=False)["failed"] == 1
        assert queued_for(broker) == 0 and read(broker) == "pending\t0"
        assert recover()["candidates"] == 0
        update(broker, "next_wake_at=0")
        assert recover()["queued"] == 1
        command(*worker)
        captured("broker-unavailable")

        crash = submit("crash")
        assert recover()["queued"] == 1
        process = subprocess.Popen([str(binary), "artisan", *worker], env=env, stdout=log, stderr=log)
        try:
            assert receiver.entered["crash"].wait(timeout=10)
            assert marker(crash) == "0" and recover()["candidates"] == 0
            process.kill()
            process.wait(timeout=10)
            update(crash, "lease_until=0")
            assert recover()["queued"] == 1
            assert read(crash) == "running\t1"
            command(*worker)
            assert attempts(crash) == "1\tunknown\t0\n2\tsucceeded\t204"
            captured("crash", 2)
        finally:
            receiver.release["crash"].set()
            if process.poll() is None:
                process.kill()
                process.wait(timeout=10)

        migration = ("--path=vendor/openrust/webhook-server/database/migrations/"
                     "2026_11_25_000000_extend_webhook_deliveries_for_recovery.rs")
        assert "extend_webhook_deliveries_for_recovery" in command("migrate:rollback", migration, "--force")
        assert attempts(crash) == "1\tunknown\t0\n2\tsucceeded\t204"
        assert "extend_webhook_deliveries_for_recovery" in command("migrate", migration, "--force")
        assert recover()["candidates"] == 0
        print(f"{driver}/{worker[1]}: 并发 Scheduler、丢失任务、到期退避、崩溃补派、范围隔离及迁移通过", flush=True)
        return

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

    custom_sync = submit("custom-stop-sync", "builder-custom-sync", expected=502)
    assert read(custom_sync) == "failed\t1"
    captured("custom-stop-sync")

    custom_worker = submit("custom-stop-worker", "builder-custom-async")
    assert '"custom_job":true' in sql(f"SELECT snapshot FROM webhook_deliveries WHERE id='{custom_worker}'")
    command(*worker)
    assert read(custom_worker) == "failed\t1"
    assert '"unavailable"' in send(custom_worker)
    captured("custom-stop-worker")

    continued = submit("custom-continue", "builder-custom-async")
    command(*worker)
    assert read(continued) == "pending\t1"
    assert '"succeeded"' in send(continued)
    captured("custom-continue", 2)

    missing = submit("custom-missing", "builder-custom-async")
    command(*worker, current=env | {"FIXTURE_HIDE_JOB_HOOK": "1"})
    assert read(missing) == "pending\t0" and attempts(missing) == ""
    send(missing, current=env | {"FIXTURE_HIDE_JOB_HOOK": "1"}, success=False)
    assert read(missing) == "pending\t0" and attempts(missing) == ""
    captured("custom-missing", 0)
    assert '"succeeded"' in send(missing)
    captured("custom-missing")

    revoked_custom = submit("custom-revoked", "builder-custom-async")
    command(*worker, current=env | {"FIXTURE_SEND_REVOKED": "1"})
    assert attempts(revoked_custom) == "1\trejected\t0"
    captured("custom-revoked", 0)

    wrong_worker = submit("custom-wrong-worker", "builder-custom-async")
    routes["write_database"](driver, sql,
        "UPDATE jobs SET payload=replace(payload, '.fixture-custom', '.default') "
        f"WHERE queue='webhook-outgoing' AND payload LIKE '%{wrong_worker}%'")
    command(*worker)
    assert read(wrong_worker) == "pending\t0" and attempts(wrong_worker) == ""
    captured("custom-wrong-worker", 0)
    assert '"succeeded"' in send(wrong_worker)
    captured("custom-wrong-worker")

    hooks = [json.loads(line) for line in Path(env["FIXTURE_JOB_HOOK_LOG"]).read_text().splitlines()]
    assert [(item["message"], item["kind"]) for item in hooks] == [
        ("custom-stop-sync", "should_remove"), ("custom-stop-sync", "failed"),
        ("custom-stop-worker", "should_remove"), ("custom-stop-worker", "failed"),
        ("custom-continue", "should_remove"), ("custom-revoked", "failed"),
    ], hooks
    for hook in hooks:
        assert hook["scope"] == SCOPE and hook["number"] == 1 and hook["id"]
        assert hook["meta"] == {"trace": "fixture"} and hook["tags"] == ["audit"]
        assert hook["url"] == env["FIXTURE_SEND_TARGET"] and hook["request_matches"]
        if hook["message"] == "custom-revoked":
            assert hook["outcome"] == "rejected" and hook["status"] is None and not hook["response_matches"]
        else:
            assert hook["outcome"] == "http_failure" and hook["status"] == 503 and hook["response_matches"]

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
        assert b"fixture-custom-hook-sensitive" not in output


class SocksProxy(socketserver.BaseRequestHandler):
    def read(self, count):
        result = b""
        while len(result) < count:
            data = self.request.recv(count - len(result))
            if not data:
                raise ConnectionError("incomplete SOCKS5 request")
            result += data
        return result

    def handle(self):
        self.request.settimeout(6)
        version, count = self.read(2)
        assert version == 5 and 2 in self.read(count)
        self.request.sendall(b"\x05\x02")
        version, size = self.read(2)
        username = self.read(size)
        password = self.read(self.read(1)[0])
        if (version, username, password) != (1, b"fixture-user", b"local p@ss/%"):
            self.server.rejected += 1
            self.request.sendall(b"\x01\x01")
            return
        self.request.sendall(b"\x01\x00")
        # 被授权的回环目标固定为数字 IPv4，收到域名即为回归。
        assert self.read(4) == b"\x05\x01\x00\x01"
        target = socket.inet_ntoa(self.read(4)), int.from_bytes(self.read(2), "big")
        assert target == self.server.expected, target
        self.server.targets.append(target)
        with socket.create_connection(target, timeout=6) as peer:
            self.request.sendall(b"\x05\x00\x00\x01\x00\x00\x00\x00\x00\x00")
            swap = {self.request: peer, peer: self.request}
            while ready := select.select(list(swap), [], [], 6)[0]:
                for source in ready:
                    data = source.recv(8192)
                    if not data:
                        return
                    swap[source].sendall(data)


def configure_tls(receiver, folder, profile):
    def openssl(*tokens):
        subprocess.run(["openssl", *tokens], cwd=folder, check=True, capture_output=True, timeout=10)

    def ca(name):
        openssl("req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256",
                "-noenc", "-subj", f"/CN={name}", "-days", "2",
                "-keyout", f"{name}.key", "-out", f"{name}.pem")

    def certificate(name, authority, serial, extensions, dates=("-days", "2")):
        openssl("req", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256",
                "-noenc", "-subj", f"/CN={name}", "-keyout", f"{name}.key", "-out", f"{name}.csr")
        config = folder / f"{name}.ext"
        config.write_text(extensions)
        openssl("x509", "-req", "-in", f"{name}.csr", "-CA", f"{authority}.pem",
                "-CAkey", f"{authority}.key", "-set_serial", str(serial), "-extfile",
                config.name, *dates, "-out", f"{name}.pem")

    old = ("-not_before", "200101000000Z", "-not_after", "200102000000Z")
    ca("ca")
    ca("other-ca")
    san = {
        "wrong-host": "DNS:wrong-name.invalid",
        "proxy-wrong-host": "DNS:wrong-name.invalid",
        "proxy-ip": "IP:127.0.0.1",
    }.get(profile, "DNS:outgoing-fixture.invalid")
    certificate("server", "ca", 1, f"subjectAltName={san}\nextendedKeyUsage=serverAuth\n",
                old if profile == "expired" else ("-days", "2"))
    if profile in ("mutual", "proxy-mutual"):
        certificate("client", "ca", 2, "extendedKeyUsage=clientAuth\n")
        certificate("other-client", "other-ca", 3, "extendedKeyUsage=clientAuth\n")
        certificate("expired-client", "ca", 4, "extendedKeyUsage=clientAuth\n", old)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    receiver.tls_names = []

    def store_sni(_, name, __):
        receiver.tls_names.append(name)

    context.set_servername_callback(store_sni)
    context.load_cert_chain(folder / "server.pem", folder / "server.key")
    if profile in ("mutual", "proxy-mutual"):
        context.verify_mode = ssl.CERT_REQUIRED
        context.load_verify_locations(folder / "ca.pem")
    receiver.socket = context.wrap_socket(receiver.socket, server_side=True)


def main():
    binary = routes["args"].binary.resolve()
    driver, url = helpers["database_settings"]()
    receiver = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Receiver)
    receiver.mutex = threading.Lock()
    receiver.received = {}
    receiver.unexpected = []
    receiver.client_certificates = {}
    receiver.entered = {name: threading.Event() for name in ("crash", "concurrent", "stale", "timeout", "endpoint-inflight")}
    receiver.release = {name: threading.Event() for name in receiver.entered}
    peer = threading.Thread(target=receiver.serve_forever, daemon=True)
    proxy = proxy_thread = None
    try:
        with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="webhook-send-") as root:
            profile = os.environ.get("FIXTURE_SEND_TLS")
            if profile:
                tls = Path(root) / "tls"
                tls.mkdir(mode=0o700)
                configure_tls(receiver, tls, profile)
            if profile and profile.startswith("proxy"):
                proxy = socketserver.ThreadingTCPServer(("127.0.0.1", 0), SocksProxy)
                proxy.expected = ("127.0.0.1", receiver.server_port)
                proxy.targets = []
                proxy.rejected = 0
                receiver.proxy = proxy
                proxy_thread = threading.Thread(target=proxy.serve_forever, daemon=True)
                proxy_thread.start()
            peer.start()
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
                "FIXTURE_JOB_HOOK_LOG": str(Path(root) / "job-hooks.jsonl"),
                "HTTP_PROXY": "http://127.0.0.1:1", "HTTPS_PROXY": "http://127.0.0.1:1", "NO_PROXY": "",
            }
            if profile:
                env["FIXTURE_SEND_TLS_DIR"] = str(tls)
                env["FIXTURE_SEND_TARGET"] = env["FIXTURE_SEND_TARGET"].replace("http:", "https:", 1)
                if profile == "proxy-ip":
                    env["FIXTURE_SEND_TARGET"] = env["FIXTURE_SEND_TARGET"].replace("outgoing-fixture.invalid", "127.0.0.1")
            if proxy:
                env["FIXTURE_SEND_PROXY_ADDRESS"] = f"127.0.0.1:{proxy.server_address[1]}"
            if url:
                env["DB_URL"] = url
            else:
                env.pop("DB_URL", None)
            if redis_url := env.get("OPENRUST_TEST_WEBHOOK_REDIS_URL"):
                assert env.get("FIXTURE_SEND_SCHEDULE") and env.get("OPENRUST_TEST_WEBHOOK_REDIS_CONTAINER")
                env |= {"FIXTURE_SEND_QUEUE": "redis", "REDIS_URL": redis_url,
                        "REDIS_PREFIX": Path(root).name + ":"}
                env.pop("REDIS_USERNAME", None)
                env.pop("REDIS_PASSWORD", None)

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
        if not (os.environ.get("FIXTURE_SEND_SCHEDULE") or os.environ.get("FIXTURE_SEND_REPLAY")
                or os.environ.get("FIXTURE_SEND_ENDPOINTS") or os.environ.get("FIXTURE_SEND_LIMITS")
                or os.environ.get("FIXTURE_SEND_FANOUT") or os.environ.get("FIXTURE_SEND_TLS")
                or os.environ.get("FIXTURE_SEND_DIAGNOSTICS")):
            print(f"{driver}: HTTP 签名和字节、事务回滚、重复领取、重试预算、撤销、超时、崩溃恢复、Builder 分支、投递事件与自定义 Job 通过")
    finally:
        if proxy_thread is not None:
            proxy.shutdown()
            proxy_thread.join(timeout=5)
            proxy.server_close()
        if peer.ident is not None:
            receiver.shutdown()
            peer.join(timeout=5)
        receiver.server_close()


if __name__ == "__main__":
    main()
