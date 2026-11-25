"""Sightengine 点播提交、查询和停止，检查 GET 凭据从观察事件中移除。"""

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
from urllib.parse import parse_qs, urlsplit
import xml.etree.ElementTree as XML

ROOT = Path(__file__).resolve().parents[1]
VIDEO = runpy.run_path(str(ROOT / "tests/moderation_aliyun_video_acceptance.py"))
HTTP = VIDEO["ALIYUN"]["HTTP"]
ORIGIN, KEY = VIDEO["ORIGIN"], VIDEO["KEY"]
SECRET = "sightengine-video-private+/"
PRIVATE = "sightengine-hidden-ocr-original"
TASK = "med_detailed"
COMMAND = "drivers:moderation-sightengine-video"


class Supplier(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def respond(self, status, payload):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def form(self):
        assert self.headers["Content-Type"] == "application/x-www-form-urlencoded"
        return parse_qs(self.rfile.read(int(self.headers["Content-Length"])).decode(), strict_parsing=True)

    def authorization(self, parameters):
        assert parameters.pop("api_user") == ["local-sightengine-account"]
        assert parameters.pop("api_secret") == [SECRET]

    def do_POST(self):
        assert self.path == "/1.0/video/check.json"
        params = self.form()
        self.authorization(params)
        clip, = params.pop("stream_url")
        assert params == {"models": ["violence,text-content-2.0"], "text_categories": ["email"],
                          "max_duration": ["900"]}
        source = urlsplit(clip)
        query = parse_qs(source.query, strict_parsing=True)
        version, = query["versionId"]
        assert f"{source.scheme}://{source.netloc}" == ORIGIN and source.path.endswith("/" + KEY)
        assert "X-Amz-Signature" in query and 0 < int(query["X-Amz-Expires"][0]) <= 300
        backend = self.server.storage
        connection = http.client.HTTPConnection(backend.hostname, backend.port, timeout=5)
        try:
            connection.request("GET", source.path + "?" + source.query, headers={"Host": backend.netloc})
            response = connection.getresponse()
            clip_data = response.read()
            assert response.status == 200
        finally:
            connection.close()
        with self.server.lock:
            self.server.posted.append((version, clip_data))
            mode = self.server.mode
        if mode == "quota":
            self.respond(429, {"status": "failure", "error": {"message": PRIVATE}})
        elif mode == "lost-id":
            self.respond(200, {"status": "success", "media": {"id": "invalid-media-id", "uri": PRIVATE}})
        else:
            self.respond(200, {"status": "success", "media": {"id": TASK, "uri": PRIVATE}})

    def do_GET(self):
        base = urlsplit(self.path)
        assert base.path == "/1.0/video/byid.json"
        params = parse_qs(base.query, strict_parsing=True)
        self.authorization(params)
        assert set(params) == {"id"}
        task, = params["id"]
        with self.server.lock:
            self.server.queried.append(task)
            stopped = task in self.server.stops
        status = {"med_running": "ongoing", "med_failed": "failure", "med_other": "unknown"}.get(task, "finished")
        if stopped:
            status = "stopped"
        payload = {"status": "success", "request": {"id": "se-query-reference"},
                   "output": {"media": {"id": task, "uri": PRIVATE},
                              "data": {"status": status, "progress": .6, "frames": []}}}
        data = payload["output"]["data"]
        if task == TASK:
            data["frames"] = [{
                "info": {"position": 1.125}, "violence": {"prob": .96},
                "text": {"detected_categories": ["email"],
                         "detections": {"email": {"details": [{"match": PRIVATE}]}}},
            }]
        elif task == "med_wrongid":
            payload["output"]["media"]["id"] = "med_foreign"
        elif task == "med_invalidprogress":
            data["progress"] = 1.2
        elif task == "med_invalidtime":
            data["frames"] = [{"info": {"position": -1}, "violence": {"prob": .9}}]
        if task == "med_forbidden":
            self.respond(403, {"status": "failure", "error": {"message": PRIVATE}})
        else:
            self.respond(200, payload)

    def do_DELETE(self):
        assert self.path == "/1.0/video/byid.json"
        params = self.form()
        self.authorization(params)
        assert set(params) == {"id"}
        task, = params["id"]
        with self.server.lock:
            self.server.stops.append(task)
        self.respond(200, {"status": "success", "request": {"id": "se-stop-reference"}})


def exercise(binary, work, storage_env, storage):
    storage("PUT", query="versioning", content=(
        b'<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">'
        b'<Status>Enabled</Status></VersioningConfiguration>'))
    original = VIDEO["generate"](work, "original.mp4", "purple", True)
    changed = VIDEO["generate"](work, "changed.mp4", "white", False)
    storage("PUT", KEY, content=original)
    storage("PUT", KEY, content=changed)
    result = XML.fromstring(storage("GET", query="versions&prefix=versions%2Fclip.mp4"))
    versions = {node.findtext("{*}IsLatest"): node.findtext("{*}VersionId")
                for node in result.findall("{*}Version") if node.findtext("{*}Key") == KEY}
    first, last = versions["false"], versions["true"]
    assert first != last and "null" not in {first, last}
    storage("GET", KEY, status=403, signed=False)

    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Supplier)
    supplier.lock, supplier.mode = threading.Lock(), "accepted"
    supplier.posted, supplier.queried, supplier.stops = [], [], []
    supplier.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    thread = threading.Thread(target=supplier.serve_forever, daemon=True)
    thread.start()
    database = work / "queue.sqlite"
    database.touch()
    observations = work / "redacted-events.log"
    env = os.environ | storage_env | {
        "APP_BASE_PATH": str(work), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(work / "config.json"), "APP_ROUTES_CACHE": str(work / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DB_CONNECTION": "sqlite", "DB_DATABASE": str(database), "QUEUE_CONNECTION": "database",
        "DRIVER_HTTP": "1", "MODERATION_FIXTURE_SIGHTENGINE_VIDEO": f"http://127.0.0.1:{supplier.server_port}",
        "AWS_TEMPORARY_URL": ORIGIN, "MODERATION_FIXTURE_IMAGE_ORIGIN": ORIGIN,
        "FIXTURE_SIGHTENGINE_USER": "local-sightengine-account", "FIXTURE_SIGHTENGINE_SECRET": SECRET,
        "MODERATION_EVENT_OBSERVATIONS": str(observations),
    }
    env.pop("DB_URL", None)

    def artisan(current, *tokens, successful=True):
        process = subprocess.run([str(binary), "artisan", *tokens], env=current, capture_output=True,
                                 text=True, timeout=25)
        assert (process.returncode == 0) == successful, process.stdout + process.stderr
        assert PRIVATE not in process.stdout + process.stderr and SECRET not in process.stdout + process.stderr
        return process.stdout

    def post(expected=200, *, version=None, task=None, cancel=False, disk="s3"):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            headers = {"Accept": "application/json"}
            if version:
                headers.update({"X-Fixture-Version": version, "X-Fixture-Disk": disk})
            if task:
                headers["X-Fixture-Task"] = task
            if cancel:
                headers["X-Fixture-Cancel"] = "1"
            connection.request("POST", "/api/drivers/moderation/sightengine-video", headers=headers)
            response = connection.getresponse()
            data = response.read()
            assert response.status == expected, (response.status, expected, data[:250])
            assert PRIVATE.encode() not in data
            return json.loads(data)
        finally:
            connection.close()

    accepted = {"task": TASK, "cancel": True}
    try:
        assert json.loads(artisan(env, COMMAND, "submit", first)) == accepted
        artisan(env | {"AWS_TEMPORARY_URL": "https://disallowed.fixture.invalid"},
                COMMAND, "submit", first, successful=False)
        artisan(env, "migrate", "--force")
        artisan(env, "config:cache")
        artisan(env, "route:cache")
        cache = (work / "config.json").read_text()
        assert SECRET not in cache and "FIXTURE_SIGHTENGINE_SECRET" in cache
        cached = env | {"MODERATION_FIXTURE_SIGHTENGINE_VIDEO": "http://127.0.0.1:1"}
        with (work / "http.log").open("w+") as log:
            application = HTTP["serve"](binary, cached, log)
            try:
                assert post(version=last) == accepted
                assert post(400, version=first, disk="local")["error"].startswith("InvalidInput")
                assert post(400, version="null")["error"].startswith("InvalidInput")
                supplier.mode = "quota"
                assert post(429, version=first) == {"error": "Throttled"}
                supplier.mode = "lost-id"
                assert post(502, version=first) == {"error": "SubmissionUnknown"}
                assert post(task="med_running") == {"status": "Running"}
                assert post(task="med_failed") == {"status": "Failed"}
                assert post(task="med_clean")["assessment"]["decision"] is None
                assert post(400, task="invalid-id")["error"].startswith("InvalidInput")
                assert post(401, task="med_forbidden") == {"error": "Unauthorized"}
                for task in ("med_other", "med_wrongid", "med_invalidprogress", "med_invalidtime"):
                    assert post(502, task=task) == {"error": "Protocol"}
                assert post(task="med_running", cancel=True) == {"cancelled": True}
                assert post(task="med_running") == {"status": "Cancelled"}
                checked = post(task=TASK)
                outcome = checked["assessment"]
                assert outcome["decision"] == "Review" and outcome["coverage"] == {
                    "Visual": "Partial", "Speech": "Unavailable", "Ocr": "Partial"}
                evidence = {item["provider"]: item for item in outcome["labels"]}
                assert evidence["violence/prob"]["span_ms"] == [1125, 1125]
                assert evidence["violence/prob"]["confidence"] == {"value": .96, "maximum": 1.0}
                assert evidence["text-content-2.0/email"]["modality"] == "Ocr"
            finally:
                HTTP["stop"](application)
            log.seek(0)
            traces = log.read()
            assert all(value not in traces for value in (PRIVATE, SECRET, "X-Amz-Signature"))
        assert json.loads(artisan(cached, COMMAND, "query", TASK)) == checked
        assert json.loads(artisan(cached, COMMAND, "queue", TASK)) == {"queued": True}
        path = work / "worker-results.json"
        artisan(cached | {"MODERATION_FIXTURE_RESULT": str(path)}, "queue:work", "database", "--once")
        assert json.loads(path.read_text()) == checked
        with sqlite3.connect(database) as conn:
            assert conn.execute("SELECT COUNT(*) FROM jobs").fetchone() == (0,)
            assert conn.execute("SELECT COUNT(*) FROM failed_jobs").fetchone() == (0,)
        assert supplier.posted == [(first, original), (last, changed), (first, original), (first, original)]
        assert supplier.stops == ["med_running"]
        assert len(supplier.queried) == 12
        recorded = observations.read_text().splitlines()
        assert recorded.count("GET") == len(supplier.queried) and recorded.count("DELETE") == 1
    finally:
        supplier.shutdown()
        supplier.server_close()
        thread.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-sightengine-video-") as work, \
            VIDEO["OBJECTS"]["TOPOLOGY"]["object_storage"](True) as (env, request):
        exercise(args.binary.resolve(), Path(work), env, request)
    print("Sightengine 点播：HTTP／Artisan／独立 Worker、查询凭据保护、取消和时间证据通过")
