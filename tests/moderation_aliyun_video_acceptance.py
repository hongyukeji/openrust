"""阿里云点播协议，验证实际 HTTP、Artisan、数据库 Worker 和私有视频。"""

import argparse
import http.client
import http.server
import json
import os
from pathlib import Path
import runpy
import socket
import sqlite3
import ssl
import subprocess
import tempfile
import threading
import time
from urllib.parse import parse_qs, urlsplit
import xml.etree.ElementTree as XML

ROOT = Path(__file__).resolve().parents[1]
ALIYUN = runpy.run_path(str(ROOT / "tests/moderation_aliyun_acceptance.py"))
OBJECTS = runpy.run_path(str(ROOT / "tests/moderation_aliyun_image_acceptance.py"))
ORIGIN, KEY = OBJECTS["ORIGIN"], "versions/clip.mp4"
PRIVATE = "private-ocr-or-transcript-marker"
COMMAND = "drivers:moderation-aliyun-video"
TASK = "clip-fixture"


class Supplier(ALIYUN["Supplier"]):
    def do_POST(self):
        action = self.headers["x-acs-action"]
        assert action in {"VideoModeration", "VideoModerationResult"}
        parameters, access = self.verification(action, "videoDetection")
        assert access == next(iter(ALIYUN["CREDENTIALS"]))
        if action == "VideoModeration":
            if expected := getattr(self.server, "expected_callback", None):
                assert (parameters.pop("callback"), parameters.pop("seed")) == expected
                assert parameters.pop("cryptType") == "SHA256"
            assert set(parameters) == {"url"}
            source = urlsplit(parameters["url"])
            assert f"{source.scheme}://{source.netloc}" == ORIGIN and source.path.endswith("/" + KEY)
            fields = parse_qs(source.query, strict_parsing=True)
            version, = fields["versionId"]
            assert "X-Amz-Signature" in fields and 0 < int(fields["X-Amz-Expires"][0]) <= 300
            status, payload = OBJECTS["download_media"](self.server, source)
            assert status == 200
            with self.server.lock:
                mode = self.server.submit_mode
                self.server.submissions.append((version, payload))
            if gate := getattr(self.server, "gate", None):
                reached, release = gate
                reached.set()
                assert release.wait(12), "回调夹具未释放受理凭据"
            if mode == "lost-response":
                time.sleep(.8)
            code = {"accepted": 200, "nearline": 288, "quota": 480, "lost-response": 200,
                    "unknown": 200}.get(mode)
            result = {"TaskId": TASK} if mode != "unknown" else {}
        else:
            assert set(parameters) == {"taskId"}
            task = parameters["taskId"]
            with self.server.lock:
                self.server.queries.append(task)
            code = {"pending": "288", "running": 280, "expired": 409, "failed": 407,
                    "quota": 403, "unauthorized": 408, "server-failure": 500}.get(task, 200)
            result = {"TaskId": task}
            if code in {200, 280}:
                result["RiskLevel"] = "none"
                result["FrameResult"] = {"FrameNum": 2, "FrameSummarys": [], "Frames": [], "RiskLevel": "none"}
                if task == TASK:
                    result["FrameResult"] = {
                        "FrameNum": 2, "RiskLevel": "medium",
                        "FrameSummarys": [{"Label": "violent_explosion"}, {"Label": "logo_brand_lib"}],
                        "Frames": [{"Offset": 1.125, "RiskLevel": "medium", "TempUrl": PRIVATE,
                                    "Results": [{"Service": "baselineCheck", "TextInImage": {"OcrDatas": [PRIVATE]},
                                                 "Result": [{"Label": "violent_explosion", "Confidence": 88.25}]}]}],
                    }
                    result["AudioResult"] = {"RiskLevel": "medium", "SliceDetails": [
                        {"StartTime": .125, "EndTime": .75, "Labels": "sexual_content", "RiskLevel": "high",
                         "Text": PRIVATE, "Url": PRIVATE, "Extend": PRIVATE},
                    ]}
                elif task == "wrong-task":
                    result["TaskId"] = "foreign-task"
                elif task == "missing-risk":
                    del result["RiskLevel"]
                elif task == "reversed-time":
                    result["AudioResult"] = {"SliceDetails": [
                        {"StartTime": 2, "EndTime": 1, "Labels": "sexual_content"}]}
                elif task == "manual-review":
                    result["ManualTaskId"] = "manual-work-remains"
            if task in {"pending", "expired", "failed"}:
                result = None
        body = json.dumps({"Code": code, "Data": result, "RequestId": "aliyun-video-reference",
                           "Message": PRIVATE}).encode()
        try:
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            assert action == "VideoModeration" and mode == "lost-response"


def generate(directory, name, color, audio):
    path = directory / name
    arguments = ["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error",
                 "-f", "lavfi", "-i", f"color={color}:s=64x64:r=8:d=2"]
    if audio:
        arguments += ["-f", "lavfi", "-i", "sine=frequency=220:duration=2", "-c:a", "aac", "-shortest"]
    subprocess.run(arguments + ["-c:v", "mpeg4", str(path)], check=True, timeout=10)
    result = subprocess.check_output(["ffprobe", "-v", "error", "-show_entries", "stream=codec_type",
                                      "-of", "json", str(path)], timeout=10)
    streams = {stream["codec_type"] for stream in json.loads(result)["streams"]}
    assert streams == ({"video", "audio"} if audio else {"video"})
    return path.read_bytes()


def exercise(binary, root, storage_env, storage, *, media_ca=None, video_dir=None):
    storage("PUT", query="versioning=", content=(
        b'<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">'
        b'<Status>Enabled</Status></VersioningConfiguration>'))
    if video_dir is None:
        first_clip = generate(root, "video-with-audio.mp4", "blue", True)
        new_clip = generate(root, "video-visual-only.mp4", "red", False)
    else:
        first_clip = (video_dir / "video-with-audio.mp4").read_bytes()
        new_clip = (video_dir / "video-visual-only.mp4").read_bytes()
        assert first_clip and new_clip and first_clip != new_clip
    storage("PUT", KEY, content=first_clip)
    storage("PUT", KEY, content=new_clip)
    listing = XML.fromstring(storage("GET", query="versions="))
    versions = {node.findtext("{*}IsLatest"): node.findtext("{*}VersionId")
                for node in listing.findall("{*}Version") if node.findtext("{*}Key") == KEY}
    first, last = versions["false"], versions["true"]
    assert first != last and "null" not in {first, last}
    storage("GET", KEY, status=403, signed=False)

    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Supplier)
    supplier.lock, supplier.nonces, supplier.unexpected = threading.Lock(), set(), []
    supplier.submissions, supplier.queries = [], []
    supplier.submit_mode = "accepted"
    supplier.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    supplier.media_tls = ssl.create_default_context(cafile=media_ca) if media_ca else None
    supplier.tls_verified = False
    supplier.latest = last
    thread = threading.Thread(target=supplier.serve_forever, daemon=True)
    thread.start()

    access_id = next(iter(ALIYUN["CREDENTIALS"]))
    secret, token = ALIYUN["CREDENTIALS"][access_id]
    database = root / "queue.sqlite"
    database.touch()
    env = os.environ | storage_env | {
        "APP_BASE_PATH": str(root), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DB_CONNECTION": "sqlite", "DB_DATABASE": str(database), "QUEUE_CONNECTION": "database",
        "DRIVER_HTTP": "1", "MODERATION_FIXTURE_ALIYUN_VIDEO": f"http://127.0.0.1:{supplier.server_port}",
        "AWS_TEMPORARY_URL": ORIGIN, "MODERATION_FIXTURE_IMAGE_ORIGIN": ORIGIN,
        "FIXTURE_ALIYUN_ID": access_id, "FIXTURE_ALIYUN_SECRET": secret, "FIXTURE_ALIYUN_TOKEN": token,
    }
    env.pop("DB_URL", None)

    def artisan(current, *tokens, successful=True):
        process = subprocess.run([str(binary), "artisan", *tokens], env=current, capture_output=True,
                                 text=True, timeout=25)
        assert (process.returncode == 0) == successful, process.stdout + process.stderr
        assert PRIVATE not in process.stdout + process.stderr
        return process.stdout

    def post(expected=200, *, version=None, task=None, disk="s3"):
        connector = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            headers = {"Accept": "application/json"}
            if version:
                headers.update({"X-Fixture-Version": version, "X-Fixture-Disk": disk})
            if task:
                headers["X-Fixture-Task"] = task
            connector.request("POST", "/api/drivers/moderation/aliyun-video", headers=headers)
            response = connector.getresponse()
            body = response.read()
            assert response.status == expected, (response.status, expected, body[:250])
            assert PRIVATE.encode() not in body
            return json.loads(body)
        finally:
            connector.close()

    accepted = {"task": TASK, "cancel": False}
    try:
        assert json.loads(artisan(env, COMMAND, "submit", first)) == accepted
        artisan(env | {"MODERATION_FIXTURE_VIDEO_SERVICE": "liveStreamDetection"},
                COMMAND, "submit", first, successful=False)
        artisan(env | {"MODERATION_FIXTURE_VIDEO_REGION": "cn-shenzhen"},
                COMMAND, "submit", first, successful=False)
        artisan(env | {"AWS_TEMPORARY_URL": "https://mismatch.fixture.invalid"},
                COMMAND, "submit", first, successful=False)
        artisan(env, "migrate", "--force")
        artisan(env, "config:cache")
        artisan(env, "route:cache")
        cache = (root / "config.json").read_text()
        assert secret not in cache and token not in cache and "FIXTURE_ALIYUN_SECRET" in cache
        cached = env | {"MODERATION_FIXTURE_VIDEO_SERVICE": "unsupported-after-config-cache"}
        with (root / "http.log").open("w+") as log:
            application = ALIYUN["HTTP"]["serve"](binary, cached, log)
            try:
                supplier.submit_mode = "nearline"
                assert post(version=last) == accepted
                assert post(400, version=first, disk="local")["error"].startswith("InvalidInput")
                assert post(400, version="null")["error"].startswith("InvalidInput")
                supplier.submit_mode = "quota"
                assert post(429, version=first) == {"error": "Throttled"}
                supplier.submit_mode = "lost-response"
                assert post(502, version=first) == {"error": "SubmissionUnknown"}
                supplier.submit_mode = "unknown"
                assert post(502, version=first) == {"error": "SubmissionUnknown"}
                for task, state in (("pending", "Pending"), ("running", "Running"),
                                    ("expired", "Expired"), ("failed", "Failed")):
                    assert post(task=task) == {"status": state}
                for task, status, error in (("quota", 429, "Throttled"), ("unauthorized", 401, "Unauthorized"),
                                            ("server-failure", 502, "ServiceUnavailable"),
                                            ("wrong-task", 502, "Protocol"), ("missing-risk", 502, "Protocol"),
                                            ("reversed-time", 502, "Protocol")):
                    assert post(status, task=task) == {"error": error}
                clean = post(task="silent")
                assert clean["assessment"]["coverage"] == {"Visual": "Partial", "Speech": "Unavailable", "Ocr": "Unavailable"}
                assert clean["assessment"]["decision"] is None
                assert post(task="manual-review")["assessment"]["decision"] == "Review"
                complete = post(task=TASK)
                assessed = complete["assessment"]
                assert assessed["decision"] == "Block" and assessed["coverage"] == dict.fromkeys(
                    ["Visual", "Speech", "Ocr"], "Partial")
                evidence = {item["provider"]: item for item in assessed["labels"]}
                assert evidence["violent_explosion"]["span_ms"] == [1125, 1125]
                assert evidence["violent_explosion"]["modality"] == "Visual"
                assert evidence["violent_explosion"]["confidence"] == {"value": 88.25, "maximum": 100.0}
                assert evidence["sexual_content"]["span_ms"] == [125, 750]
                assert evidence["sexual_content"]["modality"] == "Speech"
                assert "span_ms" not in evidence["logo_brand_lib"]
            finally:
                ALIYUN["HTTP"]["stop"](application)
            log.seek(0)
            trace = log.read()
            assert all(value not in trace for value in (PRIVATE, secret, token, "X-Amz-Signature", "manual-work-remains"))
        assert json.loads(artisan(cached, COMMAND, "query", TASK)) == complete
        assert json.loads(artisan(cached, COMMAND, "queue", TASK)) == {"queued": True}
        with sqlite3.connect(database) as db:
            (payload,), = db.execute("SELECT payload FROM jobs").fetchall()
            assert TASK in payload and KEY not in payload and secret not in payload
        result = root / "worker-response.json"
        artisan(cached | {"MODERATION_FIXTURE_RESULT": str(result)}, "queue:work", "database", "--once")
        assert json.loads(result.read_text()) == complete
        with sqlite3.connect(database) as db:
            assert db.execute("SELECT COUNT(*) FROM jobs").fetchone() == (0,)
            assert db.execute("SELECT COUNT(*) FROM failed_jobs").fetchone() == (0,)
        assert supplier.submissions == [(first, first_clip), (last, new_clip)] + [(first, first_clip)] * 3
        assert supplier.queries[-3:] == [TASK, TASK, TASK] and not supplier.unexpected
        assert len(supplier.nonces) == len(supplier.queries) + len(supplier.submissions)
        assert supplier.tls_verified == (media_ca is not None)
    finally:
        supplier.shutdown()
        supplier.server_close()
        thread.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    media_ca = os.environ.get("OPENRUST_TEST_MODERATION_MEDIA_CA")
    video_dir = os.environ.get("OPENRUST_TEST_MODERATION_VIDEO_DIR")
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-aliyun-video-") as work, \
            OBJECTS["storage_fixture"](media_ca) as (env, request):
        exercise(args.binary.resolve(), Path(work), env, request, media_ca=media_ca,
                 video_dir=Path(video_dir) if video_dir else None)
    print("阿里云点播：HTTP／Artisan／独立 Worker、ACS3、私有视频版本、时间证据和状态通过")
