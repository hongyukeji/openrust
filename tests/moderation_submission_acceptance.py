"""三库以真实 HTTP、Artisan 和 Worker 验收点播领取、失联与过期。"""

import argparse
from concurrent.futures import ThreadPoolExecutor
from email.utils import formatdate
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
VIDEO = runpy.run_path(str(ROOT / "tests/moderation_sightengine_video_acceptance.py"))
DATABASE = runpy.run_path(str(ROOT / "tests/sms_worker.py"))
HTTP = VIDEO["HTTP"]
COMMAND = "drivers:moderation-review"


class PausedSupplier(VIDEO["Supplier"]):
    def respond(self, status, payload):
        gate = self.server.gate if self.command == "POST" else self.server.query_gate
        if gate:
            reached, release = gate
            reached.set()
            assert release.wait(timeout=20)
        if self.command == "GET" and status == 200:
            if self.server.query_mode == "wait":
                payload["output"]["data"]["status"] = "ongoing"
            elif self.server.query_mode in ("quota", "unavailable"):
                status = 429 if self.server.query_mode == "quota" else 503
                payload = {"status": "failure", "error": {"message": VIDEO["PRIVATE"]}}
        try:
            super().respond(status, payload)
        except (BrokenPipeError, ConnectionResetError):
            if not gate:
                raise


    def end_headers(self):
        if self.command == "GET":
            for value in getattr(self.server, "retry_after_headers", ()):
                self.send_header("Retry-After", value)
        super().end_headers()


def exercise(binary, driver, url, root, storage_env, storage):
    first, last, original, changed = VIDEO["versioned_clips"](root, storage)
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), PausedSupplier)
    supplier.lock, supplier.mode = threading.Lock(), "accepted"
    supplier.expected_secret, supplier.gate = VIDEO["SECRET"], None
    supplier.query_mode, supplier.query_gate = "finished", None
    supplier.posted, supplier.queried, supplier.stops = [], [], []
    supplier.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    supply = threading.Thread(target=supplier.serve_forever, daemon=True)
    supply.start()
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    sqlite = root / "reviews.sqlite"
    if driver == "sqlite":
        sqlite.touch()
        os.environ["OPENRUST_SMS_SQLITE_PATH"] = str(sqlite)
    mount = root / "credentials"
    mount.mkdir(mode=0o700)
    credential = mount / "sightengine.r1.json"
    descriptor = os.open(credential, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o400)
    with os.fdopen(descriptor, "w") as stream:
        json.dump({"api_user": "local-sightengine-account", "api_secret": VIDEO["SECRET"]}, stream)
    endpoint = f"http://127.0.0.1:{supplier.server_port}"
    env = os.environ | storage_env | {
        "APP_BASE_PATH": str(root), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "DB_CONNECTION": driver, "QUEUE_CONNECTION": "database",
        "DB_DATABASE": str(sqlite) if driver == "sqlite" else "openrust_acceptance",
        "MODERATION_CREDENTIAL_DIRECTORY": str(mount), "MODERATION_FIXTURE_PIN_ENDPOINT": endpoint,
        "MODERATION_FIXTURE_SIGHTENGINE_VIDEO": endpoint, "MODERATION_FIXTURE_SIGHTENGINE_VIDEO_TIMEOUT": "18000",
        "AWS_TEMPORARY_URL": VIDEO["ORIGIN"], "MODERATION_FIXTURE_IMAGE_ORIGIN": VIDEO["ORIGIN"],
        "MODERATION_FIXTURE_QUERY_INTERVAL": "20", "MODERATION_FIXTURE_QUERY_ATTEMPTS": "2",
    }
    if url:
        env["DB_URL"] = url
    else:
        env.pop("DB_URL", None)

    def artisan(current, *tokens):
        result = subprocess.run([str(binary), "artisan", *tokens], env=current,
                                capture_output=True, text=True, timeout=30)
        assert VIDEO["SECRET"] not in result.stdout + result.stderr
        assert result.returncode == 0, result.stdout + result.stderr
        return result.stdout

    def stage(intent, version=first, current=env):
        return json.loads(artisan(current, COMMAND, "stage", "app", "tenant", intent, version))

    def submit(review, current=env):
        return json.loads(artisan(current, COMMAND, "submit", "app", "tenant", review["id"]))

    def sql(query):
        if driver == "sqlite":
            with sqlite3.connect(sqlite) as connection:
                return "\n".join("\t".join(map(str, row)) for row in connection.execute(query))
        return DATABASE["database_rows"](DATABASE["database_container"](), query).strip()

    def post(review, expected=200, *, query=False):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=25)
        try:
            header = "X-Fixture-Query-Review" if query else "X-Fixture-Submit-Review"
            connection.request("POST", "/api/drivers/moderation/sightengine-video", headers={
                "Accept": "application/json", header: review["id"],
            })
            response = connection.getresponse()
            data = response.read()
            assert response.status == expected, (expected, response.status, data[:250])
            assert VIDEO["SECRET"].encode() not in data
            return json.loads(data)
        finally:
            connection.close()

    def expire_lease(review):
        sql(f"UPDATE moderation_checks SET submit_lease_until = 1 WHERE id = '{review['check_id']}'")

    def query(review, current=env):
        return json.loads(artisan(current, COMMAND, "query", "app", "tenant", review["id"]))

    def due(review, timestamp=1):
        sql(f"UPDATE moderation_checks SET available_at = {timestamp} WHERE id = '{review['check_id']}'")

    accepted = {"state": "Submitted", "task": VIDEO["TASK"]}
    try:
        artisan(env, "migrate", "--force")
        artisan(env, "config:cache")
        artisan(env, "route:cache")
        first_review = stage("coordinated")
        with (root / "http.log").open("w+") as log:
            app = HTTP["serve"](binary, env, log)
            try:
                assert post({"id": "00000000-0000-4000-8000-000000000000"}, 404) == {"error": "ReviewNotFound"}
                with ThreadPoolExecutor(max_workers=1) as executor:
                    reached, release = supplier.gate = (threading.Event(), threading.Event())
                    sending = executor.submit(post, first_review)
                    try:
                        assert reached.wait(8), "送审未到达协议替身"
                        pending = submit(first_review)
                        assert pending["version"] == 2 and pending["submission"] == {"state": "Submitting"}
                        worker_review = stage("worker-pinned", last)
                    finally:
                        release.set()
                    sent = sending.result(15)
                    supplier.gate = None
                assert sent["version"] == 3 and sent["state"] == "Running" and sent["submission"] == accepted
                assert submit(first_review) == sent

                late_review = stage("late-worker")
                with ThreadPoolExecutor(max_workers=1) as executor:
                    reached, release = supplier.gate = (threading.Event(), threading.Event())
                    sending = executor.submit(post, late_review)
                    try:
                        assert reached.wait(8)
                        expire_lease(late_review)
                        recovered = submit(late_review)
                        assert recovered["version"] == 3 and recovered["submission"] == {"state": "Unknown"}
                    finally:
                        release.set()
                    assert sending.result(15) == recovered
                    supplier.gate = None
            finally:
                HTTP["stop"](app)
            log.seek(0)
            assert VIDEO["SECRET"] not in log.read()

        assert json.loads(artisan(env, COMMAND, "queue-submit", "app", "tenant", worker_review["id"])) == {"queued": True}
        payload = sql("SELECT payload FROM jobs")
        assert worker_review["id"] in payload
        assert all(value not in payload for value in (endpoint, VIDEO["SECRET"], last, VIDEO["KEY"]))
        output = root / "worker.json"
        artisan(env | {"MODERATION_FIXTURE_RESULT": str(output)}, "queue:work", "database", "--once")
        result = json.loads(output.read_text())
        assert result["submission"] == accepted and result["object_version"] == last
        assert sql("SELECT COUNT(*) FROM jobs") == sql("SELECT COUNT(*) FROM failed_jobs") == "0"
        sql(f"UPDATE moderation_reviews SET deadline_at = 1 WHERE id = '{worker_review['id']}'")
        expired = submit(worker_review)
        assert expired["state"] == "Expired" and expired["submission"] == accepted

        crashed = stage("crashed-process")
        reached, release = supplier.gate = (threading.Event(), threading.Event())
        with subprocess.Popen([str(binary), "artisan", COMMAND, "submit", "app", "tenant", crashed["id"]],
                              env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE) as process:
            try:
                assert reached.wait(8)
                process.kill()
                process.wait(timeout=5)
                expire_lease(crashed)
                result = submit(crashed)
                assert result["submission"] == {"state": "Unknown"} and result["version"] == 3
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)
                release.set()
        supplier.gate = None

        stale = stage("past-deadline")
        sql(f"UPDATE moderation_reviews SET deadline_at = 1 WHERE id = '{stale['id']}'")
        result = submit(stale)
        assert result["state"] == "Expired" and result["submission"] == {"state": "Expired"}
        supplier.mode = "quota"
        limited = stage("quota")
        result = submit(limited)
        assert result["state"] == "Failed" and result["submission"] == {"state": "Failed", "reason": "throttled"}
        assert submit(limited) == result
        supplier.mode = "lost-id"
        ambiguous = stage("response-unknown")
        result = submit(ambiguous)
        assert result["state"] == "Running" and result["submission"] == {"state": "Unknown"}
        assert submit(ambiguous) == result
        missing = stage("missing-old-revision")
        without = env | {"MODERATION_FIXTURE_PIN_CHANGE": "missing"}
        artisan(without, "config:clear")
        artisan(without, "config:cache")
        result = submit(missing, without)
        assert result["state"] == "Failed" and result["submission"] == {"state": "Failed", "reason": "revision_unavailable"}
        assert supplier.posted == [(first, original), (first, original), (last, changed),
                                   (first, original), (first, original), (first, original)]
        assert not supplier.queried
        assert "sightengine" not in sql("SELECT remote_task_id FROM moderation_checks WHERE remote_task_id IS NOT NULL")

        artisan(env, "config:clear")
        artisan(env, "config:cache")
        supplier.mode = "accepted"
        assert query(ambiguous)["query_attempts"] == 0
        assert sent["next_query_at"] > sent["created_at"]
        due(first_review, sent["deadline_at"])
        assert query(first_review)["query_attempts"] == 0
        due(first_review)
        supplier.query_mode = "wait"
        with (root / "query-http.log").open("w+") as log:
            app = HTTP["serve"](binary, env, log)
            try:
                with ThreadPoolExecutor(max_workers=1) as executor:
                    reached, release = supplier.query_gate = (threading.Event(), threading.Event())
                    querying = executor.submit(post, first_review, query=True)
                    try:
                        assert reached.wait(8)
                        concurrent = query(first_review)
                        assert concurrent["query_attempts"] == 1 and concurrent["state"] == "Running"
                    finally:
                        release.set()
                    waiting = querying.result(15)
                    supplier.query_gate = None
                assert waiting["state"] == "Running" and waiting["query_attempts"] == 1
                assert query(first_review) == waiting and supplier.queried == [VIDEO["TASK"]]
            finally:
                HTTP["stop"](app)
        due(first_review)
        supplier.query_mode = "finished"
        assert json.loads(artisan(env, COMMAND, "queue-query", "app", "tenant", first_review["id"])) == {"queued": True}
        assert first_review["id"] in sql("SELECT payload FROM jobs")
        output.unlink()
        artisan(env | {"MODERATION_FIXTURE_RESULT": str(output)}, "queue:work", "database", "--once")
        partial = json.loads(output.read_text())
        assert partial["state"] == "Failed" and partial["query_failure"] == "incomplete_coverage"
        assert partial["query_attempts"] == 2 and partial["decision"] is None
        assert partial["assessment"]["coverage"] == {"Ocr": "Partial", "Speech": "Unavailable", "Visual": "Partial"}
        labels = {finding["provider"]: finding for finding in partial["assessment"]["labels"]}
        assert labels["violence/prob"]["span_ms"] == [1125, 1125]
        assert labels["violence/prob"]["confidence"] == {"value": .96, "maximum": 1.0}
        assert VIDEO["PRIVATE"] not in sql("SELECT assessment FROM moderation_checks WHERE assessment IS NOT NULL")
        assert query(first_review) == partial

        rate = stage("query-throttled")
        submit(rate)
        due(rate)
        supplier.query_mode = "quota"
        deferred = query(rate)
        assert deferred["state"] == "Running" and deferred["query_failure"] == "throttled"
        assert deferred["query_attempts"] == 1
        assert query(rate) == deferred
        due(rate)
        exhausted = query(rate)
        assert exhausted["state"] == "Failed" and exhausted["query_failure"] == "budget_exhausted"
        assert query(rate) == exhausted and exhausted["query_attempts"] == 2

        retry_date = int(time.time()) + 160
        cases = (
            ("seconds", "quota", ("90",), "throttled"),
            ("date", "unavailable", (formatdate(retry_date, usegmt=True),), "service_unavailable"),
            ("malformed", "quota", ("-20",), "throttled"),
            ("duplicates", "quota", ("900", "910"), "throttled"),
            ("after-deadline", "quota", ("900",), "throttled"),
        )
        with (root / "retry-after-http.log").open("w+") as log:
            app = HTTP["serve"](binary, env, log)
            try:
                for name, mode, headers, failure in cases:
                    review = stage(f"retry-after-{name}")
                    submitted = submit(review)
                    due(review)
                    supplier.query_mode = mode
                    supplier.retry_after_headers = headers
                    before = int(time.time())
                    held = post(review, query=True)
                    seen = len(supplier.queried)
                    assert held["query_failure"] == failure and held["query_attempts"] == 1
                    assert held["state"] == "Running" and held["decision"] is None
                    hold = sql("SELECT COALESCE(provider_retry_at, 0) FROM moderation_checks "
                               f"WHERE id = '{review['check_id']}'")
                    if name == "seconds":
                        assert before + 90 <= int(hold) <= int(time.time()) + 90
                        assert held["next_query_at"] == int(hold)
                    elif name == "date":
                        assert held["next_query_at"] == int(hold) == retry_date
                    elif name == "after-deadline":
                        assert held["next_query_at"] == int(hold) == submitted["deadline_at"]
                    else:
                        assert hold == "0" and before + 40 <= held["next_query_at"] <= int(time.time()) + 44
                    assert query(review) == held
                    artisan(env, COMMAND, "queue-query", "app", "tenant", review["id"])
                    artisan(env | {"MODERATION_FIXTURE_RESULT": str(output)}, "queue:work", "database", "--once")
                    assert json.loads(output.read_text()) == held and len(supplier.queried) == seen
                    if name == "after-deadline":
                        sql("UPDATE moderation_reviews SET deadline_at = 1 "
                            f"WHERE id = '{review['id']}'")
                        assert query(review)["state"] == "Expired" and len(supplier.queried) == seen
                        assert sql("SELECT COUNT(*) FROM moderation_checks "
                                   f"WHERE id = '{review['check_id']}' AND provider_retry_at IS NULL") == "1"
            finally:
                supplier.retry_after_headers = ()
                HTTP["stop"](app)
            log.seek(0)
            assert VIDEO["PRIVATE"] not in log.read()

        crashed_query = stage("crashed-query")
        submit(crashed_query)
        due(crashed_query)
        supplier.query_mode = "wait"
        reached, release = supplier.query_gate = (threading.Event(), threading.Event())
        with subprocess.Popen([str(binary), "artisan", COMMAND, "query", "app", "tenant", crashed_query["id"]],
                              env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) as process:
            try:
                assert reached.wait(8)
                process.kill()
                process.wait(timeout=5)
                sql(f"UPDATE moderation_checks SET query_lease_until = 1 WHERE id = '{crashed_query['check_id']}'")
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)
                release.set()
        supplier.query_gate = None
        recovered_query = query(crashed_query)
        assert recovered_query["query_attempts"] == 2 and recovered_query["query_failure"] == "budget_exhausted"

        deadline = stage("late-query")
        submit(deadline)
        due(deadline)
        supplier.query_mode = "finished"
        with ThreadPoolExecutor(max_workers=1) as executor:
            reached, release = supplier.query_gate = (threading.Event(), threading.Event())
            checking = executor.submit(query, deadline)
            try:
                assert reached.wait(8)
                sql(f"UPDATE moderation_reviews SET deadline_at = 1 WHERE id = '{deadline['id']}'")
                expired_after_query = query(deadline)
                assert expired_after_query["state"] == "Expired" and expired_after_query["decision"] is None
            finally:
                release.set()
            assert checking.result(15) == expired_after_query
            supplier.query_gate = None

        external = env | {"MODERATION_FIXTURE_QUERY_EXTERNAL": "1"}
        artisan(external, "config:clear")
        artisan(external, "config:cache")
        first_party = stage("external-complete-visual", "revision-2", external)
        submit(first_party, external)
        due(first_party)
        complete = query(first_party, external)
        assert complete["state"] == "Succeeded" and complete["decision"] == "allow"
        assert complete["assessment"]["coverage"]["Visual"] == "Complete"
        assert len(supplier.queried) == 12
        assert sql("SELECT COUNT(*) FROM jobs") == sql("SELECT COUNT(*) FROM failed_jobs") == "0"
    finally:
        supplier.shutdown()
        supplier.server_close()
        supply.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    driver, url = DATABASE["database_settings"]()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-submission-") as root, \
            VIDEO["VIDEO"]["OBJECTS"]["TOPOLOGY"]["object_storage"](True) as (env, storage):
        exercise(args.binary.resolve(), driver, url, Path(root), env, storage)
    print(f"{driver}: 真实 HTTP／Artisan／Worker 送审、查单预算与 Retry-After、失联、覆盖与过期通过")
