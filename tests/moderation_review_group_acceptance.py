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

    def post(intent, profiles=(VISUAL, SPEECH), expected=200, tenant="tenant",
             version="revision-2", group=None, cancel=False, extra=None):
        headers = {"Accept": "application/json", "X-Fixture-Tenant": tenant}
        if group:
            headers["X-Fixture-Group-Id"] = group
            if cancel:
                headers["X-Fixture-Operation"] = "cancel"
        else:
            headers |= {"X-Fixture-Review-Intent": intent, "X-Fixture-Profiles": ",".join(profiles),
                        "X-Fixture-Version": version}
        headers.update(extra or {})
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=12)
        try:
            connection.request("POST", "/api/drivers/moderation/group", headers=headers)
            response = connection.getresponse()
            data = response.read()
            statuses = expected if isinstance(expected, tuple) else (expected,)
            assert response.status in statuses, (response.status, expected, data[:300])
            return json.loads(data)
        finally:
            connection.close()

    def gate(target, revision="content-v1", expected=200, single=False, tenant="tenant", content_id=None):
        extra = {"X-Fixture-Operation": "decision", "X-Fixture-Content-Revision": revision}
        if single:
            extra["X-Fixture-Review-Unit"] = "single"
        if content_id:
            extra["X-Fixture-Content-Id"] = content_id
        return post(None, group=target, expected=expected, tenant=tenant, extra=extra)

    def record(target, previous, decision, expected=200, revision="content-v1",
               actor="moderator-A", single=False):
        headers = {
            "X-Fixture-Operation": "record", "X-Fixture-Content-Revision": revision,
            "X-Fixture-Decision-Revision": str(previous), "X-Fixture-Decision": decision,
            "X-Fixture-Actor": actor, "X-Fixture-Reason": "Verified editorial exception",
        }
        if single:
            headers["X-Fixture-Review-Unit"] = "single"
        return post(None, group=target, expected=expected, extra=headers)

    def permit(target, version, revision, expected):
        return post(None, group=target, expected=expected, extra={
            "X-Fixture-Operation": "publish", "X-Fixture-Content-Revision": version,
            "X-Fixture-Decision-Revision": str(revision),
        })["permitted"]

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
            assert gate(first["id"]) is None
            assert record(first["id"], 0, "allow", expected=409)["error"] == "ReviewConflict"
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
                assert post(None, group=group["id"], cancel=True) == final

            partial, manual = cases[1][0], cases[3][0]
            assert gate(partial["id"]) is None
            assert record(partial["id"], 0, "allow", expected=409)["error"] == "ReviewConflict"
            assert gate(manual["id"]) == {"decision": "review", "revision": 0}
            assert record(manual["id"], 0, "allow", expected=401, actor="viewer")["error"] == "Unauthorized"
            assert record(manual["id"], 0, "allow", expected=409, revision="content-v2")["error"] == "ReviewConflict"

            def attempt(actor):
                extra = {
                    "X-Fixture-Operation": "record", "X-Fixture-Decision-Revision": "0",
                    "X-Fixture-Decision": "allow", "X-Fixture-Actor": f"moderator-{actor}",
                }
                response = post(None, group=manual["id"], expected=(200, 409), extra=extra)
                return response

            with ThreadPoolExecutor(max_workers=8) as executor:
                results = list(executor.map(attempt, range(8)))
            accepted = [result for result in results if result.get("revision") == 1]
            assert len(accepted) == 1 and sum(result.get("error") == "ReviewConflict" for result in results) == 7
            allow = accepted[0]
            assert allow["decision"] == "allow" and allow["decided_at"] > 0
            assert gate(manual["id"]) == allow
            assert permit(manual["id"], "content-v1", 1, 200)
            assert not permit(manual["id"], "content-v2", 1, 403)
            assert gate(manual["id"], content_id="another-content") is None
            assert gate(manual["id"], tenant="Tenant", expected=404)["error"] == "ReviewNotFound"
            assert gate(manual["id"], revision="content-v2") is None
            assert json.loads(artisan(COMMAND, "group-decision", "app", "tenant", manual["id"],
                                      "content-v1")) == allow
            assert record(manual["id"], 0, "allow", expected=409)["error"] == "ReviewConflict"
            block = record(manual["id"], 1, "block")
            assert block["revision"] == 2 and block["decision"] == "block"
            assert block["reason"] == "Verified editorial exception"
            assert not permit(manual["id"], "content-v1", 1, 403)
            assert gate(manual["id"]) == block and post(None, group=manual["id"])["decision"] == "review"

            first_check = first["checks"][0]
            assert gate(first_check["id"], single=True) == {"decision": "allow", "revision": 0}
            single = record(first_check["id"], 0, "review", single=True)
            assert single["revision"] == 1 and single["decision"] == "review"
            assert json.loads(artisan(COMMAND, "review-decision", "app", "tenant", first_check["id"],
                                      "content-v1")) == single
            assert gate(first["id"]) == {"decision": "allow", "revision": 0}

            current = post("new-content-revision", (VISUAL, SPEECH), version="revision-3",
                           extra={"X-Fixture-Content-Revision": "content-v2"})
            assert gate(current["id"], revision="content-v2") is None
            assert not permit(current["id"], "content-v2", 0, 403)
            assert work("query", manual["checks"][1])["decision"] == "review"
            assert gate(manual["id"]) == block and gate(current["id"], revision="content-v2") is None
            assert sql("SELECT COUNT(*) FROM moderation_review_decisions") == "3"

            stopping = post("stopping", (VISUAL, SPEECH, "group-manual"))
            first_check, running, pending = stopping["checks"]
            work("submit", first_check)
            assert work("query", first_check)["state"] == "Succeeded"
            work("submit", running)
            latest = post(None, group=stopping["id"])
            assert [check["state"] for check in latest["checks"]] == ["Succeeded", "Running", "Pending"]
            if driver == "sqlite":
                sql("CREATE TRIGGER reject_group_freeze BEFORE UPDATE ON moderation_reviews "
                    f"WHEN NEW.id = '{pending['id']}' AND NEW.state = 'Cancelled' "
                    "BEGIN SELECT RAISE(ABORT, 'group cancellation rollback'); END")
                remove = "DROP TRIGGER reject_group_freeze"
            else:
                sql("ALTER TABLE moderation_reviews ADD CONSTRAINT reject_group_freeze "
                    f"CHECK (id <> '{pending['id']}' OR state <> 'Cancelled')")
                remove = "ALTER TABLE moderation_reviews DROP " + (
                    "CONSTRAINT " if driver == "pgsql" else "CHECK ") + "reject_group_freeze"
            assert post(None, group=stopping["id"], cancel=True, expected=502)["error"] == "ReviewStorage"
            assert post(None, group=stopping["id"]) == latest
            sql(remove)
            for action, check in (("queue-query", running), ("queue-submit", pending)):
                artisan(COMMAND, action, "app", "tenant", check["id"])
            with ThreadPoolExecutor(max_workers=8) as executor:
                requests = [executor.submit(post, None, group=stopping["id"], cancel=True) for _ in range(8)]
                assert all(request.result()["state"] == "Cancelled" for request in requests)
            frozen = post(None, group=stopping["id"])
            assert [check["state"] for check in frozen["checks"]] == ["Succeeded", "Cancelled", "Cancelled"]
            assert [check["provider_cancellation"] for check in frozen["checks"]] == [None, "unsupported", "no_task"]
            assert frozen["checks"][0] == latest["checks"][0] and frozen["decision"] is None
            for _ in range(2):
                artisan("queue:work", "database", "--once")
            assert json.loads(artisan(COMMAND, "cancel-group", "app", "tenant", stopping["id"])) == frozen
            assert post(None, group=stopping["id"]) == frozen

            assert sql("SELECT COUNT(*) FROM moderation_review_groups") == "6"
            assert sql("SELECT COUNT(*) FROM moderation_checks") == "13"
            assert sql("SELECT COUNT(*) FROM jobs") == sql("SELECT COUNT(*) FROM failed_jobs") == "0"
        finally:
            HTTP["stop"](app)
    artisan("migrate:rollback", "--force",
            "--path=vendor/openrust/content-moderation/database/migrations/"
            "2026_11_26_000010_create_moderation_review_decisions_table.rs", successful=False)
    artisan("migrate:rollback", "--force",
            "--path=vendor/openrust/content-moderation/database/migrations/"
            "2026_11_26_000008_create_moderation_review_groups_table.rs", successful=False)
    assert sql("SELECT COUNT(*) FROM moderation_review_groups") == "6"
    assert sql("SELECT COUNT(*) FROM moderation_review_decisions") == "3"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    driver, url = DATABASE["database_settings"]()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-groups-") as root:
        exercise(args.binary.resolve(), driver, url, Path(root))
    print(f"{driver}: 复合审核并发落单、取消及裁决、事务回滚、22 个独立 Worker 和修订发布门禁通过")
