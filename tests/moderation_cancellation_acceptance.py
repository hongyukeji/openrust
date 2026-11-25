"""点播审核本地最终取消、供应商回执和陈旧 Worker 隔离的真实验收。"""

import argparse
from concurrent.futures import ThreadPoolExecutor
import http.client
import http.server
import json
import os
from pathlib import Path
import runpy
import socket
import sqlite3
import subprocess
import tempfile
import threading
import time
from urllib.parse import urlsplit


ROOT = Path(__file__).resolve().parents[1]
SUBMISSION = runpy.run_path(str(ROOT / "tests/moderation_submission_acceptance.py"))
VIDEO, HTTP, DATABASE, COMMAND = (SUBMISSION[key] for key in ("VIDEO", "HTTP", "DATABASE", "COMMAND"))


class Supplier(SUBMISSION["PausedSupplier"]):
    def respond(self, status, payload):
        if self.command != "DELETE":
            return super().respond(status, payload)
        if gate := self.server.cancel_gate:
            entered, release = gate
            entered.set()
            assert release.wait(12)
        if self.server.cancel_reject:
            status = 403
            payload = {"status": "failure", "error": {"message": VIDEO["PRIVATE"]}}
        try:
            VIDEO["Supplier"].respond(self, status, payload)
        except (BrokenPipeError, ConnectionResetError):
            if not self.server.cancel_gate:
                raise


def exercise(binary, driver, url, root, storage_env, storage):
    first, _, original, _ = VIDEO["versioned_clips"](root, storage)
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Supplier)
    supplier.lock, supplier.mode = threading.Lock(), "accepted"
    supplier.expected_secret = VIDEO["SECRET"]
    supplier.gate = supplier.query_gate = supplier.cancel_gate = None
    supplier.cancel_reject, supplier.query_mode = False, "finished"
    supplier.posted, supplier.queried, supplier.stops = [], [], []
    supplier.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    thread = threading.Thread(target=supplier.serve_forever, daemon=True)
    thread.start()
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    db = root / "cancel.sqlite"
    if driver == "sqlite":
        db.touch()
        os.environ["OPENRUST_SMS_SQLITE_PATH"] = str(db)
    mount = root / "credentials"
    mount.mkdir(mode=0o700)
    fd = os.open(mount / "sightengine.r1.json", os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o400)
    with os.fdopen(fd, "w") as file:
        json.dump({"api_user": "local-sightengine-account", "api_secret": VIDEO["SECRET"]}, file)
    endpoint = f"http://127.0.0.1:{supplier.server_port}"
    outcome = root / "worker.json"
    env = os.environ | storage_env | {
        "APP_BASE_PATH": str(root), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "DB_CONNECTION": driver, "QUEUE_CONNECTION": "database",
        "DB_DATABASE": str(db) if driver == "sqlite" else "openrust_acceptance",
        "MODERATION_CREDENTIAL_DIRECTORY": str(mount), "MODERATION_FIXTURE_PIN_ENDPOINT": endpoint,
        "MODERATION_FIXTURE_SIGHTENGINE_VIDEO": endpoint, "MODERATION_FIXTURE_SIGHTENGINE_VIDEO_TIMEOUT": "18000",
        "AWS_TEMPORARY_URL": VIDEO["ORIGIN"], "MODERATION_FIXTURE_IMAGE_ORIGIN": VIDEO["ORIGIN"],
        "MODERATION_FIXTURE_QUERY_INTERVAL": "1", "MODERATION_FIXTURE_RESULT": str(outcome),
        "MODERATION_FIXTURE_RECOVERY_QUEUE": "database",
        "MODERATION_FIXTURE_GROUP": "1",
    }
    if url:
        env["DB_URL"] = url
    else:
        env.pop("DB_URL", None)

    def command(*tokens):
        return [str(binary), "artisan", *tokens]

    def artisan(*tokens, successful=True, extra=None):
        result = subprocess.run(command(*tokens), env=env | (extra or {}), capture_output=True,
                                text=True, timeout=35)
        assert VIDEO["SECRET"] not in result.stdout + result.stderr
        assert (result.returncode == 0) == successful, result.stdout + result.stderr
        return result.stdout

    def stage(intent):
        return json.loads(artisan(COMMAND, "stage", "app", "tenant", intent, first))

    def model(action, review_id):
        return json.loads(artisan(COMMAND, action, "app", "tenant", review_id))

    def worker(*, query=False, review):
        model("queue-query" if query else "queue-submit", review)
        artisan("queue:work", "database", "--once")
        return json.loads(outcome.read_text())

    def sql(query):
        if driver == "sqlite":
            with sqlite3.connect(db) as connection:
                return "\n".join("\t".join(map(str, row)) for row in connection.execute(query))
        return DATABASE["database_rows"](DATABASE["database_container"](), query).strip()

    def cancel(review, status=200, tenant="tenant"):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=12)
        try:
            connection.request("POST", "/api/drivers/moderation/cancel", headers={
                "Accept": "application/json", "X-Fixture-Review-Id": review, "X-Fixture-Tenant": tenant,
            })
            response = connection.getresponse()
            data = response.read()
            assert response.status == status and VIDEO["SECRET"].encode() not in data, (response.status, status)
            return json.loads(data)
        finally:
            connection.close()

    def assert_cancelled(review, status):
        assert review["state"] == "Cancelled" and review["provider_cancellation"] == status, review
        assert review["decision"] is None

    try:
        artisan("migrate", "--force")
        artisan("config:cache")
        artisan("route:cache")
        with (root / "http.log").open("w+") as log:
            server = HTTP["serve"](binary, env | {"MODERATION_FIXTURE_PIN_ENDPOINT": "http://127.0.0.1:1"}, log)
            try:
                pending = stage("cancel-pending")
                cancel(pending["id"], 404, "Tenant")
                canceled = cancel(pending["id"])
                assert_cancelled(canceled, "no_task")
                result = worker(review=pending["id"])
                assert result == canceled, (result, canceled)
                assert supplier.posted == supplier.stops == []

                racing = stage("cancel-while-query")
                assert worker(review=racing["id"])["submission"]["task"] == VIDEO["TASK"]
                time.sleep(1.05)
                with ThreadPoolExecutor(max_workers=8) as pool:
                    reached, release = supplier.query_gate = (threading.Event(), threading.Event())
                    query = pool.submit(worker, query=True, review=racing["id"])
                    try:
                        assert reached.wait(8)
                        results = [pool.submit(cancel, racing["id"]) for _ in range(7)]
                        assert all(request.result(timeout=10)["state"] == "Cancelled" for request in results)
                        saved = model("find", racing["id"])
                        assert_cancelled(saved, "acknowledged")
                    finally:
                        release.set()
                    assert query.result(timeout=12) == saved
                    supplier.query_gate = None
                assert saved["assessment"] is None and saved["query_attempts"] == 1
                assert supplier.queried == supplier.stops == [VIDEO["TASK"]]

                running_submit = stage("cancel-while-submitting")
                with ThreadPoolExecutor(max_workers=1) as pool:
                    reached, release = supplier.gate = (threading.Event(), threading.Event())
                    submitting = pool.submit(worker, review=running_submit["id"])
                    try:
                        assert reached.wait(8)
                        saved = cancel(running_submit["id"])
                        assert_cancelled(saved, "task_unknown")
                    finally:
                        release.set()
                    assert submitting.result(timeout=12) == saved
                    supplier.gate = None

                rejected = stage("remote-rejection")
                model("submit", rejected["id"])
                supplier.cancel_reject = True
                stored = model("cancel", rejected["id"])
                assert_cancelled(stored, "rejected")
                assert cancel(rejected["id"]) == stored
                supplier.cancel_reject = False

                interrupted = stage("remote-interrupted")
                model("submit", interrupted["id"])
                reached, release = supplier.cancel_gate = (threading.Event(), threading.Event())
                with subprocess.Popen(command(COMMAND, "cancel", "app", "tenant", interrupted["id"]),
                                      env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE) as process:
                    try:
                        assert reached.wait(8)
                        process.kill()
                        assert process.wait(timeout=5) != 0
                        stored = model("find", interrupted["id"])
                        assert_cancelled(stored, "unconfirmed")
                        assert cancel(interrupted["id"]) == stored
                    finally:
                        release.set()

                expired = stage("already-expired")
                sql(f"UPDATE moderation_reviews SET deadline_at=1 WHERE id='{expired['id']}'")
                late = cancel(expired["id"])
                assert late["state"] == "Expired" and late["provider_cancellation"] is None
                assert model("cancel", expired["id"]) == late
                unsupported = json.loads(artisan(
                    COMMAND, "stage-group", "app", "tenant", "unsupported-group", "revision-2",
                    "group-visual,group-speech",
                ))
                visual, speech = unsupported["checks"]
                model("submit", visual["id"])
                assert_cancelled(cancel(visual["id"]), "unsupported")
                assert_cancelled(cancel(speech["id"]), "no_task")
                unavailable = stage("missing-cancellation-profile")
                model("submit", unavailable["id"])
                assert_cancelled(json.loads(artisan(
                    COMMAND, "cancel", "app", "tenant", unavailable["id"],
                    extra={"APP_CONFIG_CACHE": str(root / "cache-unavailable.json"),
                           "MODERATION_FIXTURE_PIN_CHANGE": "missing"},
                )), "unavailable")
                artisan("schedule:run")
                assert sql("SELECT COUNT(*) FROM jobs") == "0"
                assert supplier.stops == [VIDEO["TASK"]] * 3 and supplier.queried == [VIDEO["TASK"]]
                assert supplier.posted == [(first, original)] * 5
            finally:
                HTTP["stop"](server)
        artisan("migrate:rollback", "--step=1", "--force", successful=False)
        assert sql("SELECT COUNT(*) FROM moderation_checks WHERE provider_cancellation IS NOT NULL") == "8"
    finally:
        supplier.shutdown()
        supplier.server_close()
        thread.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    driver, url = DATABASE["database_settings"]()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-cancel-") as root, \
            VIDEO["VIDEO"]["OBJECTS"]["TOPOLOGY"]["object_storage"](True) as (env, storage):
        exercise(args.binary.resolve(), driver, url, Path(root), env, storage)
    print(f"{driver}: 真实 HTTP、Artisan、独立 Worker 的本地取消、远端回执与迟到隔离通过")
