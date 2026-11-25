"""Hive V2 点播送审、私有版本与可靠回执的真实消费者。"""

import argparse
import http.client
import http.server
import json
import os
from pathlib import Path
import runpy
import socket
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
CALLBACK = "https://moderation-callback.fixture.invalid/api/hive"
COMMAND = "drivers:moderation-hive-video"


class Supplier(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        assert self.path == "/api/v2/task/async"
        assert self.headers["Authorization"] == f"Token {TOKEN}"
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
        response = {"id": TASK, "project_id": PROJECT, "callback": CALLBACK}
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
        try:
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            assert mode == "timeout"


def exercise(binary, driver, url, root, storage_env, storage):
    first, last, original, changed = VIDEO["versioned_clips"](root, storage)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Supplier)
    server.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    server.calls, server.mode, server.lock = [], "accepted", threading.Lock()
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
    descriptor = os.open(mount / "hive-video.r1.json", os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o400)
    with os.fdopen(descriptor, "w") as file:
        json.dump({"video_token": TOKEN}, file)
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
        assert TOKEN not in result.stdout + result.stderr
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
            assert TOKEN.encode() not in data
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
    print(f"{driver}: Hive V2 HTTP／Artisan／Worker、私有 MP4、项目匹配和未知提交通过")
