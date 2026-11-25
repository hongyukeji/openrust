"""用真实 HTTP、Artisan 和独立 Worker 验收共享账户提交和查单速率。"""

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
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
RATE_TABLE = "moderation_account_rate_buckets"
LIMITS = {"sightengine": {"original-account": {"submit": 2, "query": 1, "window_seconds": 180}}}


def bucket(app, operation, account):
    identified = hashlib.sha256(app.encode()).hexdigest()
    return hashlib.sha256(f"{identified}\nsightengine\n{account}\n{operation}".encode()).hexdigest()


def exercise(binary, driver, url, root, storage_env, storage):
    version, _, original, _ = VIDEO["versioned_clips"](root, storage)
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), SUBMISSION["PausedSupplier"])
    supplier.lock, supplier.mode = threading.Lock(), "accepted"
    supplier.expected_secret, supplier.gate = VIDEO["SECRET"], None
    supplier.query_mode, supplier.query_gate = "wait", None
    supplier.posted, supplier.queried, supplier.stops = [], [], []
    supplier.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    serving = threading.Thread(target=supplier.serve_forever, daemon=True)
    serving.start()

    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    db = root / "rate.sqlite"
    if driver == "sqlite":
        db.touch()
        os.environ["OPENRUST_SMS_SQLITE_PATH"] = str(db)
    mount = root / "credentials"
    mount.mkdir(mode=0o700)
    credential = os.open(mount / "sightengine.r1.json", os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o400)
    with os.fdopen(credential, "w") as stream:
        json.dump({"api_user": "local-sightengine-account", "api_secret": VIDEO["SECRET"]}, stream)
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
        "MODERATION_FIXTURE_ACCOUNT_RATES": json.dumps(LIMITS),
    }
    if url:
        env["DB_URL"] = url
    else:
        env.pop("DB_URL", None)

    def artisan(*tokens, successful=True, extra=None):
        result = subprocess.run([str(binary), "artisan", *tokens], env=env | (extra or {}),
                                capture_output=True, text=True, timeout=35)
        assert VIDEO["SECRET"] not in result.stdout + result.stderr
        assert (result.returncode == 0) == successful, result.stdout + result.stderr
        return result.stdout + ("" if successful else result.stderr)

    def command(action, review, app="app", tenant="tenant", **kwargs):
        return json.loads(artisan(COMMAND, action, app, tenant, review["id"], **kwargs))

    def stage(intent, app="app", tenant="tenant", profile="sightengine-videos"):
        return json.loads(artisan(COMMAND, "stage", app, tenant, intent, version,
                                  extra={"MODERATION_FIXTURE_DURABLE_VIDEO_PROFILE": profile}))

    def post(action, review, tenant="tenant", expected=200):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=20)
        try:
            connection.request("POST", "/api/drivers/moderation/sightengine-video", headers={
                "Accept": "application/json", "X-Fixture-Tenant": tenant,
                f"X-Fixture-{action.title()}-Review": review["id"],
            })
            response = connection.getresponse()
            body = response.read()
            assert response.status == expected and VIDEO["SECRET"].encode() not in body, (response.status, body)
            return json.loads(body)
        finally:
            connection.close()

    def worker(action, review, tenant="tenant"):
        assert command(f"queue-{action}", review, tenant=tenant) == {"queued": True}
        payload = sql("SELECT payload FROM jobs")
        assert review["id"] in payload and VIDEO["SECRET"] not in payload
        artisan("queue:work", "database", "--once")
        return json.loads(outcome.read_text())

    def sql(query):
        if driver == "sqlite":
            with sqlite3.connect(db) as connection:
                return "\n".join("\t".join(map(str, row)) for row in connection.execute(query))
        return DATABASE["database_rows"](DATABASE["database_container"](), query).strip()

    def meter(operation, app="app", account="original-account"):
        key = bucket(app, operation, account)
        return sql(f"SELECT used, reset_at FROM {RATE_TABLE} WHERE id = '{key}'")

    def place_due(review, at=1):
        sql(f"UPDATE moderation_checks SET available_at = {at} WHERE id = '{review['check_id']}'")

    def restart_window(operation):
        key = bucket("app", operation, "original-account")
        sql(f"UPDATE {RATE_TABLE} SET reset_at = 1 WHERE id = '{key}'")

    try:
        artisan("migrate", "--force")
        artisan("config:cache")
        artisan("route:cache")
        assert VIDEO["SECRET"] not in (root / "config.json").read_text()
        first = stage("rate-first")
        second = stage("rate-second", tenant="tenant-b", profile="same-account-videos")
        third = stage("rate-third")
        assert first["profile"]["account"] == second["profile"]["account"]
        assert first["profile"]["policy_revision"] != second["profile"]["policy_revision"]
        with (root / "http.log").open("w+") as log:
            application = HTTP["serve"](binary, env, log)
            try:
                with ThreadPoolExecutor(max_workers=8) as executor:
                    results = list(executor.map(lambda _: post("submit", first), range(8)))
                assert all(item["state"] == "Running" for item in results), results
                assert command("find", first)["submission"]["state"] == "Submitted"
                assert supplier.posted == [(version, original)]
                assert meter("submit").split()[0] == "1" and not meter("query")

                with ThreadPoolExecutor(max_workers=2) as executor:
                    left = executor.submit(worker, "submit", second, "tenant-b")
                    right = executor.submit(post, "submit", third)
                    states = ((left.result(), "tenant-b"), (right.result(), "tenant"))
                sent, waiting = sorted(states, key=lambda item: item[0]["submission"]["state"], reverse=True)
                assert sent[0]["submission"]["state"] == "Submitted", states
                assert waiting[0]["submission"]["state"] == "Ready" and waiting[0]["state"] == "Pending", states
                assert supplier.posted == [(version, original)] * 2
                count, reset = map(int, meter("submit").split())
                assert count == 2 and reset > time.time()
                assert sql("SELECT available_at FROM moderation_checks "
                           f"WHERE id = '{waiting[0]['check_id']}'") == str(reset)
                assert worker("submit", waiting[0], waiting[1]) == waiting[0] and len(supplier.posted) == 2

                separate = stage("independent-provider-account", profile="separate-account-videos")
                assert post("submit", separate)["submission"]["state"] == "Submitted"
                assert meter("submit", account="separate-account").split()[0] == "1"
                other_app = stage("independent-app", app="another-app")
                assert command("submit", other_app, app="another-app")["submission"]["state"] == "Submitted"
                assert meter("submit", app="another-app").split()[0] == "1"
                assert meter("submit").split()[0] == "2" and len(supplier.posted) == 4

                restart_window("submit")
                place_due(waiting[0])
                assert worker("submit", waiting[0], waiting[1])["submission"]["state"] == "Submitted"
                assert meter("submit").split()[0] == "1" and len(supplier.posted) == 5

                for item in (first, second):
                    place_due(item)
                with ThreadPoolExecutor(max_workers=2) as executor:
                    left = executor.submit(post, "query", first)
                    right = executor.submit(worker, "query", second, "tenant-b")
                    states = ((left.result(), "tenant"), (right.result(), "tenant-b"))
                queried, waiting = sorted(states, key=lambda item: item[0]["query_attempts"], reverse=True)
                assert queried[0]["query_attempts"] == 1 and waiting[0]["query_attempts"] == 0, states
                count, reset = map(int, meter("query").split())
                assert count == 1 and reset > time.time()
                assert waiting[0]["next_query_at"] == reset and waiting[0]["query_failure"] is None
                assert len(supplier.queried) == 1 and worker("query", waiting[0], waiting[1]) == waiting[0]
                assert len(supplier.queried) == 1 and meter("submit").split()[0] == "1"
                restart_window("query")
                place_due(waiting[0])
                assert worker("query", waiting[0], waiting[1])["query_attempts"] == 1
                assert len(supplier.queried) == 2 and meter("query").split()[0] == "1"

                invalid = stage("invalid-policy")
                invalid_env = {
                    "APP_CONFIG_CACHE": str(root / "invalid.json"),
                    "MODERATION_FIXTURE_ACCOUNT_RATES": json.dumps({"sightengine": {"original-account": {"submit": 0}}}),
                }
                artisan("config:cache", extra=invalid_env)
                assert "account_rate_limits" in artisan(COMMAND, "submit", "app", "tenant", invalid["id"],
                                                       successful=False, extra=invalid_env)
                assert command("find", invalid) == invalid and len(supplier.posted) == 5

                sql(f"ALTER TABLE {RATE_TABLE} RENAME TO moderation_rate_fixture_unavailable")
                try:
                    assert post("submit", invalid, expected=502)["error"] == "ReviewStorage"
                    assert command("find", invalid) == invalid and len(supplier.posted) == 5
                finally:
                    sql(f"ALTER TABLE moderation_rate_fixture_unavailable RENAME TO {RATE_TABLE}")
                reserved = meter("submit")
                if driver == "sqlite":
                    sql("CREATE TRIGGER reject_rate_reservation BEFORE UPDATE ON moderation_checks "
                        f"WHEN NEW.id = '{invalid['check_id']}' AND NEW.submission = 'Submitting' "
                        "BEGIN SELECT RAISE(ABORT, 'rate reservation rollback'); END")
                    cleanup = "DROP TRIGGER reject_rate_reservation"
                else:
                    sql("ALTER TABLE moderation_checks ADD CONSTRAINT reject_rate_reservation "
                        f"CHECK (id <> '{invalid['check_id']}' OR submission <> 'Submitting')")
                    cleanup = "ALTER TABLE moderation_checks DROP " + (
                        "CONSTRAINT " if driver == "pgsql" else "CHECK ") + "reject_rate_reservation"
                try:
                    assert post("submit", invalid, expected=502)["error"] == "ReviewStorage"
                    assert command("find", invalid) == invalid and meter("submit") == reserved
                    assert len(supplier.posted) == 5
                finally:
                    sql(cleanup)
                assert post("submit", invalid)["submission"]["state"] == "Submitted"
                assert meter("submit").split()[0] == "2"
                assert sql("SELECT COUNT(*) FROM jobs") == sql("SELECT COUNT(*) FROM failed_jobs") == "0"
                assert supplier.posted == [(version, original)] * 6
            finally:
                HTTP["stop"](application)
            log.seek(0)
            assert VIDEO["SECRET"] not in log.read()
    finally:
        supplier.shutdown()
        supplier.server_close()
        serving.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    driver, url = DATABASE["database_settings"]()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-rate-") as root, \
            VIDEO["VIDEO"]["OBJECTS"]["TOPOLOGY"]["object_storage"](True) as (env, storage):
        exercise(args.binary.resolve(), driver, url, Path(root), env, storage)
    print(f"{driver}: 实际 HTTP、Artisan 和独立 Worker 并发共享送审与查单速率、跨租户和失败保护通过")
