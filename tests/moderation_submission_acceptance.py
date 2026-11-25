"""三库以真实 HTTP、Artisan 和 Worker 验收点播领取、失联与过期。"""

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
from urllib.parse import urlsplit


ROOT = Path(__file__).resolve().parents[1]
VIDEO = runpy.run_path(str(ROOT / "tests/moderation_sightengine_video_acceptance.py"))
DATABASE = runpy.run_path(str(ROOT / "tests/sms_worker.py"))
HTTP = VIDEO["HTTP"]
COMMAND = "drivers:moderation-review"


class PausedSupplier(VIDEO["Supplier"]):
    def respond(self, status, payload):
        gate = getattr(self.server, "gate", None)
        if self.command == "POST" and gate:
            reached, release = gate
            reached.set()
            assert release.wait(timeout=20)
        try:
            super().respond(status, payload)
        except (BrokenPipeError, ConnectionResetError):
            if not gate:
                raise


def exercise(binary, driver, url, root, storage_env, storage):
    first, last, original, changed = VIDEO["versioned_clips"](root, storage)
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), PausedSupplier)
    supplier.lock, supplier.mode = threading.Lock(), "accepted"
    supplier.expected_secret, supplier.gate = VIDEO["SECRET"], None
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

    def stage(intent, version=first):
        return json.loads(artisan(env, COMMAND, "stage", "app", "tenant", intent, version))

    def submit(review, current=env):
        return json.loads(artisan(current, COMMAND, "submit", "app", "tenant", review["id"]))

    def sql(query):
        if driver == "sqlite":
            with sqlite3.connect(sqlite) as connection:
                return "\n".join("\t".join(map(str, row)) for row in connection.execute(query))
        return DATABASE["database_rows"](DATABASE["database_container"](), query).strip()

    def post(review, expected=200):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=25)
        try:
            connection.request("POST", "/api/drivers/moderation/sightengine-video", headers={
                "Accept": "application/json", "X-Fixture-Submit-Review": review["id"],
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
    print(f"{driver}: 真实 HTTP／Artisan／Worker 领取、独立对象版本、失联与过期通过")
