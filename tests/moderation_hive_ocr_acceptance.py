"""Hive V2 独立 OCR 项目的真实 HTTP／Artisan 与隐私、覆盖边界。"""

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
HTTP = IMAGES["TEXT"]["HTTP"]
ORIGIN, KEY = IMAGES["ORIGIN"], IMAGES["KEY"]
TOKEN = "hive-ocr-isolated-token"
PRIVATE = "ocr-source-must-stay-private"


class Supplier(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        assert self.path == "/api/v2/task/sync" and self.headers["Authorization"] == "Token " + TOKEN
        body = self.rfile.read(int(self.headers["Content-Length"]))
        content_type = self.headers["Content-Type"]
        version = None
        if content_type.startswith("multipart/form-data"):
            message = BytesParser(policy=policy.default).parsebytes(
                b"Content-Type: " + content_type.encode() + b"\r\nMIME-Version: 1.0\r\n\r\n" + body)
            part, = list(message.iter_parts())
            assert part.get_param("name", header="content-disposition") == "media"
            data = part.get_payload(decode=True)
        else:
            assert content_type == "application/x-www-form-urlencoded"
            fields = parse_qs(body.decode(), strict_parsing=True)
            assert set(fields) == {"url"}
            public = urlsplit(fields["url"][0])
            assert f"{public.scheme}://{public.netloc}" == ORIGIN and public.path.endswith("/" + KEY)
            fields = parse_qs(public.query, strict_parsing=True)
            version, = fields["versionId"]
            assert "X-Amz-Signature" in fields and 0 < int(fields["X-Amz-Expires"][0]) <= 300
            backend = self.server.storage
            connection = http.client.HTTPConnection(backend.hostname, backend.port, timeout=5)
            try:
                connection.request("GET", public.path + "?" + public.query, headers={"Host": backend.netloc})
                response = connection.getresponse()
                data = response.read()
                assert response.status == 200
            finally:
                connection.close()
        with self.server.lock:
            mode = self.server.mode
            self.server.calls.append((version, data))
        project, job = 48174, "hive-ocr-task"
        metadata = {"type": "PHOTO", "num_frames": 1, "duration": 0, "uri": PRIVATE}
        findings = {"block_text": PRIVATE, "classes": [{"class": "spam", "score": 0}, {"class": "hate", "score": 0}]}
        frame = {"time": 0, "frame_results": [findings]}
        model = {"input": {"model_type": "OCR_MODERATION", "id": job, "project_id": project, "media": metadata},
                 "output": [frame]}
        if mode == "animated":
            metadata.update({"type": "GIF", "num_frames": 2, "duration": .18})
            findings["classes"][1]["score"] = 2
            second = {
                "block_text": PRIVATE, "classes": [{"class": "spam", "score": 0}, {"class": "hate", "score": 3}],
                "pii_entities": [{"value": PRIVATE + "@example.invalid"}],
                "text_filters": [{"value": PRIVATE}], "custom_classes": [{"class": PRIVATE, "value": PRIVATE}],
                "bounding_poly": [{"classes": [{"class": PRIVATE, "score": 1}]}],
            }
            model["output"].append({"time": .06, "frame_results": [second]})
        elif mode == "unsupported":
            findings["classes"][0]["score"] = -1
        elif mode == "still-no-text":
            frame["frame_results"] = []
        elif mode == "unproven":
            del metadata["num_frames"]
        elif mode == "empty-output":
            model["output"] = []
        elif mode == "malformed":
            del frame["frame_results"]
        elif mode == "wrong-model":
            model["input"]["model_type"] = "BOUNDING_BOX"
        elif mode == "bad-score":
            findings["classes"][0]["score"] = 4
        envelope = {"id": job, "code": 200, "project_id": project,
                    "status": [{"status": {"code": "0"}, "response": model}]}
        raw = json.dumps(envelope).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)


def exercise(binary, work, storage_env, storage):
    storage("PUT", query="versioning", content=(
        b'<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">'
        b'<Status>Enabled</Status></VersioningConfiguration>'))
    first_image, last_image = IMAGES["png"](24, 45, 66), IMAGES["png"](66, 45, 24)
    storage("PUT", KEY, content=first_image)
    storage("PUT", KEY, content=last_image)
    document = XML.fromstring(storage("GET", query="versions&prefix=versions%2Fimage.png"))
    versions = {version.findtext("{*}IsLatest"): version.findtext("{*}VersionId")
                for version in document.findall("{*}Version") if version.findtext("{*}Key") == KEY}
    first, last = versions["false"], versions["true"]
    assert first != last and "null" not in {first, last}
    storage("GET", KEY, status=403, signed=False)
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Supplier)
    supplier.lock, supplier.calls, supplier.mode = threading.Lock(), [], None
    supplier.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    thread = threading.Thread(target=supplier.serve_forever, daemon=True)
    thread.start()
    env = os.environ | storage_env | {
        "APP_BASE_PATH": str(work), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(work / "config.json"), "APP_ROUTES_CACHE": str(work / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "MODERATION_FIXTURE_HIVE_OCR": f"http://127.0.0.1:{supplier.server_port}",
        "AWS_TEMPORARY_URL": ORIGIN, "MODERATION_FIXTURE_IMAGE_ORIGIN": ORIGIN, "FIXTURE_HIVE_OCR_TOKEN": TOKEN,
    }

    def artisan(current, *tokens, successful=True):
        result = subprocess.run([str(binary), "artisan", *tokens], env=current, capture_output=True,
                                text=True, timeout=25)
        assert (result.returncode == 0) == successful, result.stdout + result.stderr
        assert PRIVATE not in result.stdout + result.stderr
        return result.stdout

    def post(status=200, *, version=None, body=None):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            headers = {"Accept": "application/json"}
            if version:
                headers["X-Fixture-Version"] = version
            connection.request("POST", "/api/drivers/moderation/hive-ocr", body, headers)
            response = connection.getresponse()
            data = response.read()
            assert response.status == status, (status, response.status, data[:200])
            assert PRIVATE.encode() not in data
            return json.loads(data)
        finally:
            connection.close()

    allow = {"decision": "Allow", "combined": None, "request_id": "hive-ocr-task",
             "labels": [], "visual": "Unavailable", "ocr": "Complete"}
    partial = allow | {"decision": None, "ocr": "Partial"}
    try:
        assert json.loads(artisan(env, "drivers:moderation-hive-ocr", first)) == allow
        artisan(env | {"FIXTURE_HIVE_OCR_TOKEN": ""}, "drivers:moderation-hive-ocr", first, successful=False)
        artisan(env, "config:cache")
        artisan(env, "route:cache")
        assert TOKEN not in (work / "config.json").read_text()
        with (work / "http.log").open("w+") as log:
            application = HTTP["serve"](binary, env, log)
            try:
                assert post(version=last) == allow
                supplier.mode = "animated"
                risk = post(body=IMAGES["ANIMATION"])
                assert risk["decision"] == "Review" and risk["ocr"] == "Partial"
                assert {item["provider"] for item in risk["labels"]} == {
                    "hate", "pii_entities", "text_filters", "custom_classes"}
                assert [item["confidence"]["value"] for item in risk["labels"] if "confidence" in item] == [3.0]
                supplier.mode = "unsupported"
                assert post(body=first_image) == partial
                supplier.mode = "unproven"
                assert post(body=first_image) == partial
                supplier.mode = "still-no-text"
                assert post(body=first_image) == allow
                supplier.mode = "empty-output"
                assert post(body=first_image) == partial | {"ocr": "Unavailable"}
                for mode in ("malformed", "wrong-model", "bad-score"):
                    supplier.mode = mode
                    assert post(502, body=first_image) == {"error": "Protocol"}
            finally:
                HTTP["stop"](application)
            log.seek(0)
            traces = log.read()
            assert all(value not in traces for value in (PRIVATE, TOKEN, "X-Amz-Signature"))
        assert supplier.calls == [(first, first_image), (last, last_image), (None, IMAGES["ANIMATION"])] + [
            (None, first_image)] * 7
    finally:
        supplier.shutdown()
        supplier.server_close()
        thread.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-hive-ocr-") as directory, \
            IMAGES["IMAGES"]["TOPOLOGY"]["object_storage"](True) as (env, request):
        exercise(args.binary.resolve(), Path(directory), env, request)
    print("Hive OCR：HTTP／Artisan、独立项目、私有 S3、静态完整／动图部分、证据和脱敏通过")
