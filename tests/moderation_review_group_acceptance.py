"""复合审核同库落单、固定检查集合和独立 Worker 验收。"""

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
import time


ROOT = Path(__file__).resolve().parents[1]
CHECK = runpy.run_path(str(ROOT / "tests/moderation_review_storage_acceptance.py"))
DATABASE, HTTP, COMMAND = (CHECK[key] for key in ("DATABASE", "HTTP", "COMMAND"))
VISUAL = "group-visual"
SPEECH = "group-speech"


def exercise(binary, driver, database_url, root):
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    sqlite = root / "groups.sqlite"
    if driver == "sqlite":
        sqlite.touch()
        os.environ["OPENRUST_SMS_SQLITE_PATH"] = str(sqlite)
    result_file = root / "worker.json"
    env = os.environ | {
        "APP_BASE_PATH": str(root), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "DB_CONNECTION": driver, "QUEUE_CONNECTION": "database",
        "DB_DATABASE": str(sqlite) if driver == "sqlite" else "openrust_acceptance",
        "MODERATION_FIXTURE_GROUP": "1", "MODERATION_FIXTURE_RESULT": str(result_file),
    }
    if database_url:
        env["DB_URL"] = database_url
    else:
        env.pop("DB_URL", None)

    def artisan(*tokens, successful=True):
        result = subprocess.run([str(binary), "artisan", *tokens], env=env,
                                capture_output=True, text=True, timeout=30)
        assert (result.returncode == 0) == successful, result.stdout + result.stderr
        return result.stdout

    def sql(query):
        if driver == "sqlite":
            with sqlite3.connect(sqlite) as connection:
                return "\n".join("\t".join(map(str, row)) for row in connection.execute(query))
        return DATABASE["database_rows"](DATABASE["database_container"](), query).strip()

    def post(intent, profiles=(VISUAL, SPEECH), expected=200, tenant="tenant", version="revision-2", group=None):
        headers = {"Accept": "application/json", "X-Fixture-Tenant": tenant}
        if group:
            headers["X-Fixture-Group-Id"] = group
        else:
            headers |= {"X-Fixture-Review-Intent": intent, "X-Fixture-Profiles": ",".join(profiles),
                        "X-Fixture-Version": version}
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=12)
        try:
            connection.request("POST", "/api/drivers/moderation/group", headers=headers)
            response = connection.getresponse()
            data = response.read()
            assert response.status == expected, (response.status, expected, data[:300])
            return json.loads(data)
        finally:
            connection.close()

    def work(action, check):
        if action == "query":
            time.sleep(1.05)
        assert json.loads(artisan(COMMAND, f"queue-{action}", "app", "tenant", check["id"])) == {"queued": True}
        payload = sql("SELECT payload FROM jobs")
        assert check["id"] in payload and "profile" not in payload and "revision-2" not in payload
        artisan("queue:work", "database", "--once")
        return json.loads(result_file.read_text())

    artisan("migrate", "--force")
    if driver == "sqlite":
        sql("CREATE TRIGGER reject_group_check BEFORE INSERT ON moderation_checks "
            "WHEN NEW.modalities LIKE '%speech%' BEGIN SELECT RAISE(ABORT, 'group rollback'); END")
        remove_gate = "DROP TRIGGER reject_group_check"
    else:
        sql("ALTER TABLE moderation_checks ADD CONSTRAINT reject_group_check "
            "CHECK (modalities NOT LIKE '%speech%')")
        remove_gate = "ALTER TABLE moderation_checks DROP " + (
            "CONSTRAINT " if driver == "pgsql" else "CHECK ") + "reject_group_check"
    artisan(COMMAND, "stage-group", "app", "tenant", "rollback", "revision-2",
            f"{VISUAL},{SPEECH}", successful=False)
    for table in ("moderation_review_groups", "moderation_reviews", "moderation_checks"):
        assert sql(f"SELECT COUNT(*) FROM {table}") == "0"
    sql(remove_gate)
    artisan("config:cache")
    artisan("route:cache")

    with (root / "group-http.log").open("w+") as log:
        app = HTTP["serve"](binary, env | {"MODERATION_FIXTURE_GROUP": ""}, log)
        try:
            with ThreadPoolExecutor(max_workers=8) as executor:
                groups = [future.result() for future in [executor.submit(post, "complete") for _ in range(8)]]
            first = groups[0]
            assert all(group == first for group in groups)
            assert first["state"] == "Pending" and first["decision"] is None
            assert [check["profile"]["name"] for check in first["checks"]] == [VISUAL, SPEECH]
            assert len({check["deadline_at"] for check in first["checks"]}) == 1
            assert sql("SELECT COUNT(*) FROM moderation_review_groups") == "1"
            assert sql("SELECT COUNT(*) FROM moderation_checks") == "2"
            assert post("complete", (SPEECH, VISUAL), expected=409)["error"] == "ReviewConflict"
            assert post("complete", version="other-version", expected=409)["error"] == "ReviewConflict"
            assert post("wrong-owner", group=first["id"], tenant="Tenant", expected=404)["error"] == "ReviewNotFound"
            assert post("one", (VISUAL,), expected=400)["error"].startswith("InvalidInput")
            assert post("duplicate", (VISUAL, VISUAL), expected=400)["error"].startswith("InvalidInput")
            assert json.loads(artisan(COMMAND, "stage-group", "app", "tenant", "complete",
                                      "revision-2", f"{VISUAL},{SPEECH}")) == first
            artisan(COMMAND, "find-group", "App", "tenant", first["id"], successful=False)

            cases = [(first, "Succeeded", "allow"),
                     (post("partial", (VISUAL, "group-partial")), "Failed", None),
                     (post("blocked", (VISUAL, "group-blocked")), "Succeeded", "block"),
                     (post("manual", (VISUAL, "group-manual")), "Succeeded", "review")]
            for group, final_state, expected in cases:
                first_check, second_check = group["checks"]
                assert work("submit", first_check)["state"] == "Running"
                tested = work("query", first_check)
                assert tested["decision"] == "allow", tested
                interim = post(None, group=group["id"])
                assert interim["state"] == "Running" and interim["decision"] is None
                assert interim["checks"][1]["state"] == "Pending"
                work("submit", second_check)
                work("query", second_check)
                final = post(None, group=group["id"])
                assert (final["state"], final["decision"]) == (final_state, expected), final
                assert json.loads(artisan(COMMAND, "find-group", "app", "tenant", group["id"])) == final
                assert post(None, group=group["id"]) == final
            assert sql("SELECT COUNT(*) FROM moderation_review_groups") == "4"
            assert sql("SELECT COUNT(*) FROM moderation_checks") == "8"
            assert sql("SELECT COUNT(*) FROM jobs") == sql("SELECT COUNT(*) FROM failed_jobs") == "0"
        finally:
            HTTP["stop"](app)
    artisan("migrate:rollback", "--step=2", "--force", successful=False)
    assert sql("SELECT COUNT(*) FROM moderation_review_groups") == "4"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    driver, url = DATABASE["database_settings"]()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-groups-") as root:
        exercise(args.binary.resolve(), driver, url, Path(root))
    print(f"{driver}: 复合审核 8 并发请求、事务回滚、16 个独立 Worker 和完整覆盖门禁通过")
