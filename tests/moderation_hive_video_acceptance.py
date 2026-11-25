"""Hive V2 点播送审、私有版本与可靠回执的真实消费者。"""

import argparse
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
from urllib.parse import parse_qs, urlsplit


ROOT = Path(__file__).resolve().parents[1]
VIDEO = runpy.run_path(str(ROOT / "tests/moderation_sightengine_video_acceptance.py"))
DATABASE = runpy.run_path(str(ROOT / "tests/sms_worker.py"))
HTTP = VIDEO["HTTP"]
PROJECT = 48175
TASK = "dd52c0d0-9ec7-11ea-98a3-4b92424b8366"
TOKEN = "fixture-hive-video-project-token"
OCR_TOKEN = "fixture-hive-video-ocr-project-token"
SPEECH_TOKEN = "fixture-hive-video-speech-project-token"
PRIVATE = "hidden-hive-frame-reference"
CALLBACK = "https://moderation-callback.fixture.invalid/api/hive"
COMMAND = "drivers:moderation-hive-video"


class Supplier(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        assert self.path == "/api/v2/task/async"
        assert self.headers["Authorization"] == f"Token {self.server.token}"
        assert self.headers["Content-Type"] == "application/x-www-form-urlencoded"
        fields = parse_qs(self.rfile.read(int(self.headers["Content-Length"])).decode(), strict_parsing=True)
        assert fields.pop("callback_url") == [CALLBACK] and set(fields) == {"url"}
        source = urlsplit(fields["url"][0])
        assert f"{source.scheme}://{source.netloc}" == VIDEO["ORIGIN"]
        assert source.path.endswith("/" + VIDEO["KEY"])
        signature = parse_qs(source.query, strict_parsing=True)
        assert "X-Amz-Signature" in signature and 0 < int(signature["X-Amz-Expires"][0]) <= 300
        version, = signature["versionId"]
        backend = self.server.storage
        connection = http.client.HTTPConnection(backend.hostname, backend.port, timeout=5)
        try:
            connection.request("GET", source.path + "?" + source.query, headers={"Host": backend.netloc})
            result = connection.getresponse()
            content = result.read()
            assert result.status == 200
        finally:
            connection.close()
        with self.server.lock:
            self.server.calls.append((version, content))
            mode = self.server.mode
        response = {"id": TASK, "project_id": self.server.project, "callback": CALLBACK}
        status = 200
        if mode == "wrong-project":
            response["project_id"] += 1
        elif mode == "wrong-callback":
            response["callback"] += "/other"
        elif mode == "missing-id":
            response.pop("id")
        elif mode == "nil-id":
            response["id"] = "00000000-0000-0000-0000-000000000000"
        elif mode == "rate":
            response = {"code": 429, "error": TOKEN}
        elif mode == "auth":
            status = 403
        elif mode == "server":
            status = 503
        elif mode == "timeout":
            time.sleep(1.3)
        body = b"{" if mode == "malformed" else json.dumps(response).encode()
        self.respond(status, body, allow_disconnect=(mode == "timeout"))

    def respond(self, status, body, *, allow_disconnect=False):
        try:
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            assert allow_disconnect

    def do_GET(self):
        target = urlsplit(self.path)
        assert target.path == "/api/v1/task" and self.headers["Authorization"] == f"Token {self.server.token}"
        parameters = parse_qs(target.query, strict_parsing=True)
        offset, = parameters.pop("frame_offset")
        offset = int(offset)
        assert parameters == {"task_id": [TASK], "frame_limit": ["64"]}
        with self.server.lock:
            self.server.summary_calls.append(offset)
            mode = self.server.query_mode
        if self.server.video_model != "CATEGORIZATION":
            self.respond_modality(mode)
            return
        status = 200
        task = {"id": TASK, "project_id": PROJECT}
        frames = []
        if mode in ("paged", "capped") and (offset == 0 or mode == "capped"):
            frames = [{"time": offset + index, "classes": [{"class": "general_nsfw", "score": .1}]}
                      for index in range(64)]
        elif mode == "too-many":
            frames = [{"time": index, "classes": []} for index in range(65)]
        elif mode != "empty":
            frames = [{"time": 64.5 if offset == 64 else 2.25,
                       "media_link": "https://restricted.fixture.invalid/" + PRIVATE,
                       "classes": [{"class": "gun_in_hand", "score": .97}]}]
        input_id = TASK if mode not in ("cached", "wrong-input") else "b9388470-a078-11ea-a390-db89c3c0393a"
        task["status"] = [{
            "status": {"code": "0"},
            "response": {"input": {"id": input_id, "project_id": PROJECT, "model_type": "CATEGORIZATION"},
                         "output": frames},
        }]
        if mode == "cached":
            task["from_cache"] = True
        elif mode == "pending":
            task["status"] = []
        elif mode == "accepted":
            status, task = 202, {}
        elif mode == "wrong-task":
            task["id"] = "b9388470-a078-11ea-a390-db89c3c0393a"
        elif mode == "wrong-project":
            task["project_id"] += 1
        elif mode == "wrong-model":
            task["status"][0]["response"]["input"]["model_type"] = "OCR_MODERATION"
        elif mode == "bad-time":
            frames[0]["time"] = -1
        elif mode == "bad-score":
            frames[0]["classes"][0]["score"] = 1.5
        elif mode == "rate":
            status = 429
        elif mode == "duplicate-class":
            frames[0]["classes"].append(frames[0]["classes"][0])
        self.respond(status, json.dumps(task).encode())

    def respond_modality(self, mode):
        model = self.server.video_model
        if model == "OCR_MODERATION":
            data = [{"time": 1.44, "frame_results": [{
                "block_text": PRIVATE, "pii_entities": [{"match": PRIVATE}],
                "classes": [{"class": "hate", "score": 2}, {"class": "violence", "score": -1}],
            }]}]
            if mode == "empty":
                data[0]["frame_results"] = []
            elif mode == "bad-time":
                data[0]["time"] = -1
            elif mode == "bad-score":
                data[0]["frame_results"][0]["classes"][0]["score"] = 9
        else:
            data = [{"transcript": PRIVATE, "words": [{"time": 2.25, "alternatives": [{"text": PRIVATE}]}],
                     "classifications": [{"start_char_index": 2, "end_char_index": 18,
                                          "classes": [{"class": "violence", "score": 3},
                                                      {"class": "hate", "score": -1}]}]}]
            if mode == "empty":
                data[0]["classifications"] = []
            elif mode == "bad-score":
                data[0]["classifications"][0]["classes"][0]["score"] = 9
        task = {"id": TASK, "project_id": self.server.project, "status": [{
            "status": {"code": "0"},
            "response": {"input": {"id": TASK, "project_id": self.server.project,
                                   "model_type": "CATEGORIZATION" if mode == "wrong-model" else model},
                         "output": data},
        }]}
        self.respond(200, json.dumps(task).encode())


def exercise(binary, driver, url, root, storage_env, storage):
    first, last, original, changed = VIDEO["versioned_clips"](root, storage)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Supplier)
    server.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    server.calls, server.mode, server.lock = [], "accepted", threading.Lock()
    server.summary_calls, server.query_mode = [], "paged"
    server.project, server.token, server.video_model = PROJECT, TOKEN, "CATEGORIZATION"
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    sqlite = root / "queue.sqlite"
    if driver == "sqlite":
        sqlite.touch()
        os.environ["OPENRUST_SMS_SQLITE_PATH"] = str(sqlite)
    mount = root / "credentials"
    mount.mkdir(mode=0o700)
    for name, token in [("hive-video", TOKEN), ("hive-video-ocr", OCR_TOKEN), ("hive-video-speech", SPEECH_TOKEN)]:
        descriptor = os.open(mount / f"{name}.r1.json", os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o400)
        with os.fdopen(descriptor, "w") as file:
            json.dump({"video_token": token}, file)
    env = os.environ | storage_env | {
        "APP_BASE_PATH": str(root), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "DB_CONNECTION": driver, "QUEUE_CONNECTION": "database",
        "DB_DATABASE": str(sqlite) if driver == "sqlite" else "openrust_acceptance",
        "MODERATION_CREDENTIAL_DIRECTORY": str(mount),
        "MODERATION_FIXTURE_HIVE_VIDEO": f"http://127.0.0.1:{server.server_port}",
        "MODERATION_FIXTURE_HIVE_CALLBACK": CALLBACK,
        "MODERATION_FIXTURE_DURABLE_VIDEO_PROFILE": "hive-videos",
        "AWS_TEMPORARY_URL": VIDEO["ORIGIN"], "MODERATION_FIXTURE_IMAGE_ORIGIN": VIDEO["ORIGIN"],
    }
    if url:
        env["DB_URL"] = url
    else:
        env.pop("DB_URL", None)

    def artisan(current, *tokens, success=True):
        result = subprocess.run([str(binary), "artisan", *tokens], env=current,
                                capture_output=True, text=True, timeout=35)
        assert all(value not in result.stdout + result.stderr for value in (TOKEN, OCR_TOKEN, SPEECH_TOKEN, PRIVATE))
        assert (result.returncode == 0) == success, result.stdout + result.stderr
        return result.stdout

    def post(status=200, *, version=first, disk="s3", task=None):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=15)
        try:
            headers = {"Accept": "application/json", "X-Fixture-Version": version, "X-Fixture-Disk": disk}
            if task:
                headers["X-Fixture-Task"] = task
            connection.request("POST", "/api/drivers/moderation/hive-video", headers=headers)
            response = connection.getresponse()
            data = response.read()
            assert response.status == status, (status, response.status, data[:250])
            assert all(value.encode() not in data for value in (TOKEN, OCR_TOKEN, SPEECH_TOKEN, PRIVATE))
            return json.loads(data)
        finally:
            connection.close()

    ready = {"task": TASK, "cancel": False}
    try:
        assert json.loads(artisan(env, COMMAND, "submit", first)) == ready
        for callback in ("http://unsafe.fixture.invalid/hive", CALLBACK + "?secret=x"):
            misconfigured = env | {"MODERATION_FIXTURE_HIVE_CALLBACK": callback}
            artisan(misconfigured, COMMAND, "submit", first, success=False)
        artisan(env, "migrate", "--force")
        artisan(env, "config:cache")
        artisan(env, "route:cache")
        assert TOKEN not in (root / "config.json").read_text()
        cached = env | {"MODERATION_FIXTURE_HIVE_VIDEO": "http://127.0.0.1:1"}
        with (root / "http.log").open("w+") as log:
            application = HTTP["serve"](binary, cached, log)
            try:
                assert post(version=last) == ready
                assert post(400, version="null")["error"].startswith("InvalidInput")
                assert post(400, disk="local")["error"].startswith("InvalidInput")
                assert post(502, task=TASK) == {"error": "Unsupported(QueryVideo)"}
                for mode, status, error in (
                    ("wrong-project", 502, "SubmissionUnknown"), ("wrong-callback", 502, "SubmissionUnknown"),
                    ("missing-id", 502, "SubmissionUnknown"), ("nil-id", 502, "SubmissionUnknown"),
                    ("auth", 401, "Unauthorized"), ("rate", 429, "Throttled"),
                    ("server", 502, "SubmissionUnknown"), ("malformed", 502, "SubmissionUnknown"),
                    ("timeout", 502, "SubmissionUnknown"),
                ):
                    server.mode = mode
                    assert post(status) == {"error": error}
            finally:
                HTTP["stop"](application)
            log.seek(0)
            trace = log.read()
            assert TOKEN not in trace and "X-Amz-Signature" not in trace
        server.mode = "accepted"
        review = json.loads(artisan(cached, "drivers:moderation-review", "stage", "app", "tenant", "hive-video-intent", first))
        assert json.loads(artisan(cached, "drivers:moderation-review", "queue-submit", "app", "tenant", review["id"])) == {"queued": True}
        output = root / "worker.json"
        artisan(cached | {"MODERATION_FIXTURE_RESULT": str(output)}, "queue:work", "database", "--once")
        submitted = json.loads(output.read_text())
        assert submitted["submission"] == {"state": "Submitted", "task": TASK}
        assert submitted["profile"]["name"] == "hive-videos" and submitted["object_version"] == first
        assert submitted["state"] == "Running" and submitted["decision"] is None
        assert server.calls == [(first, original), (last, changed)] + [(first, original)] * 10

        summary = env | {"MODERATION_FIXTURE_HIVE_VIDEO_QUERY": "v1-task-summary"}
        artisan(summary, "config:clear")
        artisan(summary, "config:cache")
        assert not server.summary_calls
        with (root / "summary-http.log").open("w+") as log:
            application = HTTP["serve"](binary, summary, log)
            try:
                assert post(400, task="invalid-summary-id")["error"].startswith("InvalidInput")
                server.query_mode = "pending"
                assert post(task=TASK) == {"status": "Running"}
                server.query_mode = "accepted"
                assert post(task=TASK) == {"status": "Running"}
                server.query_mode = "paged"
                checked = post(task=TASK)
                assert checked["status"] == "Succeeded"
                assessment = checked["assessment"]
                assert assessment["decision"] == "Review"
                assert assessment["coverage"] == {"Ocr": "Unavailable", "Speech": "Unavailable", "Visual": "Partial"}
                label, = assessment["labels"]
                assert label["span_ms"] == [64500, 64500] and label["confidence"] == {"value": .97, "maximum": 1.0}
                assert server.summary_calls == [0, 0, 0, 64]
                server.query_mode = "capped"
                assert post(task=TASK)["assessment"]["decision"] is None
                assert server.summary_calls[-4:] == [0, 64, 128, 192]
                server.query_mode = "empty"
                assert post(task=TASK)["assessment"]["coverage"]["Visual"] == "Unavailable"
                server.query_mode = "cached"
                assert post(task=TASK)["assessment"]["labels"][0]["span_ms"] == [2250, 2250]
                for mode in ("wrong-task", "wrong-project", "wrong-model", "wrong-input",
                             "bad-time", "bad-score", "too-many", "duplicate-class"):
                    server.query_mode = mode
                    assert post(502, task=TASK) == {"error": "Protocol"}
                server.query_mode = "rate"
                assert post(429, task=TASK) == {"error": "Throttled"}
            finally:
                HTTP["stop"](application)
            log.seek(0)
            trace = log.read()
            assert TOKEN not in trace and PRIVATE not in trace
        server.query_mode = "paged"
        assert json.loads(artisan(summary, COMMAND, "query", TASK)) == checked
        # 更新了同名配置时，原未配置查单的审核仍绑定自己的修订。
        stale = json.loads(artisan(summary, "drivers:moderation-review", "find", "app", "tenant", review["id"]))
        assert stale["profile"] == submitted["profile"]
        next_review = json.loads(artisan(summary, "drivers:moderation-review", "stage", "app", "tenant",
                                         "hive-summary-intent", first))
        artisan(summary, "drivers:moderation-review", "submit", "app", "tenant", next_review["id"])
        change = f"UPDATE moderation_checks SET available_at = 1 WHERE id = '{next_review['check_id']}'"
        if driver == "sqlite":
            with sqlite3.connect(sqlite) as connection:
                connection.execute(change)
        else:
            DATABASE["database_rows"](DATABASE["database_container"](), change)
        artisan(summary, "drivers:moderation-review", "queue-query", "app", "tenant", next_review["id"])
        output.unlink()
        artisan(summary | {"MODERATION_FIXTURE_RESULT": str(output)}, "queue:work", "database", "--once")
        confirmed = json.loads(output.read_text())
        assert confirmed["state"] == "Failed" and confirmed["query_failure"] == "incomplete_coverage"
        assert confirmed["query_attempts"] == 1 and confirmed["decision"] is None
        assert confirmed["assessment"]["labels"] == checked["assessment"]["labels"]
        assert len(server.summary_calls) == 23
        for name, project, secret, model, coverage, provider, score, span in (
            ("ocr", 48176, OCR_TOKEN, "OCR_MODERATION", "Ocr", "hate", 2.0, [1440, 1440]),
            ("speech", 48177, SPEECH_TOKEN, "TRANSCRIPTION", "Speech", "violence", 3.0, None),
        ):
            server.project, server.token, server.video_model = project, secret, model
            current = summary | {"MODERATION_FIXTURE_HIVE_VIDEO_MODEL": name}
            artisan(current, "config:clear")
            artisan(current, "config:cache")
            server.query_mode = "result"
            assert json.loads(artisan(current, COMMAND, "submit", first)) == ready
            with (root / f"{name}-http.log").open("w+") as log:
                application = HTTP["serve"](binary, current, log)
                try:
                    direct = post(task=TASK)
                    assert direct["status"] == "Succeeded"
                    assessment = direct["assessment"]
                    expected_coverage = {key: "Unavailable" for key in ("Ocr", "Speech", "Visual")}
                    expected_coverage[coverage] = "Partial"
                    assert assessment["coverage"] == expected_coverage
                    finding = next(label for label in assessment["labels"] if label["provider"] == provider)
                    assert finding["confidence"] == {"value": score, "maximum": 3.0}
                    assert finding.get("span_ms") == span and assessment["decision"] == "Review"
                    if name == "ocr":
                        assert any(label["provider"] == "pii_entities" for label in assessment["labels"])
                    for mode in ("bad-score", "wrong-model"):
                        server.query_mode = mode
                        assert post(502, task=TASK) == {"error": "Protocol"}
                    server.query_mode = "empty"
                    empty = post(task=TASK)["assessment"]
                    assert empty["decision"] is None and not empty["labels"]
                    if name == "ocr":
                        server.query_mode = "bad-time"
                        assert post(502, task=TASK) == {"error": "Protocol"}
                finally:
                    HTTP["stop"](application)
                log.seek(0)
                trace = log.read()
                assert secret not in trace and PRIVATE not in trace
            server.query_mode = "result"
            intent = json.loads(artisan(current, "drivers:moderation-review", "stage", "app", "tenant",
                                         f"hive-{name}-intent", first))
            artisan(current, "drivers:moderation-review", "submit", "app", "tenant", intent["id"])
            due = f"UPDATE moderation_checks SET available_at = 1 WHERE id = '{intent['check_id']}'"
            if driver == "sqlite":
                with sqlite3.connect(sqlite) as connection:
                    connection.execute(due)
            else:
                DATABASE["database_rows"](DATABASE["database_container"](), due)
            artisan(current, "drivers:moderation-review", "queue-query", "app", "tenant", intent["id"])
            output.unlink()
            artisan(current | {"MODERATION_FIXTURE_RESULT": str(output)}, "queue:work", "database", "--once")
            result = json.loads(output.read_text())
            assert result["profile"]["required_modalities"] == [name]
            assert result["query_failure"] == "incomplete_coverage" and result["decision"] is None
            assert result["assessment"]["labels"] == direct["assessment"]["labels"]
        assert len(server.summary_calls) == 34
    finally:
        server.shutdown()
        server.server_close()
        worker.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    driver, url = DATABASE["database_settings"]()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-hive-video-") as root, \
            VIDEO["VIDEO"]["OBJECTS"]["TOPOLOGY"]["object_storage"](True) as (env, storage):
        exercise(args.binary.resolve(), driver, url, Path(root), env, storage)
    print(f"{driver}: Hive 点播视觉、OCR 和语音通过真实 HTTP、Artisan 与独立 Worker")
