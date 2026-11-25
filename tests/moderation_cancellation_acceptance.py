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


LEASE_MIGRATION = ("--path=vendor/openrust/content-moderation/database/migrations/"
                   "2026_11_26_000013_create_moderation_remote_leases_table.rs")
LEASES = "moderation_remote_leases"


class Supplier(SUBMISSION["PausedSupplier"]):
    def respond(self, status, payload):
        if self.command == "POST" and status == 200 and self.server.unique_task_ids:
            if payload["media"]["id"] == VIDEO["TASK"]:
                with self.server.lock:
                    self.server.issued += 1
                    payload["media"]["id"] = f"med_capacity{self.server.issued}"
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


def exercise(binary, driver, url, root, storage_env, storage, *, test_in_flight=False):
    first, _, original, _ = VIDEO["versioned_clips"](root, storage)
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Supplier)
    supplier.lock, supplier.mode = threading.Lock(), "accepted"
    supplier.expected_secret = VIDEO["SECRET"]
    supplier.gate = supplier.query_gate = supplier.cancel_gate = None
    supplier.cancel_reject, supplier.query_mode = False, "finished"
    supplier.posted, supplier.queried, supplier.stops = [], [], []
    supplier.unique_task_ids, supplier.issued = test_in_flight, 0
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
    if test_in_flight:
        env["MODERATION_FIXTURE_IN_FLIGHT"] = json.dumps({"sightengine": {"original-account": 2}})
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

    def post_submit(review):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=20)
        try:
            connection.request("POST", "/api/drivers/moderation/sightengine-video", headers={
                "Accept": "application/json", "X-Fixture-Submit-Review": review["id"],
            })
            response = connection.getresponse()
            body = response.read()
            assert response.status == 200 and VIDEO["SECRET"].encode() not in body, (response.status, body)
            return json.loads(body)
        finally:
            connection.close()

    def probe_legacy_migration():
        artisan("migrate:rollback", "--force", LEASE_MIGRATION)
        record = stage("legacy-active")
        sql("UPDATE moderation_reviews SET state = 'Running' " f"WHERE id = '{record['id']}'")
        sql("UPDATE moderation_checks SET submission = 'Submitted', remote_task_id = 'med_legacy' "
            f"WHERE id = '{record['check_id']}'")
        artisan("migrate", "--force", LEASE_MIGRATION, successful=False)
        sql("UPDATE moderation_reviews SET deadline_at = 1 " f"WHERE id = '{record['id']}'")
        artisan("migrate", "--force", LEASE_MIGRATION)
        assert model("query", record["id"])["state"] == "Expired" and not supplier.queried

    def in_flight():
        first_review, second, delayed = (stage(name) for name in ("slot-first", "slot-second", "slot-delayed"))
        def occupied():
            return sql(f"SELECT COUNT(*) FROM {LEASES}")

        def due(review):
            sql(f"UPDATE moderation_checks SET available_at = 1 WHERE id = '{review['check_id']}'")

        def slot(review):
            id = review["check_id"]
            return sql(f"SELECT COUNT(*) FROM {LEASES} WHERE check_id = '{id}'")

        with ThreadPoolExecutor(max_workers=2) as pool:
            reached, release = supplier.gate = (threading.Event(), threading.Event())
            active = (pool.submit(post_submit, first_review), pool.submit(worker, review=second["id"]))
            try:
                assert reached.wait(8)
                stop = time.monotonic() + 8
                while len(supplier.posted) < 2 and time.monotonic() < stop:
                    time.sleep(.02)
                assert occupied() == "2" and len(supplier.posted) == 2
                pending = post_submit(delayed)
                assert pending["state"] == "Pending" and pending["submission"]["state"] == "Ready"
                assert occupied() == "2" and len(supplier.posted) == 2
                assert sql("SELECT used FROM moderation_account_rate_buckets WHERE operation = 'submit'") == "2"
            finally:
                release.set()
            sent_first, sent_second = [item.result(timeout=12) for item in active]
            supplier.gate = None
        task_first = sent_first["submission"]["task"]
        task_second = sent_second["submission"]["task"]
        assert task_first != task_second and occupied() == "2"
        still_waiting = worker(review=delayed["id"])
        assert still_waiting["state"] == "Pending" and still_waiting["submission"]["state"] == "Ready"
        assert occupied() == "2" and len(supplier.posted) == 2

        supplier.query_mode = "wait"
        due(first_review)
        assert worker(query=True, review=first_review["id"])["state"] == "Running"
        assert occupied() == "2" and supplier.queried == [task_first]

        assert_cancelled(cancel(second["id"]), "acknowledged")
        assert slot(second) == "0" and occupied() == "1" and supplier.stops == [task_second]
        due(delayed)
        assert worker(review=delayed["id"])["submission"]["state"] == "Submitted" and occupied() == "2"

        supplier.query_mode = "finished"
        due(first_review)
        completed = worker(query=True, review=first_review["id"])
        assert completed["state"] == "Failed" and completed["query_failure"] == "incomplete_coverage"
        assert slot(first_review) == "0" and occupied() == "1"

        unknown = stage("slot-crashed")
        reached, release = supplier.gate = (threading.Event(), threading.Event())
        with subprocess.Popen(command(COMMAND, "submit", "app", "tenant", unknown["id"]),
                              env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) as process:
            try:
                assert reached.wait(8)
                process.kill()
                assert process.wait(timeout=5) != 0
                assert slot(unknown) == "1" and occupied() == "2"
                sql("UPDATE moderation_checks SET submit_lease_until = 1 "
                    f"WHERE id = '{unknown['check_id']}'")
                assert model("submit", unknown["id"])["submission"]["state"] == "Unknown"
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)
                release.set()
        supplier.gate = None
        assert_cancelled(cancel(unknown["id"]), "task_unknown")
        assert slot(unknown) == "1" and occupied() == "2"

        supplier.cancel_reject = True
        assert_cancelled(cancel(delayed["id"]), "rejected")
        supplier.cancel_reject = False
        waiting = stage("slot-wait-expiry")
        assert post_submit(waiting)["submission"]["state"] == "Ready"
        assert len(supplier.posted) == 4 and occupied() == "2"
        sql(f"UPDATE {LEASES} SET expires_at = 1 WHERE check_id = '{unknown['check_id']}'")
        sql("UPDATE moderation_reviews SET deadline_at = 1 " f"WHERE id = '{unknown['id']}'")
        due(waiting)
        assert worker(review=waiting["id"])["submission"]["state"] == "Submitted"
        assert slot(unknown) == "0" and slot(delayed) == "1" and occupied() == "2"

        assert_cancelled(cancel(waiting["id"]), "acknowledged")
        assert slot(waiting) == "0" and occupied() == "1"
        refused = stage("slot-provider-quota")
        supplier.mode = "quota"
        result = post_submit(refused)
        assert result["submission"] == {"state": "Failed", "reason": "throttled"}
        assert slot(refused) == "0" and occupied() == "1"
        assert supplier.posted == [(first, original)] * 6
        assert supplier.queried == [task_first, task_first]
        assert sql("SELECT COUNT(*) FROM jobs") == sql("SELECT COUNT(*) FROM failed_jobs") == "0"
        artisan("migrate:rollback", "--force", LEASE_MIGRATION, successful=False)

    try:
        artisan("migrate", "--force")
        if test_in_flight:
            probe_legacy_migration()
        artisan("config:cache")
        artisan("route:cache")
        with (root / "http.log").open("w+") as log:
            server = HTTP["serve"](binary, env | {"MODERATION_FIXTURE_PIN_ENDPOINT": "http://127.0.0.1:1"}, log)
            try:
                if test_in_flight:
                    in_flight()
                    return
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
        artisan("migrate:rollback", "--force",
                "--path=vendor/openrust/content-moderation/database/migrations/"
                "2026_11_26_000009_add_moderation_cancellation.rs", successful=False)
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
    test_slots = os.environ.get("MODERATION_TEST_IN_FLIGHT") == "1"
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-cancel-") as root, \
            VIDEO["VIDEO"]["OBJECTS"]["TOPOLOGY"]["object_storage"](True) as (env, storage):
        exercise(args.binary.resolve(), driver, url, Path(root), env, storage, test_in_flight=test_slots)
    if test_slots:
        print(f"{driver}: 真实 HTTP、Artisan、独立 Worker 的远端在途上限、已知结果释放和未知任务恢复通过")
    else:
        print(f"{driver}: 真实 HTTP、Artisan、独立 Worker 的本地取消、远端回执与迟到隔离通过")
