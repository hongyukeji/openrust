"""Sightengine 账户回调经 Webhook Client 实际验签、去重和队列接收。"""

import argparse
import hashlib
import hmac
import http.client
import json
import os
from pathlib import Path
import runpy
import socket
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
DATABASE = runpy.run_path(str(ROOT / "tests/sms_worker.py"))
HTTP = runpy.run_path(str(ROOT / "tests/webhooks_acceptance.py"))
JOB = "moderation-callback-fixture"
OLD = "casec_fixture_previous_key_V1"
CURRENT = "casec_fixture_active_key_V2"
OTHER = "casec_fixture_other_account"
SECRETS = (OLD, CURRENT, OTHER)


def callback_header(body, key, timestamp=None):
    timestamp = str(int(time.time()) if timestamp is None else timestamp)
    digest = hmac.new(key.encode(), timestamp.encode() + b"." + body, hashlib.sha256).hexdigest()
    return f"t={timestamp},v1={digest}"


def exercise(binary, driver, url, root):
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    sqlite = root / "callbacks.sqlite"
    if driver == "sqlite":
        sqlite.touch()
        os.environ["OPENRUST_SMS_SQLITE_PATH"] = str(sqlite)
    mount = root / "credentials"
    mount.mkdir(mode=0o700)
    for file, credential in [("sight-callback.r1.json", OLD), ("sight-callback.r2.json", CURRENT),
                             ("other-callback.r1.json", OTHER)]:
        descriptor = os.open(mount / file, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o400)
        with os.fdopen(descriptor, "w") as file:
            json.dump({"signing_secret": credential}, file)
    env = os.environ | {
        "APP_BASE_PATH": str(root), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "DB_CONNECTION": driver, "QUEUE_CONNECTION": "database",
        "DB_DATABASE": str(sqlite) if driver == "sqlite" else "openrust_acceptance",
        "MODERATION_CREDENTIAL_DIRECTORY": str(mount),
        "MODERATION_FIXTURE_SIGHTENGINE_VIDEO": "http://127.0.0.1:1",
    }
    if url:
        env["DB_URL"] = url
    else:
        env.pop("DB_URL", None)

    def artisan(current, *tokens):
        result = subprocess.run([str(binary), "artisan", *tokens], env=current,
                                capture_output=True, text=True, timeout=35)
        assert all(secret not in result.stdout + result.stderr for secret in SECRETS)
        assert result.returncode == 0, result.stdout + result.stderr
        return result.stdout

    def sql(query):
        return DATABASE["database_rows"](DATABASE["database_container"](), query).strip()

    def post(body, *signatures, status=200, other=False):
        path = "/api/drivers/moderation/callback" + ("-other" if other else "")
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            connection.putrequest("POST", path)
            connection.putheader("Content-Type", "application/json")
            connection.putheader("Accept", "application/json")
            connection.putheader("Content-Length", str(len(body)))
            for signature in signatures:
                connection.putheader("Sightengine-Signature", signature)
            connection.endheaders(body)
            response = connection.getresponse()
            output = response.read()
            assert response.status == status, (status, response.status, output[:300])
            assert all(secret.encode() not in output for secret in SECRETS)
        finally:
            connection.close()

    raw = b'{\n "media": {"id": "med_first"}, "data": {"status": "finished"}\n}'
    another = raw.replace(b"med_first", b"med_second")
    retired_message = raw.replace(b"med_first", b"med_rotation")
    artisan(env, "migrate", "--force")
    artisan(env, "config:cache")
    artisan(env, "route:cache")
    configuration = (root / "config.json").read_text()
    assert "sight-callback" in configuration and all(secret not in configuration for secret in SECRETS)
    assert "moderation-sightengine" in artisan(env, "config:show", "webhook-client")
    assert "moderation-sightengine" not in artisan(env, "config:show", "moderation")
    runtime = env | {"MODERATION_CALLBACK_FIXTURE_REVISION": "cannot-be-loaded-from-cache"}
    with (root / "http.log").open("w+") as log:
        app = HTTP["serve"](binary, runtime, log)
        try:
            old_signature = callback_header(raw, OLD)
            post(raw, old_signature)
            post(raw, callback_header(raw, CURRENT))
            assert sql("SELECT COUNT(*) FROM webhook_calls") == "1"
            current = callback_header(another, CURRENT)
            stamp, signature = current.split(",")
            post(another, f"{stamp},v0=ignored,v1={'0'*64},{signature}")
            post(raw, callback_header(raw, OTHER), other=True)
            negatives = [
                (raw, ()),
                (raw, (callback_header(raw, OTHER),)),
                (raw, (callback_header(raw, CURRENT, int(time.time()) - 601),)),
                (raw, (callback_header(raw, CURRENT, int(time.time()) + 601),)),
                (raw, (old_signature, old_signature)),
                (raw, (old_signature + ",t=12345678",)),
                (raw, (old_signature.replace(",v1=", ",v2="),)),
                (raw, (old_signature + ",v1=malformed",)),
                (raw.replace(b"\n ", b""), (old_signature,)),
            ]
            for body, signatures in negatives:
                post(body, *signatures, status=500)
            post(raw, old_signature, status=500, other=True)
            post(b"x" * 2049, status=400)
            assert sql("SELECT COUNT(*) FROM webhook_calls") == "3"
            assert sql("SELECT COUNT(*) FROM jobs") == "3"
        finally:
            HTTP["stop"](app)
        log.seek(0)
        trace = log.read()
        assert all(secret not in trace for secret in SECRETS)
    revoked = env | {"MODERATION_CALLBACK_FIXTURE_RETIRED": "1"}
    artisan(revoked, "config:clear")
    (mount / "sight-callback.r1.json").unlink()
    artisan(revoked, "config:cache")
    with (root / "rotated.log").open("w+") as log:
        app = HTTP["serve"](binary, revoked, log)
        try:
            post(retired_message, callback_header(retired_message, OLD), status=500)
            post(retired_message, callback_header(retired_message, CURRENT))
        finally:
            HTTP["stop"](app)
    payload = sql("SELECT payload FROM jobs ORDER BY id")
    assert all(value not in payload for value in (*SECRETS, "med_first", "med_second", "Sightengine-Signature"))
    output = root / "worker.json"
    observed = []
    for _ in range(4):
        artisan(revoked | {"MODERATION_CALLBACK_FIXTURE_RESULT": str(output)},
                "queue:work", "database", f"--queue={JOB}", "--once")
        observed.append(json.loads(output.read_text()))
        output.unlink()
    assert [row["media"] for row in observed] == ["med_first", "med_second", "med_first", "med_rotation"]
    assert len({row["id"] for row in observed}) == 4
    assert sql("SELECT COUNT(*) FROM jobs") == sql("SELECT COUNT(*) FROM failed_jobs") == "0"
    assert sql("SELECT COUNT(*) FROM webhook_calls") == "4"
    stored_headers = sql("SELECT headers FROM webhook_calls")
    assert all(secret not in stored_headers for secret in SECRETS)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    driver, url = DATABASE["database_settings"]()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-callback-") as directory:
        exercise(args.binary.resolve(), driver, url, Path(directory))
    print(f"{driver}: Sightengine HTTP 验签、缓存、轮换和 Webhook Worker 接收通过")
