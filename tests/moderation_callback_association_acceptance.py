"""真实三库检验 Sightengine 提前回调延期、关联和原任务确认。"""

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
CALLBACK = runpy.run_path(str(ROOT / "tests/moderation_callback_acceptance.py"))
SUBMISSION = runpy.run_path(str(ROOT / "tests/moderation_submission_acceptance.py"))
DATABASE, HTTP = CALLBACK["DATABASE"], CALLBACK["HTTP"]
VIDEO = SUBMISSION["VIDEO"]
REVIEW = "drivers:moderation-review"
JOB = CALLBACK["JOB"]


def exercise(binary, driver, url, root, storage_env, storage):
    version, _, original, _ = VIDEO["versioned_clips"](root, storage)
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), SUBMISSION["PausedSupplier"])
    supplier.lock, supplier.mode = threading.Lock(), "accepted"
    supplier.expected_secret = VIDEO["SECRET"]
    supplier.gate, supplier.query_gate, supplier.query_mode = None, None, "finished"
    supplier.posted, supplier.queried, supplier.stops = [], [], []
    supplier.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    serving = threading.Thread(target=supplier.serve_forever, daemon=True)
    serving.start()
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    db_path = root / "association.sqlite"
    if driver == "sqlite":
        db_path.touch()
        os.environ["OPENRUST_SMS_SQLITE_PATH"] = str(db_path)
    mount = root / "credentials"
    mount.mkdir(mode=0o700)
    credentials = [
        ("sight-callback.r1.json", {"signing_secret": CALLBACK["OLD"]}),
        ("sight-callback.r2.json", {"signing_secret": CALLBACK["CURRENT"]}),
        ("other-callback.r1.json", {"signing_secret": CALLBACK["OTHER"]}),
        ("sightengine.r1.json", {"api_user": "local-sightengine-account", "api_secret": VIDEO["SECRET"]}),
    ]
    for filename, contents in credentials:
        descriptor = os.open(mount / filename, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o400)
        with os.fdopen(descriptor, "w") as file:
            json.dump(contents, file)
    endpoint = f"http://127.0.0.1:{supplier.server_port}"
    env = os.environ | storage_env | {
        "APP_BASE_PATH": str(root), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "DB_CONNECTION": driver, "QUEUE_CONNECTION": "database",
        "DB_DATABASE": str(db_path) if driver == "sqlite" else "openrust_acceptance",
        "MODERATION_CREDENTIAL_DIRECTORY": str(mount),
        "MODERATION_FIXTURE_SIGHTENGINE_VIDEO": endpoint, "MODERATION_FIXTURE_PIN_ENDPOINT": endpoint,
        "MODERATION_FIXTURE_SIGHTENGINE_VIDEO_TIMEOUT": "18000",
        "AWS_TEMPORARY_URL": VIDEO["ORIGIN"], "MODERATION_FIXTURE_IMAGE_ORIGIN": VIDEO["ORIGIN"],
        "MODERATION_FIXTURE_QUERY_INTERVAL": "300", "MODERATION_FIXTURE_RECOVERY_QUEUE": "database",
        "MODERATION_CALLBACK_FIXTURE_ASSOCIATE": "1",
    }
    if url:
        env["DB_URL"] = url
    else:
        env.pop("DB_URL", None)

    def artisan(*tokens):
        result = subprocess.run([str(binary), "artisan", *tokens], env=env,
                                capture_output=True, text=True, timeout=35)
        assert all(secret not in result.stdout + result.stderr for secret in (*CALLBACK["SECRETS"], VIDEO["SECRET"]))
        assert result.returncode == 0, result.stdout + result.stderr
        return result.stdout

    def read(review):
        return json.loads(artisan(REVIEW, "find", "app", "tenant", review["id"]))

    def consume(queue=JOB):
        artisan("queue:work", "database", f"--queue={queue}", "--once")

    def sql(query):
        if driver == "sqlite":
            with sqlite3.connect(db_path) as connection:
                rows = connection.execute(query).fetchall()
                return "\n".join("\t".join("" if value is None else str(value) for value in row) for row in rows)
        return DATABASE["database_rows"](DATABASE["database_container"](), query).strip()

    def callback(status, marker, *, other=False):
        body = json.dumps({"media": {"id": VIDEO["TASK"]}, "data": {"status": status}, "marker": marker}).encode()
        secret = CALLBACK["OTHER"] if other else CALLBACK["CURRENT"]
        path = "/api/drivers/moderation/callback" + ("-other" if other else "")
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=12)
        try:
            connection.request("POST", path, body, {
                "Accept": "application/json", "Content-Type": "application/json",
                "Sightengine-Signature": CALLBACK["callback_header"](body, secret),
            })
            response = connection.getresponse()
            answer = response.read()
            assert response.status == 200, (response.status, answer[:300])
        finally:
            connection.close()

    try:
        artisan("migrate", "--force")
        artisan("config:cache")
        artisan("route:cache")
        early = json.loads(artisan(REVIEW, "stage", "app", "tenant", "early-callback", version))
        with (root / "http.log").open("w+") as log:
            app = HTTP["serve"](binary, env, log)
            try:
                with ThreadPoolExecutor(max_workers=1) as executor:
                    reached, release = supplier.gate = (threading.Event(), threading.Event())
                    submission = executor.submit(artisan, REVIEW, "submit", "app", "tenant", early["id"])
                    try:
                        assert reached.wait(8), "供应商未收到原素材"
                        callback("finished", "early")
                        receipt = sql("SELECT id FROM webhook_calls")
                        assert receipt and read(early)["callback_receipt"] is None
                        consume()
                        assert sql("SELECT processing_deferrals FROM webhook_calls") == "1"
                        assert sql("SELECT COUNT(*) FROM webhook_calls WHERE processing_next_at IS NOT NULL") == "1"
                        assert sql("SELECT COUNT(*) FROM webhook_calls WHERE processed_at IS NOT NULL") == "0"
                        callback("finished", "early")
                        assert sql("SELECT COUNT(*) FROM webhook_calls") == "1"
                    finally:
                        release.set()
                    accepted = json.loads(submission.result(20))
                    supplier.gate = None
                assert accepted["submission"]["task"] == VIDEO["TASK"] and accepted["callback_receipt"] is None
                assert not supplier.queried

                assert artisan("drivers:moderation-wake-callback", receipt).strip() == "true"
                assert artisan("drivers:moderation-wake-callback", receipt).strip() == "false"
                consume()
                associated = read(early)
                assert associated["callback_receipt"] == receipt
                assert associated["next_query_at"] < accepted["next_query_at"]
                assert associated["query_attempts"] == 0 and associated["decision"] is None
                assert sql("SELECT COUNT(*) FROM webhook_calls WHERE processed_at IS NOT NULL") == "1"
                # 即使队列仍保留旧的延时副本，重复消费也不会再次关联或查单。
                sql(f"UPDATE jobs SET available_at = 1 WHERE queue = '{JOB}'")
                if sql(f"SELECT COUNT(*) FROM jobs WHERE queue = '{JOB}'") != "0":
                    consume()
                assert read(early) == associated and not supplier.queried
                artisan("schedule:run")
                consume("moderation-recovery")
                checked = read(early)
                assert checked["query_attempts"] == 1 and checked["query_failure"] == "incomplete_coverage"
                assert checked["decision"] is None and checked["callback_receipt"] == receipt
                assert supplier.queried == [VIDEO["TASK"]]

                callback("finished", "late")
                consume()
                assert read(early) == checked
                callback("finished", "other", other=True)
                consume()
                assert read(early) == checked
                assert supplier.queried == [VIDEO["TASK"]]

                duplicate = json.loads(artisan(REVIEW, "stage", "app", "tenant", "ambiguous-task", version))
                accepted_duplicate = json.loads(artisan(REVIEW, "submit", "app", "tenant", duplicate["id"]))
                assert accepted_duplicate["submission"]["task"] == VIDEO["TASK"]
                callback("finished", "ambiguous")
                consume()
                assert read(duplicate)["callback_receipt"] is None and supplier.queried == [VIDEO["TASK"]]
            finally:
                HTTP["stop"](app)
        assert supplier.posted == [(version, original), (version, original)]
        assert sql("SELECT COUNT(*) FROM failed_jobs") == "2"
        assert sql("SELECT COUNT(*) FROM jobs") == "0"
    finally:
        supplier.shutdown()
        supplier.server_close()
        serving.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    driver, url = DATABASE["database_settings"]()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-callback-association-") as root, \
            VIDEO["VIDEO"]["OBJECTS"]["TOPOLOGY"]["object_storage"](True) as (env, storage):
        exercise(args.binary.resolve(), driver, url, Path(root), env, storage)
    print(f"{driver}: 实际 HTTP、Artisan、Worker 提前回调、延期唤醒、范围、查单与覆盖通过")
