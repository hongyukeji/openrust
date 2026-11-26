"""真实 HTTP、Artisan、独立 Worker 验收 Hive 的无签名点播查单提示。"""

import argparse
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
INGRESS = runpy.run_path(str(ROOT / "tests/support/moderation_callback_ingress.py"))
HIVE = runpy.run_path(str(ROOT / "tests/moderation_hive_video_acceptance.py"))
DATABASE, HTTP, VIDEO = (HIVE[key] for key in ("DATABASE", "HTTP", "VIDEO"))
HINT_QUEUE = "moderation-hive-fixture"
PATH = "/api/drivers/moderation/hive-video-hint"
PRIVATE = "hint-body-confidential-canary"


def exercise(binary, driver, url, root, storage_env, storage):
    first, _, original, _ = VIDEO["versioned_clips"](root, storage)
    vendor = http.server.ThreadingHTTPServer(("127.0.0.1", 0), HIVE["Supplier"])
    vendor.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    vendor.calls, vendor.mode, vendor.lock = [], "accepted", threading.Lock()
    vendor.summary_calls, vendor.query_mode = [], "finished"
    vendor.project, vendor.token, vendor.video_model = HIVE["PROJECT"], HIVE["TOKEN"], "CATEGORIZATION"
    vendor.expected_callback = (f"https://{INGRESS['HOST']}{PATH}"
                                if os.environ.get("OPENRUST_TEST_MODERATION_CALLBACK_NGINX") else HIVE["CALLBACK"])
    thread = threading.Thread(target=vendor.serve_forever, daemon=True)
    thread.start()
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    db = root / "hints.sqlite"
    if driver == "sqlite":
        db.touch()
        os.environ["OPENRUST_SMS_SQLITE_PATH"] = str(db)
    mount = root / "credentials"
    mount.mkdir(mode=0o700)
    descriptor = os.open(mount / "hive-video.r1.json", os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o400)
    with os.fdopen(descriptor, "w") as file:
        json.dump({"video_token": HIVE["TOKEN"]}, file)

    env = os.environ | storage_env | {
        "APP_BASE_PATH": str(root), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "DB_CONNECTION": driver, "QUEUE_CONNECTION": "database",
        "DB_DATABASE": str(db) if driver == "sqlite" else "openrust_acceptance",
        "MODERATION_CREDENTIAL_DIRECTORY": str(mount),
        "MODERATION_FIXTURE_HIVE_VIDEO": f"http://127.0.0.1:{vendor.server_port}",
        "MODERATION_FIXTURE_HIVE_CALLBACK": vendor.expected_callback,
        "MODERATION_FIXTURE_HIVE_VIDEO_QUERY": "v1-task-summary",
        "MODERATION_FIXTURE_DURABLE_VIDEO_PROFILE": "hive-videos",
        "MODERATION_CALLBACK_FIXTURE_HIVE": "1", "MODERATION_FIXTURE_RECOVERY_QUEUE": "database",
        "AWS_TEMPORARY_URL": VIDEO["ORIGIN"], "MODERATION_FIXTURE_IMAGE_ORIGIN": VIDEO["ORIGIN"],
    }
    if url:
        env["DB_URL"] = url
    else:
        env.pop("DB_URL", None)

    def artisan(*tokens):
        result = subprocess.run([str(binary), "artisan", *tokens], env=env,
                                capture_output=True, text=True, timeout=35)
        assert all(secret not in result.stdout + result.stderr for secret in (PRIVATE, HIVE["TOKEN"]))
        assert result.returncode == 0, result.stdout + result.stderr
        return result.stdout

    def review(command, value, tenant="tenant"):
        return json.loads(artisan("drivers:moderation-review", command, "app", tenant, value))

    def sql(query):
        if driver == "sqlite":
            with sqlite3.connect(db) as connection:
                return "\n".join("\t".join(map(str, row)) for row in connection.execute(query))
        return DATABASE["database_rows"](DATABASE["database_container"](), query).strip()

    def notify(*, project=HIVE["PROJECT"], task=HIVE["TASK"], signature=None,
               status=202, extra="", array=False):
        data = [task, project] if array else {
            "id": task, "project_id": project,
            "status": [{"review": "Allow", "transcript": PRIVATE + extra}],
        }
        body = json.dumps(data).encode()
        headers = {"Accept": "application/json", "Content-Type": "application/json"}
        if signature is not None:
            headers["Signature"] = signature
        connection = open_callback()
        try:
            connection.request("POST", PATH, body, headers=headers)
            response = connection.getresponse()
            actual = response.read()
            assert response.status == status, (response.status, status, actual[:220])
            assert PRIVATE.encode() not in actual
            if status == 202:
                assert json.loads(actual) == {"message": "accepted"}
        finally:
            connection.close()

    try:
        artisan("migrate", "--force")
        artisan("config:cache")
        artisan("route:cache")
        assert HIVE["TOKEN"] not in (root / "config.json").read_text()
        intent = json.loads(artisan("drivers:moderation-review", "stage", "app", "tenant", "hint-original", first))
        foreign = json.loads(artisan("drivers:moderation-review", "stage", "app", "other", "hint-foreign", first))
        assert review("submit", foreign["id"], tenant="other")["submission"]["task"] == HIVE["TASK"]
        with (root / "http.log").open("w+") as log, INGRESS["callback_ingress"](root, port) as open_callback:
            app = HTTP["serve"](binary, env | {"MODERATION_FIXTURE_HIVE_VIDEO": "http://127.0.0.1:1"}, log)
            try:
                notify()
                assert sql("SELECT COUNT(*) FROM webhook_hints") == "0"
                assert vendor.summary_calls == []
                submitted = review("submit", intent["id"])
                assert submitted["submission"]["task"] == HIVE["TASK"]
                notify(project=HIVE["PROJECT"] + 1)
                notify(task="8069a216-a581-43ab-8440-3b642a8f169d")
                notify(task="wrong-shape")
                notify(array=True)
                notify(signature="", status=500)
                notify(signature="fake-signature", status=500)
                notify(extra="x" * 4096, status=413)
                assert sql("SELECT COUNT(*) FROM webhook_hints") == "0"
                notify()
                notify()
                assert sql("SELECT COUNT(*) FROM webhook_hints") == "2"
                assert sql("SELECT COUNT(*) FROM webhook_calls") == "0"
                assert sql("SELECT COUNT(*) FROM webhook_hints "
                           f"WHERE task_id = '{intent['id']}' AND status = 'pending'") == "2"
                serialized = sql("SELECT payload FROM jobs ORDER BY id")
                assert PRIVATE not in serialized and HIVE["TASK"] not in serialized
                assert sql("SELECT COUNT(*) FROM webhook_verification_audits "
                           "WHERE classification = 'verified'") == "0"
                for _ in range(2):
                    artisan("queue:work", "database", f"--queue={HINT_QUEUE}", "--once")
                after = review("find", intent["id"])
                assert after["lookup_hint"] and after["callback_receipt"] is None
                assert after["next_query_at"] < submitted["next_query_at"] and after["query_attempts"] == 0
                assert after["decision"] is None and vendor.summary_calls == []
                assert sql("SELECT status, COUNT(*) FROM webhook_hints GROUP BY status ORDER BY status") == (
                    "completed\t1\nignored\t1")
                notify()
                assert sql("SELECT COUNT(*) FROM webhook_hints") == "2"
                artisan("schedule:run")
                artisan("queue:work", "database", "--queue=moderation-recovery", "--once")
                final = review("find", intent["id"])
                assert final["lookup_hint"] == after["lookup_hint"] and final["callback_receipt"] is None
                assert final["query_attempts"] == 1 and final["query_failure"] == "incomplete_coverage"
                assert final["decision"] is None and vendor.summary_calls == [0]
                assert review("find", foreign["id"], tenant="other")["lookup_hint"] is None
                notify()
                for _ in range(6):
                    notify(task="other-shape")
                notify(status=429)
                assert sql("SELECT COUNT(*) FROM webhook_calls") == "0"
                assert sql("SELECT COUNT(*) FROM webhook_hints") == "2"
                assert vendor.summary_calls == [0]
            finally:
                HTTP["stop"](app)
            log.seek(0)
            trace = log.read()
            assert PRIVATE not in trace and HIVE["TOKEN"] not in trace
        assert vendor.calls == [(first, original)] * 2
    finally:
        vendor.shutdown()
        vendor.server_close()
        thread.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    driver, url = DATABASE["database_settings"]()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-hive-hint-") as root, \
            VIDEO["VIDEO"]["OBJECTS"]["TOPOLOGY"]["object_storage"](True) as (env, storage):
        exercise(args.binary.resolve(), driver, url, Path(root), env, storage)
    print(f"{driver}: Hive 无签名提示、限流、认证查单通过实际 HTTP、Artisan 和独立 Worker")
