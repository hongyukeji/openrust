"""真实 HTTP、Artisan 和独立 Worker 验收父审核与子检查同库持久化。"""

import argparse
from concurrent.futures import ThreadPoolExecutor
import http.client
import json
import os
from pathlib import Path
import runpy
import socket
import sqlite3
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
DATABASE = runpy.run_path(str(ROOT / "tests/sms_worker.py"))
HTTP = runpy.run_path(str(ROOT / "tests/webhooks_acceptance.py"))
COMMAND = "drivers:moderation-review"
PROFILE = "sightengine-videos"


def exercise(binary, driver, database_url, root):
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    sqlite = root / "reviews.sqlite"
    if driver == "sqlite":
        sqlite.touch()
        os.environ["OPENRUST_SMS_SQLITE_PATH"] = str(sqlite)
    env = os.environ | {
        "APP_BASE_PATH": str(root), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "DB_CONNECTION": driver, "QUEUE_CONNECTION": "database",
        "DB_DATABASE": str(sqlite) if driver == "sqlite" else "openrust_acceptance",
        "MODERATION_CREDENTIAL_DIRECTORY": str(root / "missing-credentials"),
        "MODERATION_FIXTURE_SIGHTENGINE_VIDEO": "http://127.0.0.1:1",
        "MODERATION_FIXTURE_PIN_ENDPOINT": "http://127.0.0.1:1",
        "MODERATION_FIXTURE_IMAGE_ORIGIN": "https://media.fixture.invalid",
    }
    if database_url:
        env["DB_URL"] = database_url
    else:
        env.pop("DB_URL", None)

    def artisan(current, *tokens, successful=True):
        result = subprocess.run([str(binary), "artisan", *tokens], env=current,
                                capture_output=True, text=True, timeout=50)
        assert (result.returncode == 0) == successful, result.stdout + result.stderr
        return result.stdout

    def sql(query):
        if driver == "sqlite":
            with sqlite3.connect(sqlite) as connection:
                return "\n".join("\t".join(map(str, row)) for row in connection.execute(query))
        return DATABASE["database_rows"](DATABASE["database_container"](), query).strip()

    def post(intent, version, expected=200):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=12)
        try:
            connection.request("POST", "/api/drivers/moderation/sightengine-video", headers={
                "Accept": "application/json", "X-Fixture-Review-Intent": intent,
                "X-Fixture-Version": version,
            })
            response = connection.getresponse()
            data = response.read()
            assert response.status == expected, (response.status, expected, data[:300])
            return json.loads(data)
        finally:
            connection.close()

    artisan(env, "migrate", "--force")
    artisan(env, "migrate:rollback", "--step=2", "--force")
    artisan(env, "migrate", "--force")
    artisan(env, "vendor:publish", "--tag=moderation-migrations")
    migrations = sorted((root / "database/migrations").glob("*moderation*"))
    assert len(migrations) == 11
    expected = migrations[0].read_text() + "\n// 自定义迁移保留\n"
    migrations[0].write_text(expected)
    artisan(env, "vendor:publish", "--tag=moderation-migrations")
    assert migrations[0].read_text() == expected
    assert "Pending" not in artisan(env, "migrate:status")
    if driver == "sqlite":
        sql("CREATE TRIGGER reject_check BEFORE INSERT ON moderation_checks "
            "BEGIN SELECT RAISE(ABORT, 'acceptance rollback'); END")
        remove_gate = "DROP TRIGGER reject_check"
    elif driver == "pgsql":
        sql("ALTER TABLE moderation_checks ADD CONSTRAINT reject_check CHECK (kind <> 'video')")
        remove_gate = "ALTER TABLE moderation_checks DROP CONSTRAINT reject_check"
    else:
        sql("ALTER TABLE moderation_checks ADD CONSTRAINT reject_check CHECK (kind <> 'video')")
        remove_gate = "ALTER TABLE moderation_checks DROP CHECK reject_check"
    artisan(env, COMMAND, "stage", "app", "tenant", "rollback-intent", "original", successful=False)
    assert sql("SELECT COUNT(*) FROM moderation_reviews") == "0"
    assert sql("SELECT COUNT(*) FROM moderation_checks") == "0"
    sql(remove_gate)

    artisan(env, "config:cache")
    artisan(env, "route:cache")
    with (root / "http.log").open("w+") as log:
        app = HTTP["serve"](binary, env, log)
        try:
            with ThreadPoolExecutor(max_workers=8) as executor:
                requests = [executor.submit(post, "stable-intent", "original") for _ in range(8)]
                result = [request.result() for request in requests]
            first = result[0]
            assert all(row == first for row in result)
            assert post("stable-intent", "different", 409) == {"error": "ReviewConflict"}
            assert post("rollback-intent", "new")["id"] != first["id"]
        finally:
            HTTP["stop"](app)
    assert first["content_id"] == "影片/1"
    assert first["object_version"] == "original" and first["version"] == 1
    assert first["deadline_at"] - first["created_at"] == 300
    assert first["profile"]["name"] == PROFILE
    assert first["profile"]["credential"]["revision"] == "r1"
    assert json.loads(artisan(env, COMMAND, "stage", "app", "tenant", "stable-intent", "original")) == first
    assert json.loads(artisan(env, COMMAND, "find", "App", "tenant", first["id"])) is None
    assert json.loads(artisan(env, COMMAND, "find", "app", "Tenant", first["id"])) is None
    tenant = json.loads(artisan(env, COMMAND, "stage", "app", "Tenant", "stable-intent", "different"))
    assert tenant["id"] != first["id"] and tenant["check_id"] != first["check_id"]

    assert json.loads(artisan(env, COMMAND, "queue", "app", "tenant", first["id"])) == {"queued": True}
    payload = sql("SELECT payload FROM jobs")
    assert first["id"] in payload and "versions/clip.mp4" not in payload and "profile" not in payload
    output = root / "worker.json"
    later = env | {"MODERATION_FIXTURE_RESULT": str(output), "MODERATION_FIXTURE_PIN_CHANGE": "missing"}
    artisan(later, "config:clear")
    artisan(later, "config:cache")
    artisan(later, "queue:work", "database", "--once")
    assert json.loads(output.read_text()) == first
    assert sql("SELECT COUNT(*) FROM jobs") == sql("SELECT COUNT(*) FROM failed_jobs") == "0"
    assert sql("SELECT COUNT(*) FROM moderation_reviews WHERE state = 'Pending' AND version = 1") == "3"
    assert sql("SELECT COUNT(*) FROM moderation_checks WHERE kind = 'video' AND submission = 'Ready'") == "3"
    artisan(env, "migrate:reset", "--force",
            "--path=vendor/openrust/content-moderation/database/migrations/"
            "2026_11_26_000006_add_moderation_callback_reference.rs", successful=False)
    assert sql("SELECT COUNT(*) FROM moderation_reviews") == sql("SELECT COUNT(*) FROM moderation_checks") == "3"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    driver, url = DATABASE["database_settings"]()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-reviews-") as directory:
        exercise(args.binary.resolve(), driver, url, Path(directory))
    print(f"{driver}: 实际迁移、HTTP 幂等、Artisan 隔离和独立 Worker 读取通过")
