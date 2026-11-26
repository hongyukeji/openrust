"""用隔离 HTTP 接收器实测持久投递、Artisan 和独立 Worker。"""
import base64
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
        if (key in ("concurrent", "crash", "stale", "timeout", "endpoint-inflight")
                or key.startswith("slot-hold-")) and attempt == 1:
            owner.entered[key].set()
            if not owner.release[key].wait(timeout=12):
                return
        if key == "transport-disconnected":
            self.connection.shutdown(socket.SHUT_RDWR)
            self.close_connection = True
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
        if key == "transport-large-response":
            status = 200
        self.send_response(status)
        if key == "retry" and attempt == 1:
            self.send_header("Retry-After", "15")
        if key == "retry-date" and attempt == 1:
            self.send_header("Retry-After", email.utils.formatdate(owner.retry_at, usegmt=True))
        if key == "redirect":
            self.send_header("Location", "/unapproved")
        response_body = b"fixture-remote-response" if key.startswith("custom-") and status == 503 else b""
        if key == "transport-large-response":
            response_body = b"fixture-private-response" * 3000
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

    def submit(key, mode="normal", body=BODY, expected=202, current=env):
        connection = http.client.HTTPConnection("127.0.0.1", int(current["APP_PORT"]), timeout=20)
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

    def inspect_transport(identifier, number, scope=SCOPE):
        return json.loads(command("webhook-server:inspect", identifier, "--scope", scope,
                                  f"--transport={number}"))

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
                ("proxy-map-https", "proxy-map", "succeeded"),
                ("proxy-map-configured", "proxy-map-config", "succeeded"),
                ("proxy-map-bypass", "proxy-map-excluded", "succeeded"),
                ("proxy-map-missing-scheme", "proxy-map-missing-https", "succeeded"),
                ("proxy-map-not-authorized", "proxy-map-unregistered", "rejected"),
            ],
            "proxy-ip": [
                ("proxy-good", "proxy", "succeeded"),
                ("proxy-map-ip-literal", "proxy-map-ip-exclusion", "succeeded"),
                ("tls-wrong-ca", "proxy", "unknown"),
            ],
            "proxy-mutual": [
                ("tls-client", "proxy", "succeeded"),
                ("tls-no-client", "proxy", "unknown"),
            ],
            "proxy-wrong-host": [("tls-ca", "proxy", "unknown")],
            "wrong-host": [("tls-ca", "normal", "unknown")],
            "expired": [("tls-ca", "normal", "unknown")],
        }[profile]
        proxy_tls = env.get("FIXTURE_SEND_PROXY_PROTOCOL") == "connect-tls"
        if proxy_tls and profile == "proxy":
            cases += [
                ("proxy-tls-wrong-ca", "proxy", "unknown"),
                ("proxy-tls-wrong-host", "proxy", "unknown"),
                ("tls-wrong-ca", "proxy", "unknown"),
            ]
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
            if mode in ("proxy", "proxy-map", "proxy-map-config", "proxy-map-unregistered"):
                reference = "fixture-missing" if mode == "proxy-map-unregistered" else "fixture-egress"
                assert json.loads(snapshot)["proxy"] == reference
                assert "unused-egress" not in snapshot and "unused-egress" not in queue
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
            destination = (receiver.server_address[0], receiver.server_port)
            proxy = receiver.proxy
            target_count = 2 if profile in ("proxy", "proxy-ip", "proxy-mutual") else 1
            if proxy_tls and profile == "proxy":
                target_count += 1  # Target TLS rejects after CONNECT succeeds.
            if profile == "proxy":
                target_count += 2  # Builder and object configuration both select the HTTPS route.
            assert proxy.targets == [destination] * target_count, proxy.targets
            assert proxy.rejected == (1 if profile == "proxy" else 0)
            if proxy_tls:
                names = ["proxy-fixture.invalid"] * (target_count + proxy.rejected)
                if profile == "proxy":
                    names.extend(["proxy-fixture.invalid", "wrong-proxy.invalid"])
                assert sorted(proxy.tls_names) == sorted(names), proxy.tls_names
                assert proxy.client_certificates == [None] * (target_count + proxy.rejected)
        protocol = env.get("FIXTURE_SEND_PROXY_PROTOCOL", "socks5") if profile.startswith("proxy") else "direct"
        network = "IPv6" if env.get("FIXTURE_SEND_IPV6") == "1" else "IPv4"
        print(f"{driver}: TLS {profile} {protocol} {network} HTTP、Artisan、Worker 及固定连接通过")
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

    if env.get("FIXTURE_SEND_HISTORY"):
        def db(statement):
            routes["write_database"](driver, sql, statement)

        def prune(*args, scope=SCOPE, success=True):
            output = command("webhook-server:prune", "--scope", scope, *args, success=success)
            return json.loads(output) if success else None

        def hold(identifier, *args):
            command("webhook-server:hold", identifier, "--scope", SCOPE, *args)

        def inspect_history(identifier, *args):
            return json.loads(command("webhook-server:inspect", identifier, "--scope", SCOPE, *args))

        direct = submit("history-direct")
        retained = submit("history-hold")
        original = submit("replay-failed-history", "fail-once")
        enqueue(direct)
        db("INSERT INTO jobs(queue,payload,attempts,available_at,created_at) "
           "SELECT 'webhook-stale',payload,attempts,available_at,created_at FROM jobs WHERE queue='webhook-outgoing'")
        command(*worker)
        for identifier in (retained, original):
            enqueue(identifier)
            command(*worker)
        result = command("webhook-server:replay", original, "--scope", SCOPE,
                         "--actor=fixture-operator", "--reason=test-history")
        replayed = json.loads(re.search(r"\{[^\n]+\}", result)[0])["delivery_id"]
        assert prune("--days=0", "--replay-audit")["candidates"] == 0
        enqueue(replayed)
        command(*worker)
        assert read(replayed) == "succeeded\t1"

        admin("create", "primary")
        admin("subscribe", "primary", "--topic=invoice.paid", "--schema=1")
        receipt = publish("history-fanout")
        fanout = receipt["delivery_ids"][0]
        message = receipt["message_id"]
        enqueue(fanout)
        command(*worker)
        assert read(fanout) == "succeeded\t1"
        for identifier in (direct, retained, original, replayed, fanout):
            update(identifier, "deadline=1")
        assert prune("--days=0")["pruned"] == 5
        for identifier in (direct, retained, original, replayed, fanout):
            update(identifier, "payload_pruned_at=1")
        assert prune("--days=1", "--idempotency=delivery")["expired"] == 5
        assert prune("--days=1", "--replay-audit")["candidates"] == 0
        assert prune("--days=1", "--history=delivery")["candidates"] == 0
        for identifier in (direct, retained, original, replayed, fanout):
            update(identifier, "idempotency_expired_at=1")
            db(f"UPDATE webhook_delivery_attempts SET finished_at=1 WHERE delivery_id='{identifier}'")
        assert prune("--days=0", "--history=delivery")["scanned"] == 0
        hold(retained)
        assert prune("--days=1", "--attempts")["pruned"] == 4
        assert prune("--days=1", "--history=delivery", scope="other-tenant")["scanned"] == 0
        assert prune("--days=1", "--history=delivery", "--after=bad", success=False) is None
        first = prune("--days=1", "--history=delivery", "--limit=1", "--pretend")
        assert first["scanned"] == 1 and first["next_after"] == min(direct, fanout)
        second = prune("--days=1", "--history=delivery", "--limit=1", "--pretend",
                       f"--after={first['next_after']}")
        assert second["scanned"] == 1 and second["next_after"] is None
        assert first["candidates"] + second["candidates"] == 1
        assert read(direct) == "succeeded\t1"
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            jobs = [pool.submit(prune, "--days=1", "--history=delivery") for _ in range(2)]
            assert sum(job.result()["deleted"] for job in jobs) == 1
        assert inspect_history(direct) is None and inspect_history(retained)["retention_hold_at"]
        assert all(read(identifier) for identifier in (original, replayed, fanout))
        assert sql(f"SELECT count(*) FROM webhook_delivery_replays WHERE original_id='{original}'") == "1"

        renewed = submit("history-direct")
        assert renewed != direct
        command("queue:work", "database", "--queue=webhook-stale", "--once", "--tries=7")
        assert read(renewed) == "pending\t0"
        captured("history-direct")
        assert prune("--days=1", "--history=publication")["deleted"] == 0
        db(f"UPDATE webhook_messages SET created_at=1 WHERE id='{message}'")
        assert prune("--days=1", "--messages")["pruned"] == 1
        assert prune("--days=1", "--history=delivery")["deleted"] == 1
        assert inspect_history(fanout) is None
        assert inspect_history(message, "--message")["delivery_ids"] == [fanout]
        db(f"UPDATE webhook_messages SET body_pruned_at=1 WHERE id='{message}'")
        assert prune("--days=1", "--idempotency=publication")["expired"] == 1
        db(f"UPDATE webhook_messages SET idempotency_expired_at=1 WHERE id='{message}'")
        hold(message, "--message")
        assert prune("--days=1", "--history=publication")["deleted"] == 0
        hold(message, "--message", "--release")
        assert prune("--days=1", "--history=publication")["deleted"] == 1
        assert inspect_history(message, "--message") is None
        assert publish("history-fanout")["message_id"] != message
        hold(retained, "--release")
        assert prune("--days=1", "--attempts")["pruned"] == 1
        hold(retained)
        assert prune("--days=1", "--history=delivery")["deleted"] == 0
        hold(retained, "--release")
        assert prune("--days=1", "--history=delivery")["deleted"] == 1
        assert prune("--days=1", "--history=delivery")["scanned"] == 0
        assert prune("--days=1", "--replay-audit", scope="unknown-scope")["candidates"] == 0
        assert prune("--days=1", "--replay-audit", "--history=delivery", success=False) is None
        assert prune("--days=1", "--replay-audit", f"--after={replayed}", success=False) is None
        assert prune("--days=1", "--replay-audit", "--limit=1", "--pretend") == {
            "candidates": 1, "deleted": 0, "pretend": True,
        }
        for identifier in (original, replayed):
            hold(identifier)
            assert prune("--days=1", "--replay-audit")["deleted"] == 0
            assert prune("--days=1", "--history=delivery")["deleted"] == 0
            hold(identifier, "--release")

        marker = Path(env["APP_BASE_PATH"])
        holder = subprocess.Popen(
            [str(binary), "artisan", "webhooks:fixture-endpoint", "hold-before-prune", original],
            env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        cleaner = None
        try:
            deadline = time.monotonic() + 6
            while not (marker / "prune-hold-ready").exists():
                assert holder.poll() is None and time.monotonic() < deadline
                time.sleep(0.01)
            cleaner = subprocess.Popen(
                [str(binary), "artisan", "webhook-server:prune", "--scope", SCOPE, "--days=1", "--replay-audit"],
                env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            time.sleep(0.2)
            assert cleaner.poll() is None
            (marker / "prune-hold-release").touch()
            _, error = holder.communicate(timeout=10)
            assert holder.returncode == 0, error
            output, error = cleaner.communicate(timeout=10)
            assert cleaner.returncode == 0 and json.loads(output)["deleted"] == 0, error
        finally:
            for process in (holder, cleaner):
                if process is not None and process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)
        assert sql(f"SELECT count(*) FROM webhook_delivery_replays WHERE original_id='{original}'") == "1"
        hold(original, "--release")
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            jobs = [pool.submit(prune, "--days=1", "--replay-audit") for _ in range(2)]
            assert sum(job.result()["deleted"] for job in jobs) == 1
        assert inspect_history(original) and inspect_history(replayed)
        assert prune("--days=1", "--replay-audit")["candidates"] == 0
        command("webhook-server:replay", original, "--scope", SCOPE, "--actor=fixture-operator",
                "--reason=test-history-after-audit-pruned", success=False)
        assert prune("--days=1", "--history=delivery")["deleted"] == 2
        assert inspect_history(original) is None and inspect_history(replayed) is None
        print(f"{driver}: 历史元数据、重放审计、留存保护、并发清理及旧队列隔离通过")
        return

    if env.get("FIXTURE_SEND_KEY_EXPIRY"):
        def db(statement):
            routes["write_database"](driver, sql, statement)

        def prune(*args, scope=SCOPE, success=True):
            output = command("webhook-server:prune", "--scope", scope, *args, success=success)
            return json.loads(output) if success else None

        def hold(identifier, *args):
            command("webhook-server:hold", identifier, "--scope", SCOPE, *args)

        def finish(identifier):
            enqueue(identifier)
            command(*worker)
            assert read(identifier) == "succeeded\t1"
            update(identifier, "deadline=1")

        scope_hash = hashlib.sha256(SCOPE.encode()).hexdigest()
        pending = submit("expiry-pending")
        old = submit("expiry-direct")
        enqueue(old)
        db("INSERT INTO jobs(queue,payload,attempts,available_at,created_at) "
           "SELECT 'webhook-stale',payload,attempts,available_at,created_at FROM jobs WHERE queue='webhook-outgoing'")
        command(*worker)
        assert read(old) == "succeeded\t1"
        update(old, "deadline=1")

        legacy = submit("expiry-legacy")
        db(f"DELETE FROM webhook_idempotency_keys WHERE record_id='{legacy}'")
        old_legacy = hashlib.sha256(f"{scope_hash}:expiry-legacy".encode()).hexdigest()
        db(f"UPDATE webhook_deliveries SET id='{old_legacy}' WHERE id='{legacy}'")
        finish(old_legacy)
        assert prune("--days=0", "--idempotency=delivery")["candidates"] == 0
        assert prune("--days=0")["pruned"] == 2
        assert prune("--days=1", "--idempotency=delivery")["candidates"] == 0
        for identifier in (old, old_legacy):
            update(identifier, "payload_pruned_at=1")
        assert prune("--days=0", "--idempotency=delivery", "--pretend", "--limit=1") == {
            "kind": "delivery", "candidates": 1, "expired": 0, "pretend": True
        }
        assert prune("--days=0", "--idempotency=delivery", scope="other-scope")["candidates"] == 0
        assert prune("--days=0", "--idempotency=delivery", "--messages", success=False) is None
        assert submit("expiry-direct") == old
        hold(old)
        assert prune("--days=0", "--idempotency=delivery")["expired"] == 1
        assert submit("expiry-direct") == old
        legacy_replacement = submit("expiry-legacy")
        assert legacy_replacement != old_legacy and read(old_legacy) == "succeeded\t1"
        hold(old, "--release")
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            jobs = [pool.submit(prune, "--days=0", "--idempotency=delivery") for _ in range(2)]
            assert sum(job.result()["expired"] for job in jobs) == 1
        with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
            replacements = [job.result() for job in
                            [pool.submit(submit, "expiry-direct", body=b"new body") for _ in range(3)]]
        new = replacements[0]
        assert replacements == [new] * 3 and new != old and new != pending
        command("queue:work", "database", "--queue=webhook-stale", "--once", "--tries=7")
        assert read(old) == "succeeded\t1" and read(new) == "pending\t0"
        assert sql("SELECT count(*) FROM jobs WHERE queue='webhook-stale'") == "0"
        captured("expiry-direct")
        finish(new)
        with receiver.mutex:
            assert [entry[2] for entry in receiver.received["expiry-direct"]] == [BODY, b"new body"]
        inspected = json.loads(command("webhook-server:inspect", old, "--scope", SCOPE))
        assert inspected["idempotency_expired_at"] > 0 and inspected["attempts"] == 1

        old_publish = publish("expiry-publish")
        old_message = hashlib.sha256(f"webhook-publication:{scope_hash}:expiry-publish".encode()).hexdigest()
        assert old_publish["delivery_ids"] == []
        record = old_publish["message_id"]
        db(f"DELETE FROM webhook_idempotency_keys WHERE record_id='{record}'")
        db(f"UPDATE webhook_messages SET id='{old_message}', created_at=1 WHERE id='{record}'")
        assert prune("--days=0", "--messages")["pruned"] == 1
        assert prune("--days=1", "--idempotency=publication")["expired"] == 0
        db(f"UPDATE webhook_messages SET body_pruned_at=1 WHERE id='{old_message}'")
        prune("--days=0", "--idempotency=publication")
        new_publish = publish("expiry-publish")
        assert new_publish["message_id"] != old_message
        assert publish("expiry-publish") == new_publish
        old_summary = json.loads(command("webhook-server:inspect", old_message, "--scope", SCOPE, "--message"))
        assert old_summary["idempotency_expired_at"] > 0 and old_summary["body_pruned_at"] > 0

        admin("create", "primary")
        admin("subscribe", "primary", "--topic=invoice.paid", "--schema=1")
        fanout = publish("expiry-fanout")
        finish(fanout["delivery_ids"][0])
        assert prune("--days=0")["pruned"] >= 1
        db(f"UPDATE webhook_messages SET created_at=1 WHERE id='{fanout['message_id']}'")
        assert prune("--days=0", "--messages")["pruned"] >= 1
        db(f"UPDATE webhook_messages SET body_pruned_at=1 WHERE id='{fanout['message_id']}'")
        hold(fanout["message_id"], "--message")
        assert prune("--days=1", "--idempotency=publication")["expired"] == 0
        assert publish("expiry-fanout") == fanout
        hold(fanout["message_id"], "--message", "--release")
        assert prune("--days=1", "--idempotency=publication")["expired"] == 1
        replacement = publish("expiry-fanout", {"new": True})
        assert replacement["message_id"] != fanout["message_id"]
        assert replacement["delivery_ids"][0] != fanout["delivery_ids"][0]
        finish(replacement["delivery_ids"][0])
        migration = ("--path=vendor/openrust/webhook-server/database/migrations/"
                     "2026_11_25_000000_mark_webhook_deliveries_idempotency_expiry.rs")
        command("migrate:rollback", migration, "--step=1000", "--force", success=False)
        print(f"{driver}: 幂等期限、旧键回退、业务保留、并发及旧 Worker 隔离通过")
        return

    if env.get("FIXTURE_SEND_KEY_BINDINGS"):
        legacy = json.loads(env["FIXTURE_SEND_LEGACY_RECEIPTS"])
        scope_hash = hashlib.sha256(SCOPE.encode()).hexdigest()
        old_delivery = hashlib.sha256(f"{scope_hash}:legacy-submission".encode()).hexdigest()
        old_message = hashlib.sha256(f"webhook-publication:{scope_hash}:legacy-message".encode()).hexdigest()
        assert legacy[0]["id"] == old_delivery and legacy[1]["message_id"] == old_message
        assert sql("SELECT count(*) FROM webhook_idempotency_keys") == "0"
        with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
            deliveries = [pool.submit(submit, "legacy-submission") for _ in range(3)]
            publications = [pool.submit(publish, "legacy-message") for _ in range(3)]
            assert [job.result() for job in deliveries] == [old_delivery] * 3
            assert [job.result() for job in publications] == [legacy[1]] * 3
        assert sql("SELECT count(*) FROM webhook_idempotency_keys") == "2"
        submit("legacy-submission", body=b"changed", expected=409)
        publish("legacy-message", {"different": True}, expected=409)

        enqueue(old_delivery)
        enqueue(legacy[1]["delivery_ids"][0])
        command(*worker)
        command(*worker)
        captured("legacy-submission")
        assert read(legacy[1]["delivery_ids"][0]) == "succeeded\t1"
        with receiver.mutex:
            legacy_payload = receiver.received[old_message][0][2]
        assert json.loads(legacy_payload)["data"] == {"status": "retry"}

        rollback = [submit("identity-rollback", mode="rollback") for _ in range(2)]
        assert len(set(rollback)) == 2 and all(not read(identifier) for identifier in rollback)
        association = f"SELECT count(*) FROM webhook_idempotency_keys WHERE record_id IN ('{rollback[0]}','{rollback[1]}')"
        assert sql(association) == "0"
        with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
            ids = list(pool.map(submit, ["new-submission"] * 6))
        assert len(set(ids)) == 1
        fresh = ids[0]
        assert fresh != hashlib.sha256(f"{scope_hash}:new-submission".encode()).hexdigest()
        assert fresh not in rollback
        assert sql(f"SELECT count(*) FROM webhook_idempotency_keys WHERE record_id='{fresh}'") == "1"
        renewed = publish("new-message")
        assert renewed["message_id"] != hashlib.sha256(
            f"webhook-publication:{scope_hash}:new-message".encode()).hexdigest()
        assert renewed == publish("new-message")
        assert renewed["delivery_ids"] != legacy[1]["delivery_ids"]
        assert all(read(identifier) for identifier in renewed["delivery_ids"])
        migration = ("--path=vendor/openrust/webhook-server/database/migrations/"
                     "2026_11_25_000000_create_webhook_idempotency_keys_table.rs")
        command("migrate:rollback", migration, "--step=1000", "--force", success=False)
        print(f"{driver}: 旧版升级、幂等映射、并发首写、原事务回滚和稳定 HTTP 收据通过")
        return

    if env.get("FIXTURE_SEND_RETENTION"):
        def prune(*args, scope=SCOPE, success=True):
            output = command("webhook-server:prune", "--scope", scope, *args, success=success)
            return json.loads(output) if success else None

        def hold(identifier, *args, scope=SCOPE, success=True):
            output = command("webhook-server:hold", identifier, "--scope", scope, *args, success=success)
            return json.loads(output)["changed"] if success else None

        def replay_args(identifier):
            return ("webhook-server:replay", identifier, "--scope", SCOPE,
                    "--actor=fixture-operator", "--reason=test-retention")

        def age(identifier):
            update(identifier, "deadline=1")
            routes["write_database"](driver, sql,
                f"UPDATE webhook_delivery_attempts SET finished_at=2 WHERE delivery_id='{identifier}'")

        winner = submit("replay-failed-retention-wins", "fail-once")
        enqueue(winner)
        command(*worker)
        assert read(winner) == "failed\t1"
        loser = submit("replay-failed-retention-loses", "fail-once")
        enqueue(loser)
        command(*worker)
        assert read(loser) == "failed\t1"
        pending = submit("retry")
        enqueue(pending)
        command(*worker)
        assert read(pending) == "pending\t1"
        for identifier in (winner, loser, pending):
            age(identifier)

        assert not hold(winner, scope="different-scope")
        assert hold("invalid", success=False) is None
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            changes = [pool.submit(hold, winner) for _ in range(2)]
            assert sum(change.result() for change in changes) == 1
        held = json.loads(command("webhook-server:inspect", winner, "--scope", SCOPE))
        assert held["retention_hold_at"] > 0
        assert prune("--days=0", "--attempts", "--pretend")["candidates"] == 1
        assert hold(loser)
        assert prune("--days=0", "--attempts")["pruned"] == 0
        assert prune("--days=0")["pruned"] == 0
        assert hold(loser, "--release")
        marker = Path(env["APP_BASE_PATH"])
        holder = subprocess.Popen(
            [str(binary), "artisan", "webhooks:fixture-endpoint", "hold-before-prune", loser],
            env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        cleaner = None
        try:
            deadline = time.monotonic() + 6
            while not (marker / "prune-hold-ready").exists():
                assert holder.poll() is None and time.monotonic() < deadline
                time.sleep(0.01)
            cleaner = subprocess.Popen(
                [str(binary), "artisan", "webhook-server:prune", "--scope", SCOPE, "--days=0", "--attempts"],
                env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            time.sleep(0.2)
            assert cleaner.poll() is None
            (marker / "prune-hold-release").touch()
            _, error = holder.communicate(timeout=10)
            assert holder.returncode == 0, error
            output, error = cleaner.communicate(timeout=10)
            assert cleaner.returncode == 0 and json.loads(output)["pruned"] == 0, error
            assert bool(attempts(loser))
        finally:
            for process in (holder, cleaner):
                if process is not None and process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)
        assert hold(loser, "--release")
        hold_migration = ("--path=vendor/openrust/webhook-server/database/migrations/"
                          "2026_11_25_000000_mark_webhook_deliveries_retention_hold.rs")
        command("migrate:rollback", hold_migration, "--step=1000", "--force", success=False)
        assert hold(winner, "--release") and not hold(winner, "--release")
        assert prune("--days=0", "--limit=1", "--pretend") == {
            "artifact": "payloads", "pretend": True, "candidates": 1, "pruned": 0
        }
        assert prune("--days=0", scope="different-scope")["candidates"] == 0
        assert prune("--days=36501", success=False) is None
        assert prune("--days=0", "--limit=501", success=False) is None

        migration = ("--path=vendor/openrust/webhook-server/database/migrations/"
                     "2026_11_25_000000_extend_webhook_deliveries_for_retention.rs")
        expiry_migration = ("--path=vendor/openrust/webhook-server/database/migrations/"
                            "2026_11_25_000000_mark_webhook_deliveries_idempotency_expiry.rs")
        command("migrate:rollback", migration, "--step=1000", "--force", success=False)
        command("migrate:rollback", expiry_migration, "--step=1000", "--force")
        assert "extend_webhook_deliveries_for_retention" in command("migrate:rollback", migration, "--step=1000", "--force")
        assert "extend_webhook_deliveries_for_retention" in command("migrate", migration, "--force")
        command("migrate", expiry_migration, "--force")

        saved = command(*replay_args(winner))
        child = json.loads(re.search(r"\{[^\n]+\}", saved)[0])["delivery_id"]
        marker = Path(env["APP_BASE_PATH"])
        replay = subprocess.Popen(
            [str(binary), "artisan", *replay_args(loser)],
            env=env | {"FIXTURE_REPLAY_WAIT_FOR_PRUNE": "1"}, text=True,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        try:
            deadline = time.monotonic() + 6
            while not (marker / "retention-replay-ready").exists():
                assert replay.poll() is None and time.monotonic() < deadline
                time.sleep(0.01)
            assert prune("--days=0")["pruned"] == 2
            (marker / "retention-replay-release").touch()
            _, error = replay.communicate(timeout=10)
            assert replay.returncode != 0 and "暂不能重放" in error, error
        finally:
            if replay.poll() is None:
                replay.kill()
                replay.wait(timeout=5)
        assert sql(f"SELECT count(*) FROM webhook_delivery_replays WHERE original_id='{loser}'") == "0"
        assert sql(f"SELECT count(*) FROM webhook_deliveries WHERE id IN ('{winner}','{loser}') And snapshot='' AND payload_pruned_at IS NOT NULL") == "2"
        assert sql(f"SELECT count(*) FROM webhook_deliveries WHERE id='{pending}' AND payload_pruned_at IS NULL") == "1"
        assert submit("replay-failed-retention-loses", "fail-once") == loser
        submit("replay-failed-retention-loses", "fail-once", b"changed", expected=409)
        assert prune("--days=0")["candidates"] == 0
        command(*replay_args(loser), success=False)

        enqueue(child)
        command(*worker)
        assert read(child) == "succeeded\t1"
        captured("replay-failed-retention-wins", 2, rotated=(1,))
        tombstone = json.loads(command("webhook-server:inspect", loser, "--scope", SCOPE))
        assert tombstone["payload_pruned_at"] > 0 and tombstone["attempts"] == 1
        assert prune("--days=1", "--attempts", "--pretend")["candidates"] == 2
        assert prune("--days=1", "--attempts", "--limit=1")["pruned"] == 1
        assert prune("--days=1", "--attempts")["pruned"] == 1
        assert bool(attempts(child)) and bool(attempts(pending))
        assert not attempts(winner) and not attempts(loser)
        assert prune("--days=1", "--attempts")["pruned"] == 0

        empty = publish("retention-empty")
        assert empty["delivery_ids"] == []
        admin("create", "primary")
        admin("subscribe", "primary", "--topic=invoice.paid", "--schema=1")
        published = publish("retention-fanout")
        message_id = published["message_id"]
        fanout = published["delivery_ids"][0]
        message_migration = ("--path=vendor/openrust/webhook-server/database/migrations/"
                             "2026_11_25_000000_extend_webhook_messages_for_retention.rs")
        expiry_migration = ("--path=vendor/openrust/webhook-server/database/migrations/"
                            "2026_11_25_000000_mark_webhook_messages_idempotency_expiry.rs")
        command("migrate:rollback", message_migration, "--step=1000", "--force", success=False)
        command("migrate:rollback", expiry_migration, "--step=1000", "--force")
        assert "extend_webhook_messages_for_retention" in command("migrate:rollback", message_migration, "--step=1000", "--force")
        assert "extend_webhook_messages_for_retention" in command("migrate", message_migration, "--force")
        command("migrate", expiry_migration, "--force")
        for receipt in (empty, published):
            routes["write_database"](driver, sql,
                f"UPDATE webhook_messages SET created_at=1 WHERE id='{receipt['message_id']}'")
        assert prune("--days=0", "--messages", scope="different-scope")["scanned"] == 0
        assert prune("--days=0", "--messages", "--after=bad", success=False) is None
        assert prune("--days=0", "--after=" + message_id, success=False) is None
        first = prune("--days=0", "--messages", "--limit=1", "--pretend")
        cursor = first["next_after"]
        second = prune("--days=0", "--messages", "--limit=1", "--pretend", f"--after={cursor}")
        assert cursor == min(empty["message_id"], message_id) and second["next_after"] is None
        assert first["scanned"] == second["scanned"] == 1
        assert first["pruned"] == second["pruned"] == 0
        assert first["candidates"] + second["candidates"] == 1
        assert hold(empty["message_id"], "--message")
        message_hold_migration = ("--path=vendor/openrust/webhook-server/database/migrations/"
                                  "2026_11_25_000000_mark_webhook_messages_retention_hold.rs")
        command("migrate:rollback", message_hold_migration, "--step=1000", "--force", success=False)
        assert prune("--days=0", "--messages")["pruned"] == 0
        held = json.loads(command("webhook-server:inspect", empty["message_id"],
                                  "--scope", SCOPE, "--message"))
        assert held["retention_hold_at"] > 0
        assert hold(empty["message_id"], "--message", "--release")
        assert prune("--days=0", "--messages")["pruned"] == 1
        assert publish("retention-empty") == empty
        assert sql(f"SELECT count(*) FROM webhook_messages WHERE id='{message_id}' AND body<>''") == "1"

        enqueue(fanout)
        command(*worker)
        assert read(fanout) == "succeeded\t1"
        assert prune("--days=0", "--messages")["candidates"] == 0
        replay_env = env | {"FIXTURE_REPLAY_KEEP_ORIGINAL": "1"}
        saved = command("webhook-server:replay", fanout, "--scope", SCOPE,
                        "--actor=fixture-supervisor", "--reason=test-retention-fanout", "--include-succeeded",
                        current=replay_env)
        replayed_fanout = json.loads(re.search(r"\{[^\n]+\}", saved)[0])["delivery_id"]
        age(fanout)
        assert prune("--days=0")["pruned"] == 1
        assert prune("--days=0", "--messages")["candidates"] == 0
        enqueue(replayed_fanout)
        command(*worker, current=replay_env)
        assert read(replayed_fanout) == "succeeded\t1"
        with receiver.mutex:
            original, replayed = receiver.received[message_id]
        assert original[2] == replayed[2]
        assert replayed[1]["Signature"] == hmac.new(SECRET, original[2], hashlib.sha256).hexdigest()
        assert prune("--days=0", "--messages")["candidates"] == 0
        age(replayed_fanout)
        assert hold(replayed_fanout)
        report = prune("--days=0")
        assert report["candidates"] == report["pruned"] == 0
        assert prune("--days=0", "--messages")["candidates"] == 0
        assert hold(replayed_fanout, "--release")
        assert prune("--days=0")["pruned"] == 1
        assert hold(message_id, "--message")
        assert prune("--days=0", "--messages")["pruned"] == 0
        assert hold(message_id, "--message", "--release")
        with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
            tasks = [pool.submit(prune, "--days=0", "--messages") for _ in range(2)]
            receipt = pool.submit(publish, "retention-fanout")
            assert sum(task.result()["pruned"] for task in tasks) == 1
            assert receipt.result() == published
        assert sql(f"SELECT count(*) FROM webhook_messages WHERE id='{message_id}' AND body='' AND body_pruned_at IS NOT NULL") == "1"
        assert publish("retention-fanout") == published
        publish("retention-fanout", {"different": True}, expected=409)
        inspected = json.loads(command("webhook-server:inspect", message_id, "--scope", SCOPE, "--message"))
        assert inspected["body_pruned_at"] > 0 and inspected["delivery_ids"] == [fanout]
        command("webhook-server:replay", fanout, "--scope", SCOPE, "--actor=fixture-supervisor",
                "--reason=test-retention-fanout", "--include-succeeded", success=False)
        command("migrate:rollback", migration, "--step=1000", "--force", success=False)
        command("migrate:rollback", message_migration, "--step=1000", "--force", success=False)
        assert prune("--days=0")["pruned"] == 0
        assert prune("--days=0", "--messages")["scanned"] == 0
        print(f"{driver}: 出站留存、共享正文、业务保留、并发回收和墓碑去重通过")
        return

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

    if env.get("FIXTURE_SEND_CONCURRENCY"):
        scope_hash = hashlib.sha256(SCOPE.encode()).hexdigest()
        def charged():
            return sql(f"SELECT used FROM webhook_rate_buckets WHERE kind='scope' AND scope_key='{scope_hash}'")

        def reject_without_attempt(identifier, key):
            assert '"pending"' in send(identifier, current=peer)
            assert read(identifier) == "pending\t0" and not attempts(identifier)
            assert int(sql(f"SELECT available_at FROM webhook_deliveries WHERE id='{identifier}'")) > 0
            captured(key, 0)

        def holder(key, endpoint, node):
            identifier = submit(key, f"endpoint-{endpoint}", current=node)
            if key == "slot-hold-two":
                update(identifier, f"deadline={int(time.time()) + 15}")
            enqueue(identifier)
            process = subprocess.Popen([str(binary), "artisan", *worker], env=node,
                                       stdout=log, stderr=log, text=True)
            held.append(process)
            assert receiver.entered[key].wait(timeout=8)
            assert read(identifier) == "running\t1"
            slot, lease, deadline = map(int, sql(
                "SELECT a.send_lease_until,d.lease_until,d.deadline FROM webhook_delivery_attempts a "
                f"JOIN webhook_deliveries d ON a.delivery_id=d.id WHERE d.id='{identifier}'"
            ).split())
            assert slot == lease and slot > 0
            if key == "slot-hold-two":
                assert slot > deadline
            return identifier, process

        command("webhooks:fixture-endpoint", "create", "one")
        command("webhooks:fixture-endpoint", "create", "two")
        peer_root = Path(env["APP_BASE_PATH"]) / "concurrency-peer"
        peer_root.mkdir()
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        peer = env | {"APP_BASE_PATH": str(peer_root), "APP_PORT": str(port),
                      "APP_URL": f"http://127.0.0.1:{port}",
                      "APP_CONFIG_CACHE": str(peer_root / "config.json"),
                      "APP_ROUTES_CACHE": str(peer_root / "routes.json")}
        command("config:cache", current=peer)
        command("route:cache", current=peer)
        peer_http = http_helpers["serve"](binary, peer, log)
        held = []
        try:
            first, first_worker = holder("slot-hold-one", "one", env)
            same_endpoint = submit("slot-same-endpoint", "endpoint-one", current=peer)
            reject_without_attempt(same_endpoint, "slot-same-endpoint")
            assert charged() == "1"
            second, second_worker = holder("slot-hold-two", "two", peer)
            waiting = submit("slot-same-scope", current=peer)
            reject_without_attempt(waiting, "slot-same-scope")
            assert charged() == "2"
            isolated = submit("slot-other-scope", "limit-other-scope", current=peer)
            assert '"succeeded"' in send(isolated, scope="webhook-outbound-limit-alternate", current=peer)
            captured("slot-other-scope")
            assert charged() == "2"
            migration = ("--path=vendor/openrust/webhook-server/database/migrations/"
                         "2026_11_25_000001_extend_webhook_attempts_for_concurrency.rs")
            command("migrate:rollback", migration, "--step=1000", "--force", success=False)

            receiver.release["slot-hold-one"].set()
            assert first_worker.wait(timeout=8) == 0 and read(first) == "succeeded\t1"
            update(same_endpoint, "available_at=0")
            enqueue(same_endpoint)
            command(*worker, current=peer)
            assert read(same_endpoint) == "succeeded\t1" and charged() == "3"
            captured("slot-same-endpoint")

            second_worker.kill()
            second_worker.wait(timeout=8)
            receiver.release["slot-hold-two"].set()
            after_crash = submit("slot-after-crash", "endpoint-two", current=peer)
            reject_without_attempt(after_crash, "slot-after-crash")
            assert charged() == "3"
            update(second, "lease_until=0")
            routes["write_database"](driver, sql,
                f"UPDATE webhook_delivery_attempts SET send_lease_until=0 WHERE delivery_id='{second}'")
            update(after_crash, "available_at=0")
            assert '"succeeded"' in send(after_crash, current=peer)
            assert charged() == "4"
            captured("slot-after-crash")
            assert '"succeeded"' in send(second, current=peer)
            assert attempts(second) == "1\tunknown\t0\n2\tsucceeded\t204"
            captured("slot-hold-two", 2)
            update(waiting, "available_at=0")
            assert '"succeeded"' in send(waiting, current=peer)
            captured("slot-same-scope")
            assert sql("SELECT COUNT(*) FROM webhook_delivery_attempts WHERE send_lease_until IS NOT NULL AND finished_at IS NULL") == "0"
            assert "extend_webhook_attempts_for_concurrency" in command("migrate:rollback", migration, "--step=1000", "--force")
            assert "extend_webhook_attempts_for_concurrency" in command("migrate", migration, "--force")

            keys = ("slot-hold-race-one", "slot-hold-race-two")
            nodes = (env, peer)
            deliveries = [submit(key, "endpoint-one", current=node) for key, node in zip(keys, nodes)]
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
                futures = [executor.submit(send, identifier, current=node)
                           for identifier, node in zip(deliveries, nodes)]
                try:
                    complete, _ = concurrent.futures.wait(
                        futures, timeout=7, return_when=concurrent.futures.FIRST_COMPLETED)
                    assert len(complete) == 1, complete
                    declined = futures.index(next(iter(complete)))
                    winner = 1 - declined
                    assert '"pending"' in futures[declined].result()
                    assert receiver.entered[keys[winner]].wait(timeout=4)
                    assert not receiver.entered[keys[declined]].is_set()
                    assert read(deliveries[declined]) == "pending\t0" and not attempts(deliveries[declined])
                finally:
                    for key in keys:
                        receiver.release[key].set()
                assert '"succeeded"' in futures[winner].result()
            captured(keys[winner])
            captured(keys[declined], 0)
            update(deliveries[declined], "available_at=0")
            assert '"succeeded"' in send(deliveries[declined], current=nodes[declined])
            captured(keys[declined])
        finally:
            for key in ("slot-hold-one", "slot-hold-two"):
                receiver.release[key].set()
            for process in held:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=8)
            http_helpers["stop"](peer_http)
        print(f"{driver}: 两节点发送在途范围／端点上限、速率回滚、独立 Worker 崩溃后回收和迁移通过", flush=True)
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
        second = submit("recover-backlog-peer")
        oldest = int(sql(f"SELECT available_at FROM webhook_deliveries WHERE id='{registered}'")) - 1
        update(second, f"available_at={oldest}")
        assert recover("--pretend", "--backlog", "--limit=1") == {
            "candidates": 1, "oldest_available_at": oldest, "has_more": True,
        }
        assert recover("--pretend", "--backlog", "--limit=2") == {
            "candidates": 2, "oldest_available_at": oldest, "has_more": False,
        }
        empty_backlog = {"candidates": 0, "oldest_available_at": None, "has_more": False}
        assert recover("--pretend", "--backlog", scope="wrong-scope") == empty_backlog
        invalid = subprocess.run(
            [str(binary), "artisan", "webhook-server:recover", "--scope", SCOPE, "--backlog"],
            env=env, capture_output=True, text=True, timeout=20,
        )
        assert invalid.returncode != 0 and "--pretend" in invalid.stdout + invalid.stderr
        assert marker(registered) == "0" and queued_for(registered) == 0
        assert marker(second) == "0" and queued_for(second) == 0
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
        assert marker(second) != "0" and queued_for(second) == 1
        assert recover()["candidates"] == 0 and read(registered) == "pending\t0"
        assert recover("--pretend", "--backlog") == empty_backlog
        captured("recover-unenqueued", 0)
        command(*worker)
        command(*worker)
        assert read(registered) == "succeeded\t1"
        assert read(second) == "succeeded\t1"
        captured("recover-unenqueued")
        captured("recover-backlog-peer")

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
        assert recover("--pretend", "--backlog")["candidates"] == 1
        assert recover()["retired"] == 1 and read(failed_dispatch) == "expired\t0"
        captured("dispatch-listener-error", 0)

        broker = submit("broker-unavailable")
        unavailable = env | {
            "APP_CONFIG_CACHE": str(Path(env["APP_BASE_PATH"]) / "missing-broker-config.json"),
            "DB_QUEUE_TABLE": "outbound_acceptance_missing_queue",
        }
        if redis_container:
            unavailable["REDIS_URL"] = "redis://127.0.0.1:1"
        assert recover("--pretend", "--backlog", current=unavailable)["candidates"] == 1
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

    transport_migration = ("--path=vendor/openrust/webhook-server/database/migrations/"
                           "2026_11_25_000001_add_transport_to_webhook_delivery_attempts.rs")
    assert "add_transport_to_webhook_delivery_attempts" in command(
        "migrate:rollback", transport_migration, "--step=1000", "--force"
    )
    assert "add_transport_to_webhook_delivery_attempts" in command("migrate", transport_migration, "--force")

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
    for number in (1, 2):
        recorded_transport = inspect_transport(retry, number)
        assert recorded_transport["transfer_time_micros"] > 0
        assert recorded_transport["transport_failure"] is None
    captured("retry", 2)

    if legacy_binary := env.get("FIXTURE_SEND_TRANSPORT_LEGACY_BINARY"):
        prior = submit("transport-legacy-worker")
        enqueue(prior)
        routes["command"](Path(legacy_binary).resolve(), env, *worker)
        assert attempts(prior) == "1\tsucceeded\t204"
        assert inspect_transport(prior, 1) == {"transfer_time_micros": None, "transport_failure": None}
        captured("transport-legacy-worker")
        print(f"{driver}: 前版 Worker 读取新版本作业并完成投递，传输诊断保留为空", flush=True)

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
    assert inspect_transport(revoked, 1) == {"transfer_time_micros": None, "transport_failure": None}
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
    assert inspect_transport(timeout, 1)["transport_failure"] in {"deadline_exceeded", "timeout"}

    failed_transports = {}
    for key in ("transport-disconnected", "transport-large-response"):
        if key == "transport-disconnected":
            identifier = submit(key, "builder-custom-async")
        else:
            identifier = submit(key)
            enqueue(identifier)
        command(*worker)
        assert attempts(identifier) == "1\tunknown\t0"
        captured(key)
        failed_transports[key] = identifier
    disconnected = failed_transports["transport-disconnected"]
    oversized_response = failed_transports["transport-large-response"]
    assert inspect_transport(disconnected, 1) == {
        "transfer_time_micros": None, "transport_failure": "connection",
    }
    assert inspect_transport(oversized_response, 1) == {
        "transfer_time_micros": None, "transport_failure": "response_too_large",
    }
    assert inspect_transport(disconnected, 2) is None
    assert inspect_transport(disconnected, 1, "different-scope") is None
    command("webhook-server:inspect", disconnected, "--scope", SCOPE, "--transport=0", success=False)
    command("webhook-server:inspect", disconnected, "--scope", SCOPE, "--transport=1", "--attempts", success=False)
    command("migrate:rollback", transport_migration, "--step=1000", "--force", success=False)

    for scope, auth, status in ((SCOPE, True, 200), (SCOPE, False, 403), ("other-scope", True, 403)):
        connection = http.client.HTTPConnection("127.0.0.1", int(env["APP_PORT"]), timeout=20)
        try:
            headers = {"x-fixture-inspector": "fixture-operator"} if auth else {}
            connection.request("GET", f"/api/outbound-inspect/{scope}/transport/{disconnected}", headers=headers)
            response = connection.getresponse()
            assert response.status == status, response.status
            body = json.loads(response.read())
            assert body == (inspect_transport(disconnected, 1) if status == 200 else {})
        finally:
            connection.close()

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
        assert inspect_transport(stale, 1) == {"transfer_time_micros": None, "transport_failure": None}
        assert inspect_transport(stale, 2)["transfer_time_micros"] > 0
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
    for mode in ("builder-async-missing", "builder-async-unbound", "builder-sync-missing", "store-unbound-job"):
        submit(mode, mode, expected=422)
        captured(mode, 0)
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
        ("transport-disconnected", "should_remove"), ("transport-disconnected", "failed"),
        ("custom-stop-sync", "should_remove"), ("custom-stop-sync", "failed"),
        ("custom-stop-worker", "should_remove"), ("custom-stop-worker", "failed"),
        ("custom-continue", "should_remove"), ("custom-revoked", "failed"),
    ], hooks
    for hook in hooks:
        assert hook["scope"] == SCOPE and hook["number"] == 1 and hook["id"]
        assert hook["meta"] == {"trace": "fixture"} and hook["tags"] == ["audit"]
        assert hook["url"] == env["FIXTURE_SEND_TARGET"] and hook["request_matches"]
        if hook["message"] == "transport-disconnected":
            assert hook["outcome"] == "unknown" and hook["transport_failure"] == "Connection"
            assert hook["status"] is None and not hook["response_matches"]
        elif hook["message"] == "custom-revoked":
            assert hook["outcome"] == "rejected" and hook["status"] is None and not hook["response_matches"]
            assert hook["transport_failure"] is None
        else:
            assert hook["outcome"] == "http_failure" and hook["status"] == 503 and hook["response_matches"]
            assert hook["transport_failure"] is None

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
    assert queued[0]["transfer_seconds"] is None and queued[0]["transport_failure"] is None
    assert queued[1]["transfer_seconds"] > 0 and queued[1]["transport_failure"] is None
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
    assert all(event["transfer_seconds"] > 0 and event["transport_failure"] is None for event in retry_events)
    assert activity("revoked")[0]["transport_failure"] is None
    for key, expected, kinds in (
        ("timeout", {"DeadlineExceeded", "Timeout"}, ["WebhookCallFailedEvent"]),
        ("transport-disconnected", {"Connection"},
         ["DispatchingWebhookCallEvent", "WebhookCallFailedEvent", "FinalWebhookCallFailedEvent"]),
        ("transport-large-response", {"ResponseTooLarge"}, ["DispatchingWebhookCallEvent", "WebhookCallFailedEvent"]),
    ):
        observations = activity(key)
        assert [event["kind"] for event in observations] == kinds, observations
        for observation in observations:
            assert observation["transfer_seconds"] is None
            if observation["attempt"] is None:
                assert observation["transport_failure"] is None
            else:
                assert observation["transport_failure"] in expected, observation
                assert observation["attempt"]["number"] == 1 and observation["attempt"]["status"] is None
    assert [event["kind"] for event in activity("stale")] == ["WebhookCallSucceededEvent"]
    assert not [event for event in events if event["message_id"].startswith("skip-")]
    assert "fixture-private-response" not in recorded and "fixture-remote-response" not in recorded
    log.flush()
    logs = [Path(log.name), *(Path(env["APP_BASE_PATH"]) / "storage/logs").glob("*.log")]
    for path in logs:
        output = path.read_bytes()
        assert SECRET not in output and b"fixture-sensitive-listener-sentinel" not in output
        assert b"fixture-custom-hook-sensitive" not in output
        assert b"fixture-private-response" not in output


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
        # 授权目标必须使用实际数字回环 IP，而非域名。
        family = socket.AF_INET6 if ":" in self.server.expected[0] else socket.AF_INET
        ipv6 = family == socket.AF_INET6
        assert self.read(4) == (b"\x05\x01\x00\x04" if ipv6 else b"\x05\x01\x00\x01")
        target = socket.inet_ntop(family, self.read(16 if ipv6 else 4)), int.from_bytes(self.read(2), "big")
        assert target == self.server.expected, target
        self.server.targets.append(target)
        with socket.create_connection(target, timeout=6) as peer:
            self.request.sendall(b"\x05\x00\x00\x01\x00\x00\x00\x00\x00\x00")
            relay_proxy(self.request, peer)


class HttpConnectProxy(socketserver.BaseRequestHandler):
    def handle(self):
        self.request.settimeout(6)
        if self.server.tls_context is None:
            self.connect(self.request)
            return
        try:
            connection = self.server.tls_context.wrap_socket(self.request, server_side=True)
        except ssl.SSLError:
            return  # A client may reject the proxy certificate before sending CONNECT.
        with connection:
            self.server.client_certificates.append(connection.getpeercert(binary_form=True))
            self.connect(connection)

    def connect(self, connection):
        with connection.makefile("rb", buffering=0) as reader:
            line = reader.readline(1024)
            target = socket_authority(*self.server.expected).encode()
            assert line == b"CONNECT " + target + b" HTTP/1.1\r\n", line
            headers = {}
            for _ in range(16):
                line = reader.readline(1024)
                if line == b"\r\n":
                    break
                field, value = line.split(b":", 1)
                headers[field.lower()] = value.strip()
            else:
                raise AssertionError("CONNECT header limit")
        expected = b"Basic " + base64.b64encode(b"fixture-user:local p@ss/%")
        if headers.get(b"proxy-authorization") != expected:
            self.server.rejected += 1
            connection.sendall(b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n")
            return
        assert headers.get(b"host") == target
        self.server.targets.append(self.server.expected)
        with socket.create_connection(self.server.expected, timeout=6) as peer:
            connection.sendall(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            relay_proxy(connection, peer)


def relay_proxy(request, peer):
    swap = {request: peer, peer: request}
    while ready := ([request] if isinstance(request, ssl.SSLSocket) and request.pending()
                    else select.select(list(swap), [], [], 6)[0]):
        for source in ready:
            data = source.recv(8192)
            if not data:
                return
            swap[source].sendall(data)


def socket_authority(host, port):
    return f"[{host}]:{port}" if ":" in host else f"{host}:{port}"


class Ipv6Receiver(http.server.ThreadingHTTPServer):
    address_family = socket.AF_INET6


class Ipv6Proxy(socketserver.ThreadingTCPServer):
    address_family = socket.AF_INET6


def configure_tls(receiver, folder, profile, proxy):
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
        "proxy-ip": f"IP:{receiver.server_address[0]}",
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
    if proxy is None:
        return
    ca("proxy-ca")
    certificate("proxy", "proxy-ca", 1,
                "subjectAltName=DNS:proxy-fixture.invalid\nextendedKeyUsage=serverAuth\n")
    proxy.tls_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    proxy.tls_context.load_cert_chain(folder / "proxy.pem", folder / "proxy.key")
    proxy.tls_context.verify_mode = ssl.CERT_OPTIONAL
    # Ask for and accept the target client certificate if the Worker wrongly offers it.
    proxy.tls_context.load_verify_locations(folder / "ca.pem")
    proxy.tls_context.set_servername_callback(lambda _, name, __: proxy.tls_names.append(name))


def main():
    binary = routes["args"].binary.resolve()
    driver, url = helpers["database_settings"]()
    ipv6 = os.environ.get("FIXTURE_SEND_IPV6") == "1"
    local = "::1" if ipv6 else "127.0.0.1"
    receiver_type = Ipv6Receiver if ipv6 else http.server.ThreadingHTTPServer
    proxy_type = Ipv6Proxy if ipv6 else socketserver.ThreadingTCPServer
    receiver = receiver_type((local, 0), Receiver)
    receiver.mutex = threading.Lock()
    receiver.received = {}
    receiver.unexpected = []
    receiver.client_certificates = {}
    receiver.entered = {name: threading.Event() for name in (
        "crash", "concurrent", "stale", "timeout", "endpoint-inflight",
        "slot-hold-one", "slot-hold-two", "slot-hold-race-one", "slot-hold-race-two",
    )}
    receiver.release = {name: threading.Event() for name in receiver.entered}
    peer = threading.Thread(target=receiver.serve_forever, daemon=True)
    proxy = proxy_thread = None
    try:
        with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="webhook-send-") as root:
            profile = os.environ.get("FIXTURE_SEND_TLS")
            protocol = os.environ.get("FIXTURE_SEND_PROXY_PROTOCOL")
            if profile and profile.startswith("proxy"):
                handler = HttpConnectProxy if protocol in ("connect", "connect-tls") else SocksProxy
                proxy = proxy_type((local, 0), handler)
                proxy.expected = (local, receiver.server_port)
                proxy.targets = []
                proxy.rejected = 0
                proxy.tls_context = None
                proxy.tls_names = []
                proxy.client_certificates = []
                receiver.proxy = proxy
            if profile:
                tls = Path(root) / "tls"
                tls.mkdir(mode=0o700)
                configure_tls(receiver, tls, profile, proxy if protocol == "connect-tls" else None)
            if proxy:
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
                "FIXTURE_SEND_ADDRESS": socket_authority(local, receiver.server_port),
                "FIXTURE_SEND_EVENTS": str(Path(root) / "delivery-events.jsonl"),
                "FIXTURE_JOB_HOOK_LOG": str(Path(root) / "job-hooks.jsonl"),
                "HTTP_PROXY": "http://127.0.0.1:1", "HTTPS_PROXY": "http://127.0.0.1:1", "NO_PROXY": "",
            }
            if profile:
                env["FIXTURE_SEND_TLS_DIR"] = str(tls)
                env["FIXTURE_SEND_TARGET"] = env["FIXTURE_SEND_TARGET"].replace("http:", "https:", 1)
                if profile == "proxy-ip":
                    env["FIXTURE_SEND_TARGET"] = f"https://{socket_authority(local, receiver.server_port)}/receiver"
            if proxy:
                env["FIXTURE_SEND_PROXY_ADDRESS"] = socket_authority(local, proxy.server_address[1])
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

            if env.get("FIXTURE_SEND_KEY_BINDINGS"):
                legacy_binary = Path(env["FIXTURE_SEND_LEGACY_BINARY"]).resolve()
                routes["command"](legacy_binary, env, "migrate", "--force")
                routes["command"](legacy_binary, env, "webhooks:fixture-endpoint", "create", "legacy-endpoint")
                routes["command"](legacy_binary, env, "webhooks:fixture-endpoint", "subscribe", "legacy-endpoint",
                                  "--topic=invoice.paid", "--schema=1")
                with (Path(root) / "legacy-process.log").open("w+") as log:
                    old = http_helpers["serve"](legacy_binary, env, log)
                    receipts = []
                    try:
                        for route, body in (("normal/legacy-submission", BODY),
                                            ("publish/legacy-message", b'{"status":"retry"}')):
                            connection = http.client.HTTPConnection("127.0.0.1", app_port, timeout=20)
                            try:
                                connection.request("POST", f"/api/outbound/{route}", body,
                                                   {"Accept": "application/json"})
                                response = connection.getresponse()
                                payload = json.loads(response.read())
                                assert response.status == 202, payload
                                receipts.append(payload)
                            finally:
                                connection.close()
                    finally:
                        http_helpers["stop"](old)
                env["FIXTURE_SEND_LEGACY_RECEIPTS"] = json.dumps(receipts)

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
                or os.environ.get("FIXTURE_SEND_CONCURRENCY")
                or os.environ.get("FIXTURE_SEND_FANOUT") or os.environ.get("FIXTURE_SEND_TLS")
                or os.environ.get("FIXTURE_SEND_DIAGNOSTICS") or os.environ.get("FIXTURE_SEND_RETENTION")
                or os.environ.get("FIXTURE_SEND_KEY_BINDINGS") or os.environ.get("FIXTURE_SEND_KEY_EXPIRY")
                or os.environ.get("FIXTURE_SEND_HISTORY")):
            print(f"{driver}: HTTP 签名和字节、事务回滚、重复领取、重试预算、撤销、超时、崩溃恢复、Builder 分支、投递事件与自定义 Job 通过")
    finally:
        if proxy_thread is not None:
            proxy.shutdown()
            proxy_thread.join(timeout=5)
        if proxy is not None:
            proxy.server_close()
        if peer.ident is not None:
            receiver.shutdown()
            peer.join(timeout=5)
        receiver.server_close()


if __name__ == "__main__":
    main()
