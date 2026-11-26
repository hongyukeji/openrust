"""真实三库检验点播提前回调延期、关联和原任务确认。"""

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
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
from urllib.parse import urlencode, urlsplit


ROOT = Path(__file__).resolve().parents[1]
CALLBACK = runpy.run_path(str(ROOT / "tests/moderation_callback_acceptance.py"))
INGRESS = runpy.run_path(str(ROOT / "tests/support/moderation_callback_ingress.py"))
SUBMISSION = runpy.run_path(str(ROOT / "tests/moderation_submission_acceptance.py"))
DATABASE, HTTP = CALLBACK["DATABASE"], CALLBACK["HTTP"]
VIDEO = SUBMISSION["VIDEO"]
REVIEW = "drivers:moderation-review"
JOB = CALLBACK["JOB"]
TENCENT_SEEDS = ("vm-local-test-seed-01", "vm-local-test-seed-02", "vm-local-test-seed-other")
ALIYUN_SEEDS = ("aliyun_video_seed_01", "aliyun_video_seed_02", "aliyun_video_seed_other")
ALIYUN_UIDS = ("1234500000000000", "9999900000000000")


def exercise(binary, driver, url, root, storage_env, storage, mode, *, test_late=False, test_purge=False):
    version, _, original, _ = VIDEO["versioned_clips"](root, storage)
    tencent, aliyun = mode == "tencent", mode == "aliyun"
    cloud = tencent or aliyun
    vendor = runpy.run_path(str(ROOT / f"tests/moderation_{mode}_video_acceptance.py")) if cloud else None
    supplier = http.server.ThreadingHTTPServer(
        ("127.0.0.1", 0), vendor["Supplier"] if cloud else SUBMISSION["PausedSupplier"])
    supplier.lock, supplier.mode = threading.Lock(), "accepted"
    supplier.expected_secret = VIDEO["SECRET"]
    supplier.gate, supplier.query_gate, supplier.query_mode = None, None, "finished"
    supplier.posted, supplier.queried, supplier.stops = [], [], []
    supplier.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    if cloud:
        supplier.posted = supplier.submissions = []
        supplier.references, supplier.cancelled = set(), set()
        supplier.nonces = set()
        supplier.queries, supplier.cancellations, supplier.unexpected = [], [], []
        supplier.submit_mode = "accepted"
        callback_url = f"https://callback.example.test/api/drivers/moderation/{mode}-callback"
        seeds = TENCENT_SEEDS if tencent else ALIYUN_SEEDS
        supplier.expected_callback = (callback_url, seeds[1])
        task = vendor["TASK"]
    else:
        task = VIDEO["TASK"]

    def queried():
        if tencent:
            return [value for value, show in supplier.queries if show]
        return supplier.queries if aliyun else supplier.queried

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
    protected = (*CALLBACK["SECRETS"], VIDEO["SECRET"])
    if tencent:
        identity = "fixture-tencent-id"
        tencent_secret, tencent_token = vendor["TC3"]["CREDENTIALS"][identity]
        for filename, seed in zip(("tencent-video.r1.json", "tencent-video.r2.json", "tencent-other.r1.json"),
                                  TENCENT_SEEDS, strict=True):
            credentials.append((filename, {"secret_id": identity, "secret_key": tencent_secret,
                                            "security_token": tencent_token, "video_callback_seed": seed}))
        protected += (*seeds, tencent_secret, tencent_token)
    if aliyun:
        identity, (secret, token) = next(iter(vendor["ALIYUN"]["CREDENTIALS"].items()))
        for filename, seed in zip(("aliyun-video.r1.json", "aliyun-video.r2.json", "aliyun-other.r1.json"),
                                  seeds, strict=True):
            credentials.append((filename, {"access_key_id": identity, "access_key_secret": secret,
                                            "security_token": token, "video_callback_seed": seed}))
        protected += (*seeds, secret, token)
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
    if cloud:
        env |= {
            f"MODERATION_FIXTURE_{mode.upper()}_VIDEO": endpoint,
            f"MODERATION_FIXTURE_{mode.upper()}_CALLBACK_URL": callback_url,
            "MODERATION_FIXTURE_DURABLE_VIDEO_PROFILE": f"{mode}-videos",
            f"MODERATION_CALLBACK_FIXTURE_{mode.upper()}": "1",
        }
    if url:
        env["DB_URL"] = url
    else:
        env.pop("DB_URL", None)

    def artisan(*tokens, successful=True):
        result = subprocess.run([str(binary), "artisan", *tokens], env=env,
                                capture_output=True, text=True, timeout=35)
        assert all(secret not in result.stdout + result.stderr for secret in protected)
        assert (result.returncode == 0) == successful, result.stdout + result.stderr
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
        content_type = "application/json"
        if tencent:
            data = {"TaskId": task, "Type": kind, "Status": status, "BizType": policy,
                    "AudioText": marker, "Suggestion": "Pass"}
            body = json.dumps(data, indent=1).encode()
            seed = TENCENT_SEEDS[2 if other else 0 if previous else 1]
            signature = hashlib.sha256(seed.encode() + body).hexdigest()
            header = "X-Signature"
            path = "/api/drivers/moderation/tencent-callback"
        elif aliyun:
            content_type = "application/x-www-form-urlencoded; charset=UTF-8"
            content = json.dumps({"Data": {"TaskId": task, "RiskLevel": "none"},
                                  "Code": status, "Message": f"{marker} 字幕 + & /"},
                                 indent=1, ensure_ascii=False)
            seed = seeds[2 if other else 0 if previous else 1]
            uid = ALIYUN_UIDS[int(other)] if fault != "wrong-uid" else ALIYUN_UIDS[1]
            checksum = hashlib.sha256((uid + seed + content).encode()).hexdigest()
            params = [("content", content + (" " if fault == "changed" else ""))]
            if fault != "missing":
                params.append(("checksum", checksum))
                if fault == "duplicate":
                    params.append(("checksum", checksum))
            body = urlencode(params).encode()
            if fault == "wrong-type":
                content_type = "text/plain"
            path = "/api/drivers/moderation/aliyun-callback"
            header, signature = None, None
        else:
            body = json.dumps({"media": {"id": task}, "data": {"status": status}, "marker": marker}).encode()
            secret = CALLBACK["OTHER"] if other else CALLBACK["CURRENT"]
            signature = CALLBACK["callback_header"](body, secret)
            header = "Sightengine-Signature"
            path = "/api/drivers/moderation/callback"
        if fault == "changed" and not aliyun:
            body += b" "
        path += "-other" if other else ""
        connection = open_callback()
        try:
            connection.putrequest("POST", path)
            connection.putheader("Content-Type", content_type)
            connection.putheader("Accept", "application/json")
            connection.putheader("Content-Length", str(len(body)))
            if header and fault != "missing":
                connection.putheader(header, signature)
                if fault == "duplicate":
                    connection.putheader(header, signature)
            connection.endheaders(body)
            response = connection.getresponse()
            answer = response.read()
            expected = 500 if fault else 200
            assert response.status == expected, (fault, expected, response.status, answer[:300])
            assert all(secret.encode() not in answer for secret in protected)
        finally:
            connection.close()

    def late_callback(review):
        submitted = json.loads(artisan(REVIEW, "submit", "app", "tenant", review["id"]))
        assert submitted["submission"]["task"] == task
        cancelled = json.loads(artisan(REVIEW, "cancel", "app", "tenant", review["id"]))
        assert cancelled["state"] == "Cancelled" and cancelled["provider_cancellation"] == "acknowledged"
        callback("finished", "late-invalid-signature", fault="changed")
        assert sql("SELECT COUNT(*) FROM moderation_late_notices") == "0"
        callback("finished", "late-first")
        receipt = sql("SELECT id FROM webhook_calls WHERE processed_at IS NULL")
        assert len(receipt) == 36
        consume()
        notices = json.loads(artisan(REVIEW, "late-notices", "app", "tenant", review["id"]))
        assert len(notices) == 1 and notices[0]["kind"] == "signed_callback"
        assert notices[0]["state"] == "Cancelled" and notices[0]["receipt_id"] == receipt
        callback("finished", "late-second")
        consume()
        assert json.loads(artisan(REVIEW, "late-notices", "app", "tenant", review["id"])) == notices
        assert json.loads(artisan(REVIEW, "late-notices", "App", "tenant", review["id"])) == []
        assert sql("SELECT COUNT(*) FROM webhook_calls WHERE processed_at IS NOT NULL") == "2"
        assert sql("SELECT COUNT(*) FROM moderation_late_notices") == "1"
        assert read(review) == cancelled and not queried()
        artisan("schedule:run")
        assert sql("SELECT COUNT(*) FROM jobs") == "0"

    def purge_callback(review):
        artisan(REVIEW, "purge-review", "app", "tenant", review["id"], successful=False)
        submitted = json.loads(artisan(REVIEW, "submit", "app", "tenant", review["id"]))
        assert submitted["submission"]["task"] == task
        artisan(REVIEW, "withdraw-revision", "app", "tenant", "影片/1", "content-v1")
        artisan(REVIEW, "purge-review", "app", "tenant", review["id"], successful=False)
        sql("UPDATE moderation_checks SET available_at = 1 "
            f"WHERE id = '{review['check_id']}'")
        assert json.loads(artisan("moderation:recover", "app", "tenant",
                                  "--queue=moderation-recovery", "--limit=1")) == {"dispatched": 1}
        cancelled = json.loads(artisan(REVIEW, "cancel", "app", "tenant", review["id"]))
        assert cancelled["state"] == "Cancelled" and cancelled["provider_cancellation"] == "acknowledged"
        callback("finished", "before-purge")
        consume()
        assert sql("SELECT COUNT(*) FROM moderation_late_notices") == "1"
        assert json.loads(artisan(REVIEW, "purge-review", "app", "Tenant", review["id"])) == {"deleted": 0}
        assert json.loads(artisan(REVIEW, "purge-review", "app", "tenant", review["id"])) == {"deleted": 1}
        assert json.loads(artisan(REVIEW, "purge-review", "app", "tenant", review["id"])) == {"deleted": 0}
        assert json.loads(artisan(REVIEW, "find", "app", "tenant", review["id"])) is None
        artisan(REVIEW, "stage", "app", "tenant", "early-callback", version, successful=False)
        assert sql("SELECT COUNT(*) FROM moderation_late_notices") == "0"
        assert sql("SELECT COUNT(*) FROM moderation_checks") == sql("SELECT COUNT(*) FROM moderation_reviews") == "0"
        id_digest, scope = sql("SELECT id, scope_key FROM moderation_video_tombstones").split()
        assert len(id_digest) == len(scope) == 64 and task not in id_digest
        artisan("migrate:rollback", "--force",
                "--path=vendor/openrust/content-moderation/database/migrations/"
                "2026_11_26_000018_create_moderation_video_tombstones_table.rs", successful=False)

        consume("moderation-recovery")
        callback("finished", "after-purge-first")
        consume()
        callback("finished", "after-purge-again")
        consume()
        assert sql("SELECT COUNT(*) FROM webhook_calls WHERE processed_at IS NOT NULL") == "3"
        assert sql("SELECT COUNT(*) FROM moderation_reviews") == "0" and not queried()

        other = json.loads(artisan(REVIEW, "stage", "app", "Tenant", "other-scope", version))
        artisan(REVIEW, "submit", "app", "Tenant", other["id"])
        artisan(REVIEW, "withdraw-revision", "app", "Tenant", "影片/1", "content-v1")
        artisan(REVIEW, "cancel", "app", "Tenant", other["id"])
        assert json.loads(artisan(REVIEW, "purge-review", "app", "Tenant", other["id"])) == {"deleted": 1}
        assert sql("SELECT COUNT(*) FROM moderation_video_tombstones") == "2"
        artisan("moderation:prune-tombstones", "app", "tenant", "--limit=0", successful=False)
        assert json.loads(artisan("moderation:prune-tombstones", "app", "tenant")) == {"deleted": 0}

        sql(f"UPDATE moderation_video_tombstones SET expires_at = 1 WHERE scope_key = '{scope}'")
        sql("INSERT INTO moderation_video_tombstones (id, scope_key, expires_at) "
            f"VALUES ('{'f' * 64}', '{scope}', 2)")
        assert json.loads(artisan("moderation:prune-tombstones", "app", "tenant", "--limit=1")) == {"deleted": 1}
        assert json.loads(artisan("moderation:prune-tombstones", "app", "tenant")) == {"deleted": 1}
        assert sql("SELECT COUNT(*) FROM moderation_video_tombstones") == "1"
        callback("finished", "after-prune")
        consume()
        assert sql("SELECT COUNT(*) FROM webhook_calls WHERE processing_next_at IS NOT NULL") == "1"
        assert sql("SELECT COUNT(*) FROM moderation_reviews") == "0" and not queried()
        assert sql("SELECT COUNT(*) FROM failed_jobs") == "0"
        artisan(REVIEW, "stage", "app", "tenant", "unrelated-new-intent", version, successful=False)

    try:
        artisan("migrate", "--force")
        artisan("config:cache")
        artisan("route:cache")
        assert all(secret not in (root / "config.json").read_text() for secret in protected)
        finish = "FINISH" if tencent else "200" if aliyun else "finished"
        early = json.loads(artisan(REVIEW, "stage", "app", "tenant", "early-callback", version))
        with (root / "http.log").open("w+") as log, INGRESS["callback_ingress"](root, port) as open_callback:
            app = HTTP["serve"](binary, env, log)
            try:
                if test_late:
                    late_callback(early)
                    return
                if test_purge:
                    purge_callback(early)
                    return
                with ThreadPoolExecutor(max_workers=1) as executor:
                    reached, release = supplier.gate = (threading.Event(), threading.Event())
                    submission = executor.submit(artisan, REVIEW, "submit", "app", "tenant", early["id"])
                    try:
                        assert reached.wait(8), "供应商未收到原素材"
                        if cloud:
                            defects = ("changed", "missing", "duplicate")
                            if aliyun:
                                defects += ("wrong-uid", "wrong-type")
                            for defect in defects:
                                callback(finish, "tampered", fault=defect)
                            assert sql("SELECT COUNT(*) FROM webhook_calls") == "0"
                            callback("RUNNING" if tencent else 280, "fragment")
                            consume()
                            assert queried() == [] and read(early)["callback_receipt"] is None
                        callback(finish, "early", previous=cloud)
                        receipt = sql("SELECT id FROM webhook_calls WHERE processed_at IS NULL")
                        assert receipt and read(early)["callback_receipt"] is None
                        consume()
                        assert sql("SELECT processing_deferrals FROM webhook_calls WHERE processed_at IS NULL") == "1"
                        assert sql("SELECT COUNT(*) FROM webhook_calls WHERE processing_next_at IS NOT NULL") == "1"
                        assert sql("SELECT COUNT(*) FROM webhook_calls WHERE processed_at IS NOT NULL") == str(int(cloud))
                        callback(finish, "early", previous=aliyun)
                        assert sql("SELECT COUNT(*) FROM webhook_calls") == str(1 + int(cloud))
                    finally:
                        release.set()
                    accepted = json.loads(submission.result(20))
                    supplier.gate = None
                assert accepted["submission"]["task"] == task and accepted["callback_receipt"] is None
                assert not queried()

                held = None
                if not cloud:
                    sql("UPDATE moderation_checks SET available_at = 1 "
                        f"WHERE id = '{early['check_id']}'")
                    supplier.query_mode = "quota"
                    supplier.retry_after_headers = ("240",)
                    held = json.loads(artisan(REVIEW, "query", "app", "tenant", early["id"]))
                    assert held["query_attempts"] == 1 and held["query_failure"] == "throttled"
                    assert held["callback_receipt"] is None and queried() == [task]
                    supplier.query_mode = "finished"
                    supplier.retry_after_headers = ()
                early_lookups = [] if cloud else [task]

                assert artisan("drivers:moderation-wake-callback", receipt).strip() == "true"
                assert artisan("drivers:moderation-wake-callback", receipt).strip() == "false"
                consume()
                associated = read(early)
                assert associated["callback_receipt"] == receipt
                if held is None:
                    assert associated["next_query_at"] < accepted["next_query_at"]
                else:
                    assert associated["next_query_at"] == held["next_query_at"]
                    artisan("schedule:run")
                    assert sql("SELECT COUNT(*) FROM jobs WHERE queue = 'moderation-recovery'") == "0"
                assert associated["query_attempts"] == len(early_lookups) and associated["decision"] is None
                assert sql("SELECT COUNT(*) FROM webhook_calls WHERE processed_at IS NOT NULL") == str(1 + int(cloud))
                # 即使队列仍保留旧的延时副本，重复消费也不会再次关联或查单。
                sql(f"UPDATE jobs SET available_at = 1 WHERE queue = '{JOB}'")
                if sql(f"SELECT COUNT(*) FROM jobs WHERE queue = '{JOB}'") != "0":
                    consume()
                assert read(early) == associated and queried() == early_lookups
                if aliyun:
                    callback(finish, "early")
                    consume()
                    assert read(early) == associated and queried() == early_lookups
                if held is not None:
                    sql("UPDATE moderation_checks SET available_at = 1, provider_retry_at = 1 "
                        f"WHERE id = '{early['check_id']}'")
                artisan("schedule:run")
                consume("moderation-recovery")
                checked = read(early)
                assert checked["query_attempts"] == len(early_lookups) + 1
                assert checked["query_failure"] == "incomplete_coverage"
                assert checked["decision"] is None and checked["callback_receipt"] == receipt
                assert queried() == early_lookups + [task]

                callback(finish, "late")
                consume()
                assert read(early) == checked
                callback(finish, "other", other=True)
                consume()
                assert read(early) == checked
                assert queried() == early_lookups + [task]
                if tencent:
                    callback(finish, "policy", policy="different_policy")
                    consume()
                    callback(finish, "live", kind="LIVE_VIDEO")
                    consume()
                    assert read(early) == checked and queried() == early_lookups + [task]
                if aliyun:
                    callback(599, "unsupported-status")
                    consume()
                    assert read(early) == checked and queried() == early_lookups + [task]

                duplicate = json.loads(artisan(REVIEW, "stage", "app", "tenant", "ambiguous-task", version))
                accepted_duplicate = json.loads(artisan(REVIEW, "submit", "app", "tenant", duplicate["id"]))
                assert accepted_duplicate["submission"]["task"] == task
                callback(finish, "ambiguous")
                consume()
                assert read(duplicate)["callback_receipt"] is None and queried() == early_lookups + [task]
            finally:
                HTTP["stop"](app)
        assert supplier.posted == [(version, original), (version, original)]
        assert sql("SELECT COUNT(*) FROM failed_jobs") == str(4 if tencent else 3 if aliyun else 2)
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
    mode = os.environ.get("MODERATION_TEST_CALLBACK_PROVIDER", "sightengine")
    if mode not in {"sightengine", "tencent", "aliyun"}:
        parser.error("未知的回调供应商")
    test_late = os.environ.get("MODERATION_TEST_LATE_CALLBACK") == "1"
    test_purge = os.environ.get("MODERATION_TEST_PURGE_CALLBACK") == "1"
    if ((test_late or test_purge) and mode != "sightengine") or (test_late and test_purge):
        parser.error("迟到或清理回调夹具单独使用 Sightengine")
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-callback-association-") as root, \
            VIDEO["VIDEO"]["OBJECTS"]["TOPOLOGY"]["object_storage"](True) as (env, storage):
        exercise(args.binary.resolve(), driver, url, Path(root), env, storage, mode, test_late=test_late, test_purge=test_purge)
    provider = {"tencent": "腾讯云", "aliyun": "阿里云", "sightengine": "Sightengine"}[mode]
    if test_purge:
        print(f"{driver}: {provider} 实际 HTTP、Artisan、Worker 删除、回调墓碑、到期清理及陈旧任务隔离通过")
    elif test_late:
        print(f"{driver}: {provider} 实际 HTTP、Artisan、Worker 迟到签名回调、诊断去重与终态隔离通过")
    else:
        print(f"{driver}: {provider} 实际 HTTP、Artisan、Worker 提前回调、延期唤醒、范围、查单与覆盖通过")
