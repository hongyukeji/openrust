"""Sightengine 短视频语音以及审核 Worker 一次请求完成的真实协议验收。"""

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
from urllib.parse import parse_qs, urlsplit


ROOT = Path(__file__).resolve().parents[1]
SUBMISSION = runpy.run_path(str(ROOT / "tests/moderation_submission_acceptance.py"))
VIDEO, HTTP, DATABASE, COMMAND = (SUBMISSION[key] for key in ("VIDEO", "HTTP", "DATABASE", "COMMAND"))
PRIVATE_MATCH = "fixture-only-transcribed-private-match"
PRIMARY = "sightengine-speech"


class Supplier(VIDEO["Supplier"]):
    def do_POST(self):
        assert self.path == "/1.0/video/check-sync.json"
        fields = self.form()
        self.authorization(fields)
        source = urlsplit(fields.pop("stream_url")[0])
        assert fields == {"models": ["audio-moderation"], "audio_categories": ["insult,phone_number"]}
        assert f"{source.scheme}://{source.netloc}" == VIDEO["ORIGIN"]
        assert source.path.endswith("/" + VIDEO["KEY"])
        query = parse_qs(source.query, strict_parsing=True)
        version, = query["versionId"]
        assert "X-Amz-Signature" in query and 0 < int(query["X-Amz-Expires"][0]) <= 300
        backend = self.server.storage
        connection = http.client.HTTPConnection(backend.hostname, backend.port, timeout=5)
        try:
            connection.request("GET", source.path + "?" + source.query, headers={"Host": backend.netloc})
            response = connection.getresponse()
            clip = response.read()
            assert response.status == 200
        finally:
            connection.close()
        with self.server.lock:
            self.server.posted.append((version, clip))
            number = len(self.server.posted)
            mode = self.server.mode
        if gate := self.server.gate:
            entered, release = gate
            entered.set()
            assert release.wait(12)

        data = {}
        if mode != "missing-track":
            audio = {"detected_categories": [], "detections": {}}
            data["audio"] = audio
            if mode in ("findings", "bad-time", "duplicate"):
                categories = ["insult", "phone_number"]
                audio["detected_categories"] = categories + (["insult"] if mode == "duplicate" else [])
                audio["detections"] = {
                    "insult": {
                        "severity": "high",
                        "details": [{"severity": "high", "start_ms": 150, "end_ms": 400,
                                     "match": PRIVATE_MATCH}],
                    },
                    "phone_number": {
                        "details": [{"start_ms": 650, "end_ms": 9000 if mode == "bad-time" else 980,
                                     "match": PRIVATE_MATCH, "country": "us"}],
                    },
                }
        body = {
            "status": "success", "request": {"id": f"req_speech_{number}"},
            "media": {"id": f"med_speech{number}", "uri": PRIVATE_MATCH}, "data": data,
        }
        self.respond(200, body)

    def do_GET(self):
        self.server.queried.append(self.path)
        self.respond(405, {"status": "failure"})


def exercise(binary, driver, url, root, storage_env, storage):
    version, silent_version, original, silent = VIDEO["versioned_clips"](root, storage)

    def probe_ms(filename):
        result = subprocess.check_output(
            ["ffprobe", "-v", "error", "-show_entries", "format=duration",
             "-of", "default=noprint_wrappers=1:nokey=1", str(root / filename)], timeout=10,
        )
        return round(float(result) * 1000)

    duration = probe_ms("original.mp4")
    silent_duration = probe_ms("changed.mp4")
    assert 980 <= duration < 60_000 and 0 < silent_duration < 60_000
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Supplier)
    supplier.lock, supplier.mode = threading.Lock(), "clean"
    supplier.expected_secret, supplier.gate = VIDEO["SECRET"], None
    supplier.posted, supplier.queried = [], []
    supplier.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    serving = threading.Thread(target=supplier.serve_forever, daemon=True)
    serving.start()

    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    db = root / "speech.sqlite"
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
    event_log = root / "request-events.txt"
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
        "MODERATION_FIXTURE_RESULT": str(outcome), "MODERATION_FIXTURE_SPEECH": "1",
        "MODERATION_EVENT_OBSERVATIONS": str(event_log),
        "MODERATION_FIXTURE_GROUP": "1", "MODERATION_FIXTURE_DURABLE_VIDEO_PROFILE": PRIMARY,
    }
    env.pop("MODERATION_FIXTURE_VIDEO_DURATION_MS", None)
    if url:
        env["DB_URL"] = url
    else:
        env.pop("DB_URL", None)

    def artisan(*tokens, extra=None, successful=True):
        result = subprocess.run([str(binary), "artisan", *tokens], env=env | (extra or {}),
                                capture_output=True, text=True, timeout=35)
        output = result.stdout + result.stderr
        assert PRIVATE_MATCH not in output and VIDEO["SECRET"] not in output
        assert (result.returncode == 0) == successful, output
        return result.stdout

    def stage(intent, clip=version, ms=duration, profile=PRIMARY, successful=True):
        extra = {"MODERATION_FIXTURE_DURABLE_VIDEO_PROFILE": profile}
        if ms is not None:
            extra["MODERATION_FIXTURE_VIDEO_DURATION_MS"] = str(ms)
        result = artisan(COMMAND, "stage", "app", "tenant", intent, clip, extra=extra, successful=successful)
        return json.loads(result) if successful else None

    def command(action, review):
        return json.loads(artisan(COMMAND, action, "app", "tenant", review["id"]))

    def worker(action, review):
        assert command(f"queue-{action}", review) == {"queued": True}
        payload = sql("SELECT payload FROM jobs")
        assert review["id"] in payload and all(value not in payload for value in (version, VIDEO["SECRET"], PRIVATE_MATCH))
        artisan("queue:work", "database", "--once")
        return json.loads(outcome.read_text())

    def sql(query):
        if driver == "sqlite":
            with sqlite3.connect(db) as connection:
                return "\n".join("\t".join(map(str, row)) for row in connection.execute(query))
        return DATABASE["database_rows"](DATABASE["database_container"](), query).strip()

    def submit_http(review):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=20)
        try:
            connection.request("POST", "/api/drivers/moderation/sightengine-video", headers={
                "Accept": "application/json", "X-Fixture-Submit-Review": review["id"],
            })
            response = connection.getresponse()
            body = response.read()
            assert response.status == 200 and all(value.encode() not in body for value in (VIDEO["SECRET"], PRIVATE_MATCH))
            return json.loads(body)
        finally:
            connection.close()

    try:
        artisan("migrate", "--force")
        artisan("config:cache")
        artisan("route:cache")
        with (root / "http.log").open("w+") as log:
            app = HTTP["serve"](binary, env, log)
            try:
                for intent, ms in (("missing-duration", None), ("at-limit", 60_000)):
                    failed = submit_http(stage(intent, ms=ms))
                    assert failed["state"] == "Failed" and failed["submission"] == {"state": "Failed", "reason": "input"}
                    assert failed["decision"] is None
                assert stage("zero-duration", ms=0, successful=False) is None and not supplier.posted

                clean = stage("short-clean")
                assert stage("short-clean") == clean
                stage("short-clean", ms=duration + 1, successful=False)
                assert sql("SELECT video_duration_ms FROM moderation_reviews "
                           f"WHERE id = '{clean['id']}'") == str(duration)
                inspected = submit_http(clean)
                assert inspected["state"] == "Succeeded" and inspected["decision"] == "allow"
                assert inspected["query_attempts"] == 0 and inspected["next_query_at"] is None
                assert inspected["assessment"]["coverage"] == {"Speech": "Complete", "Visual": "Unavailable", "Ocr": "Unavailable"}
                assert command("find", clean) == command("query", clean) == inspected
                assert submit_http(clean) == inspected and len(supplier.posted) == 1

                supplier.mode = "findings"
                flagged = worker("submit", stage("short-findings"))
                assert flagged["state"] == "Succeeded" and flagged["decision"] == "review"
                assert {item["provider"]: item["span_ms"] for item in flagged["assessment"]["labels"]} == {
                    "insult/high": [150, 400], "phone_number": [650, 980],
                }
                assert flagged["assessment"]["labels"][0]["modality"] == "Speech"

                supplier.mode = "missing-track"
                missing = worker("submit", stage("silent-video", clip=silent_version, ms=silent_duration))
                assert missing["state"] == "Failed" and missing["query_failure"] == "incomplete_coverage"
                assert missing["decision"] is None and missing["assessment"]["coverage"]["Speech"] == "Unavailable"

                for bad in ("duplicate", "bad-time"):
                    supplier.mode = bad
                    invalid = submit_http(stage(f"short-{bad}"))
                    assert invalid["state"] == "Running" and invalid["submission"] == {"state": "Unknown"}
                    assert invalid["decision"] is None and invalid["assessment"] is None

                supplier.mode = "clean"
                wider = worker("submit", stage("short-also-visual", profile="sightengine-speech-visual"))
                assert wider["state"] == "Failed" and wider["query_failure"] == "incomplete_coverage"
                assert wider["decision"] is None and wider["assessment"]["coverage"]["Speech"] == "Complete"

                group = json.loads(artisan(COMMAND, "stage-group", "app", "tenant", "with-speech",
                                           version, f"group-visual,{PRIMARY}",
                                           extra={"MODERATION_FIXTURE_VIDEO_DURATION_MS": str(duration)}))
                visual, speech = group["checks"]
                assert worker("submit", speech)["state"] == "Succeeded"
                assert command("find-group", group)["decision"] is None
                assert worker("submit", visual)["state"] == "Running"
                sql(f"UPDATE moderation_checks SET available_at = 1 WHERE id = '{visual['check_id']}'")
                assert worker("query", visual)["state"] == "Succeeded"
                assert command("find-group", group)["decision"] == "allow"

                late = stage("short-late")
                with ThreadPoolExecutor(max_workers=1) as pool:
                    arrived, release = supplier.gate = (threading.Event(), threading.Event())
                    sending = pool.submit(submit_http, late)
                    try:
                        assert arrived.wait(8)
                        cancelled = command("cancel", late)
                        assert cancelled["state"] == "Cancelled" and cancelled["provider_cancellation"] == "task_unknown"
                    finally:
                        release.set()
                    assert sending.result(timeout=12) == cancelled
                    supplier.gate = None
                assert cancelled["decision"] is None and cancelled["assessment"] is None
                assert supplier.posted == [(version, original)] * 2 + [(silent_version, silent)] + [(version, original)] * 5
                assert not supplier.queried and event_log.read_text().splitlines() == ["POST"] * 8
                assert sql("SELECT COUNT(*) FROM moderation_remote_leases") == "3"
                assert PRIVATE_MATCH not in sql("SELECT assessment FROM moderation_checks WHERE assessment IS NOT NULL")
                assert sql("SELECT COUNT(*) FROM jobs") == sql("SELECT COUNT(*) FROM failed_jobs") == "0"
            finally:
                HTTP["stop"](app)
            log.seek(0)
            messages = log.read()
            assert PRIVATE_MATCH not in messages and VIDEO["SECRET"] not in messages
        artisan("migrate:rollback", "--force",
                "--path=vendor/openrust/content-moderation/database/migrations/"
                "2026_11_26_000014_add_moderation_video_duration.rs", successful=False)
    finally:
        supplier.shutdown()
        supplier.server_close()
        serving.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    driver, url = DATABASE["database_settings"]()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-speech-") as root, \
            VIDEO["VIDEO"]["OBJECTS"]["TOPOLOGY"]["object_storage"](True) as (env, storage):
        exercise(args.binary.resolve(), driver, url, Path(root), env, storage)
    print(f"{driver}: Sightengine 短视频语音、限长、私有版本、即时终态及复合门禁通过 HTTP、Artisan 和 Worker")
