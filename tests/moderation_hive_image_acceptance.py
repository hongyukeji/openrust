"""Hive V2 视觉项目、真实 HTTP／Artisan 与私有 S3 版本验收。"""

import argparse
from email import policy
from email.parser import BytesParser
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
from urllib.parse import parse_qs, urlsplit
import xml.etree.ElementTree as XML

ROOT = Path(__file__).resolve().parents[1]
IMAGES = runpy.run_path(str(ROOT / "tests/moderation_sightengine_image_acceptance.py"))
TEXT = runpy.run_path(str(ROOT / "tests/moderation_hive_text_acceptance.py"))
ORIGIN, KEY = IMAGES["ORIGIN"], IMAGES["KEY"]
TEXT_TOKEN = TEXT["CURRENT"]
IMAGE_TOKEN = "private-hive-visual-project-token"
PRIVATE = TEXT["PRIVATE"]
PROJECT = 48173


class Supplier(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        assert self.path == "/api/v2/task/sync"
        body = self.rfile.read(int(self.headers["Content-Length"]))
        token, media_type = self.headers["Authorization"], self.headers["Content-Type"]
        version = None
        code = 200
        if media_type.startswith("multipart/form-data"):
            message = BytesParser(policy=policy.default).parsebytes(
                b"Content-Type: " + media_type.encode() + b"\r\nMIME-Version: 1.0\r\n\r\n" + body)
            media, = list(message.iter_parts())
            assert media.get_param("name", header="content-disposition") == "media"
            assert media.get_filename() == "image"
            payload = media.get_payload(decode=True)
        else:
            assert media_type == "application/x-www-form-urlencoded"
            form = parse_qs(body.decode(), strict_parsing=True)
            if "text_data" in form:
                assert form == {"text_data": ["internal-visibility"]}
                assert token == "Token " + TEXT_TOKEN
                classes = [{"class": name, "score": 0} for name in TEXT["CLASSES"]]
                self.respond(200, {
                    "id": "hive-text-task", "code": 200, "project_id": TEXT["PROJECT"],
                    "status": [{"status": {"code": "0"}, "response": {
                        "input": {"id": "hive-text-task", "project_id": TEXT["PROJECT"],
                                  "model_type": "TEXT_CLASSIFICATION"},
                        "language": "EN", "moderated_classes": TEXT["CLASSES"], "output": [{"classes": classes}],
                    }}],
                })
                with self.server.lock:
                    self.server.text_calls += 1
                return
            assert set(form) == {"url"}
            url, = form["url"]
            public = urlsplit(url)
            assert f"{public.scheme}://{public.netloc}" == ORIGIN and public.path.endswith("/" + KEY)
            query = parse_qs(public.query, strict_parsing=True)
            version, = query["versionId"]
            assert "X-Amz-Signature" in query and 0 < int(query["X-Amz-Expires"][0]) <= 300
            backend = self.server.storage
            connection = http.client.HTTPConnection(backend.hostname, backend.port, timeout=5)
            try:
                connection.request("GET", public.path + "?" + public.query, headers={"Host": backend.netloc})
                response = connection.getresponse()
                code, payload = response.status, response.read()
            finally:
                connection.close()
        assert token == "Token " + IMAGE_TOKEN
        with self.server.lock:
            self.server.calls.append((version, code, payload if code == 200 else None))
            mode = self.server.mode
        if code != 200:
            assert code == 404
            self.respond(400, {"error": PRIVATE})
            return

        task = "hive-image-task"
        scores = [{"class": "general_nsfw", "score": .05}, {"class": "gun_in_hand", "score": .1}]
        data = {"input": {"model_type": "CATEGORIZATION", "project_id": PROJECT, "id": task,
                          "media": {"uri": PRIVATE}},
                "output": [{"time": 0, "classes": scores}]}
        entry = {"status": {"code": 0}, "response": data}
        result = {"id": task, "code": 200, "project_id": PROJECT, "status": entry}
        if mode == "frames":
            data["output"].append({"time": 1, "classes": [
                {"class": "general_nsfw", "score": .91}, {"class": "gun_in_hand", "score": .99}]})
            data["output"].append({"time": 2, "classes": [{"class": "gun_in_hand", "score": .95}]})
            entry["status"]["code"] = "0"
            result["status"] = [entry]
        elif mode == "wrong-project":
            data["input"]["project_id"] = TEXT["PROJECT"]
        elif mode == "wrong-model":
            data["input"]["model_type"] = "TEXT_CLASSIFICATION"
        elif mode == "bad-confidence":
            scores[0]["score"] = 1.05
        elif mode == "duplicate":
            scores.append(scores[0])
        elif mode == "missing":
            data["output"] = []
        elif mode == "still":
            data["input"]["media"].update({"type": "PHOTO", "num_frames": 1, "duration": 0})
        self.respond(200, result)

    def respond(self, status, result):
        payload = json.dumps(result).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def exercise(binary, work, storage_env, storage):
    storage("PUT", query="versioning", content=(
        b'<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">'
        b'<Status>Enabled</Status></VersioningConfiguration>'))
    original, changed = IMAGES["png"](44, 33, 22), IMAGES["png"](11, 99, 88)
    storage("PUT", KEY, content=original)
    storage("PUT", KEY, content=changed)
    document = XML.fromstring(storage("GET", query="versions&prefix=versions%2Fimage.png"))
    versions = {version.findtext("{*}IsLatest"): version.findtext("{*}VersionId")
                for version in document.findall("{*}Version") if version.findtext("{*}Key") == KEY}
    first, latest = versions["false"], versions["true"]
    assert first != latest and "null" not in {first, latest}
    storage("GET", KEY, status=403, signed=False)
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Supplier)
    supplier.lock, supplier.calls, supplier.mode = threading.Lock(), [], None
    supplier.text_calls = 0
    supplier.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    thread = threading.Thread(target=supplier.serve_forever, daemon=True)
    thread.start()
    env = os.environ | storage_env | {
        "APP_BASE_PATH": str(work), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(work / "config.json"), "APP_ROUTES_CACHE": str(work / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "MODERATION_FIXTURE_HIVE_IMAGE": f"http://127.0.0.1:{supplier.server_port}",
        "AWS_TEMPORARY_URL": ORIGIN, "MODERATION_FIXTURE_IMAGE_ORIGIN": ORIGIN,
        "FIXTURE_HIVE_TOKEN": TEXT_TOKEN, "FIXTURE_HIVE_IMAGE_TOKEN": IMAGE_TOKEN,
    }

    def artisan(current, *tokens, successful=True):
        result = subprocess.run([str(binary), "artisan", *tokens], env=current, capture_output=True,
                                text=True, timeout=20)
        assert (result.returncode == 0) == successful, result.stdout + result.stderr
        assert PRIVATE not in result.stdout + result.stderr
        return result.stdout

    def request(expected=200, *, version=None, disk="s3", body=None, text=False):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            headers = {"Accept": "application/json"}
            if version is not None:
                headers.update({"X-Fixture-Version": version, "X-Fixture-Disk": disk})
            if text:
                headers["X-Fixture-Profile"] = "hive-images"
            path = "/api/drivers/moderation/hive" + ("" if text else "-image")
            connection.request("POST", path, body, headers)
            response = connection.getresponse()
            payload = response.read()
            assert response.status == expected, (response.status, expected, payload[:200])
            assert PRIVATE.encode() not in payload
            return json.loads(payload)
        finally:
            connection.close()

    partial = {"decision": None, "combined": None, "labels": [], "request_id": "hive-image-task",
               "visual": "Partial", "ocr": "Unavailable"}
    command = "drivers:moderation-hive-image"
    unknown = "00000000-0000-0000-0000-000000000000"
    try:
        assert json.loads(artisan(env, command, first)) == partial
        artisan(env | {"AWS_TEMPORARY_URL": "https://mismatch.fixture.invalid"}, command, first, successful=False)
        artisan(env, "config:cache")
        artisan(env, "route:cache")
        serialized = (work / "config.json").read_text()
        assert "FIXTURE_HIVE_IMAGE_TOKEN" in serialized and "FIXTURE_HIVE_TOKEN" in serialized
        assert IMAGE_TOKEN not in serialized and TEXT_TOKEN not in serialized
        with (work / "http.log").open("w+") as log:
            application = TEXT["HTTP"]["serve"](binary, env, log)
            try:
                assert request(body=original) == partial
                assert request(version=latest) == partial
                assert request(400, version=first, disk="local")["error"].startswith("InvalidInput")
                assert request(400, version="null")["error"].startswith("InvalidInput")
                assert request(body=b"internal-visibility", text=True)["request_id"] == "hive-text-task"
                supplier.mode = "frames"
                outcome = request(body=IMAGES["ANIMATION"])
                assert outcome["decision"] == "Review" and outcome["visual"] == "Partial"
                assert {item["provider"]: item["confidence"]["value"] for item in outcome["labels"]} == {
                    "general_nsfw": .91, "gun_in_hand": .99}
                for mode in ("wrong-project", "wrong-model", "bad-confidence", "duplicate"):
                    supplier.mode = mode
                    assert request(502, body=changed) == {"error": "Protocol"}
                supplier.mode = "missing"
                assert request(body=changed) == partial | {"visual": "Unavailable"}
                supplier.mode = "still"
                assert request(body=original) == partial | {"visual": "Complete", "decision": "Allow"}
                assert request(400, version=unknown) == {"error": "Rejected"}
            finally:
                TEXT["HTTP"]["stop"](application)
            log.seek(0)
            traces = log.read()
            assert all(value not in traces for value in (PRIVATE, IMAGE_TOKEN, TEXT_TOKEN, "X-Amz-Signature"))
        assert supplier.text_calls == 1
        assert supplier.calls == [(first, 200, original), (None, 200, original), (latest, 200, changed),
                                  (None, 200, IMAGES["ANIMATION"])] + [(None, 200, changed)] * 5 + [(None, 200, original), (unknown, 404, None)]
    finally:
        supplier.shutdown()
        supplier.server_close()
        thread.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-hive-image-") as directory, \
            IMAGES["IMAGES"]["TOPOLOGY"]["object_storage"](True) as (env, request):
        exercise(args.binary.resolve(), Path(directory), env, request)
    print("Hive V2 图片：HTTP／Artisan、项目凭据隔离、multipart、私有 S3、采样与失败检查通过")
