"""真实三库检验点播提前回调延期、关联和原任务确认。"""

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
from urllib.parse import urlsplit


ROOT = Path(__file__).resolve().parents[1]
CALLBACK = runpy.run_path(str(ROOT / "tests/moderation_callback_acceptance.py"))
SUBMISSION = runpy.run_path(str(ROOT / "tests/moderation_submission_acceptance.py"))
DATABASE, HTTP = CALLBACK["DATABASE"], CALLBACK["HTTP"]
VIDEO = SUBMISSION["VIDEO"]
REVIEW = "drivers:moderation-review"
JOB = CALLBACK["JOB"]
TENCENT_SEEDS = ("vm-local-test-seed-01", "vm-local-test-seed-02", "vm-local-test-seed-other")


def exercise(binary, driver, url, root, storage_env, storage, tencent):
    version, _, original, _ = VIDEO["versioned_clips"](root, storage)
    vendor = runpy.run_path(str(ROOT / "tests/moderation_tencent_video_acceptance.py")) if tencent else None
    supplier = http.server.ThreadingHTTPServer(
        ("127.0.0.1", 0), vendor["Supplier"] if tencent else SUBMISSION["PausedSupplier"])
    supplier.lock, supplier.mode = threading.Lock(), "accepted"
    supplier.expected_secret = VIDEO["SECRET"]
    supplier.gate, supplier.query_gate, supplier.query_mode = None, None, "finished"
    supplier.posted, supplier.queried, supplier.stops = [], [], []
    supplier.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    if tencent:
        supplier.posted = supplier.submissions = []
        supplier.references, supplier.cancelled = set(), set()
        supplier.queries, supplier.cancellations, supplier.unexpected = [], [], []
        supplier.submit_mode = "accepted"
        callback_url = "https://callback.example.test/api/drivers/moderation/tencent-callback"
        supplier.expected_callback = (callback_url, TENCENT_SEEDS[1])
        task = vendor["TASK"]
    else:
        task = VIDEO["TASK"]

    def queried():
        return [value for value, show in supplier.queries if show] if tencent else supplier.queried

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
    tencent_secret, tencent_token = ("", "")
    if tencent:
        identity = "fixture-tencent-id"
        tencent_secret, tencent_token = vendor["TC3"]["CREDENTIALS"][identity]
        for filename, seed in zip(("tencent-video.r1.json", "tencent-video.r2.json", "tencent-other.r1.json"),
                                  TENCENT_SEEDS, strict=True):
            credentials.append((filename, {"secret_id": identity, "secret_key": tencent_secret,
                                            "security_token": tencent_token, "video_callback_seed": seed}))
    protected = (*CALLBACK["SECRETS"], VIDEO["SECRET"]) + (
        (*TENCENT_SEEDS, tencent_secret, tencent_token) if tencent else ())
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
    if tencent:
        env |= {
            "MODERATION_FIXTURE_TENCENT_VIDEO": endpoint,
            "MODERATION_FIXTURE_TENCENT_CALLBACK_URL": callback_url,
            "MODERATION_FIXTURE_DURABLE_VIDEO_PROFILE": "tencent-videos",
            "MODERATION_CALLBACK_FIXTURE_TENCENT": "1",
        }
    if url:
        env["DB_URL"] = url
    else:
        env.pop("DB_URL", None)

    def artisan(*tokens):
        result = subprocess.run([str(binary), "artisan", *tokens], env=env,
                                capture_output=True, text=True, timeout=35)
        assert all(secret not in result.stdout + result.stderr for secret in protected)
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

    def callback(status, marker, *, other=False, previous=False, fault=None, policy="video_profile",
                 kind="VIDEO"):
        if tencent:
            data = {"TaskId": task, "Type": kind, "Status": status, "BizType": policy,
                    "AudioText": marker, "Suggestion": "Pass"}
            body = json.dumps(data, indent=1).encode()
            seed = TENCENT_SEEDS[2 if other else 0 if previous else 1]
            signature = hashlib.sha256(seed.encode() + body).hexdigest()
            header = "X-Signature"
            path = "/api/drivers/moderation/tencent-callback"
        else:
            body = json.dumps({"media": {"id": task}, "data": {"status": status}, "marker": marker}).encode()
            secret = CALLBACK["OTHER"] if other else CALLBACK["CURRENT"]
            signature = CALLBACK["callback_header"](body, secret)
            header = "Sightengine-Signature"
            path = "/api/drivers/moderation/callback"
        if fault == "changed":
            body += b" "
        path += "-other" if other else ""
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=12)
        try:
            connection.putrequest("POST", path)
            connection.putheader("Content-Type", "application/json")
            connection.putheader("Accept", "application/json")
            connection.putheader("Content-Length", str(len(body)))
            if fault != "missing":
                connection.putheader(header, signature)
                if fault == "duplicate":
                    connection.putheader(header, signature)
            connection.endheaders(body)
            response = connection.getresponse()
            answer = response.read()
            assert response.status == (500 if fault else 200), (response.status, answer[:300])
            assert all(secret.encode() not in answer for secret in protected)
        finally:
            connection.close()

    try:
        artisan("migrate", "--force")
        artisan("config:cache")
        artisan("route:cache")
        assert all(secret not in (root / "config.json").read_text() for secret in protected)
        finish = "FINISH" if tencent else "finished"
        early = json.loads(artisan(REVIEW, "stage", "app", "tenant", "early-callback", version))
        with (root / "http.log").open("w+") as log:
            app = HTTP["serve"](binary, env, log)
            try:
                with ThreadPoolExecutor(max_workers=1) as executor:
                    reached, release = supplier.gate = (threading.Event(), threading.Event())
                    submission = executor.submit(artisan, REVIEW, "submit", "app", "tenant", early["id"])
                    try:
                        assert reached.wait(8), "供应商未收到原素材"
                        if tencent:
                            for defect in ("changed", "missing", "duplicate"):
                                callback(finish, "tampered", fault=defect)
                            assert sql("SELECT COUNT(*) FROM webhook_calls") == "0"
                            callback("RUNNING", "fragment")
                            consume()
                            assert queried() == [] and read(early)["callback_receipt"] is None
                        callback(finish, "early", previous=tencent)
                        receipt = sql("SELECT id FROM webhook_calls WHERE processed_at IS NULL")
                        assert receipt and read(early)["callback_receipt"] is None
                        consume()
                        assert sql("SELECT processing_deferrals FROM webhook_calls WHERE processed_at IS NULL") == "1"
                        assert sql("SELECT COUNT(*) FROM webhook_calls WHERE processing_next_at IS NOT NULL") == "1"
                        assert sql("SELECT COUNT(*) FROM webhook_calls WHERE processed_at IS NOT NULL") == str(int(tencent))
                        callback(finish, "early")
                        assert sql("SELECT COUNT(*) FROM webhook_calls") == str(1 + int(tencent))
                    finally:
                        release.set()
                    accepted = json.loads(submission.result(20))
                    supplier.gate = None
                assert accepted["submission"]["task"] == task and accepted["callback_receipt"] is None
                assert not queried()

                assert artisan("drivers:moderation-wake-callback", receipt).strip() == "true"
                assert artisan("drivers:moderation-wake-callback", receipt).strip() == "false"
                consume()
                associated = read(early)
                assert associated["callback_receipt"] == receipt
                assert associated["next_query_at"] < accepted["next_query_at"]
                assert associated["query_attempts"] == 0 and associated["decision"] is None
                assert sql("SELECT COUNT(*) FROM webhook_calls WHERE processed_at IS NOT NULL") == str(1 + int(tencent))
                # 即使队列仍保留旧的延时副本，重复消费也不会再次关联或查单。
                sql(f"UPDATE jobs SET available_at = 1 WHERE queue = '{JOB}'")
                if sql(f"SELECT COUNT(*) FROM jobs WHERE queue = '{JOB}'") != "0":
                    consume()
                assert read(early) == associated and not queried()
                artisan("schedule:run")
                consume("moderation-recovery")
                checked = read(early)
                assert checked["query_attempts"] == 1 and checked["query_failure"] == "incomplete_coverage"
                assert checked["decision"] is None and checked["callback_receipt"] == receipt
                assert queried() == [task]

                callback(finish, "late")
                consume()
                assert read(early) == checked
                callback(finish, "other", other=True)
                consume()
                assert read(early) == checked
                assert queried() == [task]
                if tencent:
                    callback(finish, "policy", policy="different_policy")
                    consume()
                    callback(finish, "live", kind="LIVE_VIDEO")
                    consume()
                    assert read(early) == checked and queried() == [task]

                duplicate = json.loads(artisan(REVIEW, "stage", "app", "tenant", "ambiguous-task", version))
                accepted_duplicate = json.loads(artisan(REVIEW, "submit", "app", "tenant", duplicate["id"]))
                assert accepted_duplicate["submission"]["task"] == task
                callback(finish, "ambiguous")
                consume()
                assert read(duplicate)["callback_receipt"] is None and queried() == [task]
            finally:
                HTTP["stop"](app)
        assert supplier.posted == [(version, original), (version, original)]
        assert sql("SELECT COUNT(*) FROM failed_jobs") == str(4 if tencent else 2)
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
    tencent = os.environ.get("MODERATION_TEST_CALLBACK_TENCENT") == "1"
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-callback-association-") as root, \
            VIDEO["VIDEO"]["OBJECTS"]["TOPOLOGY"]["object_storage"](True) as (env, storage):
        exercise(args.binary.resolve(), driver, url, Path(root), env, storage, tencent)
    provider = "腾讯云" if tencent else "Sightengine"
    print(f"{driver}: {provider} 实际 HTTP、Artisan、Worker 提前回调、延期唤醒、范围、查单与覆盖通过")
