"""Sightengine 图像的内联 multipart、私有版本读取及取样覆盖验收。"""

import argparse
from base64 import b64decode
from email import policy
from email.parser import BytesParser
import http.client
import http.server
import json
import os
from pathlib import Path
import runpy
import socket
import struct
import subprocess
import tempfile
import threading
from urllib.parse import parse_qs, urlsplit
import xml.etree.ElementTree as XML
import zlib

ROOT = Path(__file__).resolve().parents[1]
IMAGES = runpy.run_path(str(ROOT / "tests/moderation_aliyun_image_acceptance.py"))
TEXT = runpy.run_path(str(ROOT / "tests/moderation_sightengine_text_acceptance.py"))
ORIGIN, KEY = IMAGES["ORIGIN"], IMAGES["KEY"]
SECRET, PRIVATE = TEXT["SECRET"], TEXT["PRIVATE"]
MODELS = "violence,nudity-2.1,text-content-2.0"
ANIMATION = b64decode(
    "R0lGODlhEAAQAIEAAA8QEQAAAAAAAAAAACH/C05FVFNDQVBFMi4wAwECAAAh+QQABgAAACwAAAAAEAAQAAAIHQABCBxIsKDBgwgTKlzIsKHDhxAjSpxIsaLFgQEBACH5BAEMAAEALAAAAAAQABAAgVBRUgAAAAAAAAAAAAgdAAEIHEiwoMGDCBMqXMiwocOHECNKnEixosWBAQEAOw=="
)



def png(red, green, blue):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 64, 64, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress((b"\x00" + bytes((red, green, blue)) * 64) * 64)) + chunk(b"IEND", b""))


class Supplier(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        assert self.path == "/1.0/check.json"
        data = self.rfile.read(int(self.headers["Content-Length"]))
        media_type = self.headers["Content-Type"]
        version = None
        status = 200
        if media_type.startswith("multipart/form-data"):
            message = BytesParser(policy=policy.default).parsebytes(
                b"Content-Type: " + media_type.encode() + b"\r\nMIME-Version: 1.0\r\n\r\n" + data)
            parts = {part.get_param("name", header="content-disposition"): part for part in message.iter_parts()}
            file_part = parts.pop("media")
            assert file_part.get_filename() == "image"
            content = file_part.get_payload(decode=True)
            fields = {key: part.get_payload(decode=True).decode() for key, part in parts.items()}
        else:
            assert media_type == "application/x-www-form-urlencoded"
            fields = {key: value[0] for key, value in parse_qs(data.decode(), strict_parsing=True).items()}
            public = urlsplit(fields.pop("url"))
            assert f"{public.scheme}://{public.netloc}" == ORIGIN and public.path.endswith("/" + KEY)
            query = parse_qs(public.query, strict_parsing=True)
            version, = query["versionId"]
            assert 0 < int(query["X-Amz-Expires"][0]) <= 300
            backend = self.server.storage
            connection = http.client.HTTPConnection(backend.hostname, backend.port, timeout=5)
            try:
                connection.request("GET", public.path + "?" + public.query, headers={"Host": backend.netloc})
                response = connection.getresponse()
                status, content = response.status, response.read()
            finally:
                connection.close()
        assert fields == {"api_user": "local-image-user", "api_secret": SECRET,
                          "models": MODELS, "text_categories": "email,insult"}
        with self.server.lock:
            self.server.calls.append((version, status, content if status == 200 else None))
            mode = self.server.mode
        output = {"status": "success", "request": {"id": "sightengine-image"}, "media": {"uri": PRIVATE},
                  "violence": {"prob": .05}, "nudity": {"sexual_activity": .02},
                  "text": {"detected_categories": [], "detections": {}}}
        if status != 200:
            assert status == 404
            output = {"status": "failure", "error": {"message": PRIVATE}}
            status = 400
        elif mode == "frames":
            del output["text"]
            output["data"] = {"frames": [
                {"violence": {"prob": .81}, "text": {"detected_categories": ["email"], "detections": {
                    "email": {"details": [{"match": PRIVATE}]}}}},
                {"violence": {"prob": .93}, "text": {"detected_categories": ["insult"],
                                                   "detections": {"insult": {"details": [{"match": PRIVATE}]}}}},
            ]}
        elif mode == "wrong-score":
            output["violence"]["prob"] = 1.01
        elif mode == "no-models":
            for key in ("violence", "nudity", "text"):
                del output[key]
        body = json.dumps(output).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def exercise(binary, root, storage_env, storage):
    storage("PUT", query="versioning", content=(
        b'<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">'
        b'<Status>Enabled</Status></VersioningConfiguration>'))
    original, changed = png(15, 16, 17), png(80, 81, 82)
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
        app_port = listener.getsockname()[1]
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Supplier)
    supplier.lock, supplier.calls, supplier.mode = threading.Lock(), [], None
    supplier.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    thread = threading.Thread(target=supplier.serve_forever, daemon=True)
    thread.start()
    env = os.environ | storage_env | {
        "APP_BASE_PATH": str(root), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(app_port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "MODERATION_FIXTURE_SIGHTENGINE_IMAGE": f"http://127.0.0.1:{supplier.server_port}",
        "AWS_TEMPORARY_URL": ORIGIN, "MODERATION_FIXTURE_IMAGE_ORIGIN": ORIGIN,
        "FIXTURE_SIGHTENGINE_USER": "local-image-user", "FIXTURE_SIGHTENGINE_SECRET": SECRET,
    }

    def artisan(current, *names, successful=True):
        result = subprocess.run([str(binary), "artisan", *names], env=current, capture_output=True,
                                text=True, timeout=25)
        assert (result.returncode == 0) == successful, result.stdout + result.stderr
        assert PRIVATE not in result.stdout + result.stderr
        return result.stdout

    def post(expect=200, *, version=None, disk="s3", body=None):
        connection = http.client.HTTPConnection("127.0.0.1", app_port, timeout=10)
        try:
            headers = {"Accept": "application/json"}
            if version is not None:
                headers.update({"X-Fixture-Version": version, "X-Fixture-Disk": disk})
            connection.request("POST", "/api/drivers/moderation/sightengine-image", body, headers)
            response = connection.getresponse()
            data = response.read()
            assert response.status == expect, (expect, response.status, data[:200])
            assert PRIVATE.encode() not in data
            return json.loads(data)
        finally:
            connection.close()

    partial = {"decision": None, "combined": None, "request_id": "sightengine-image",
               "labels": [], "visual": "Partial", "ocr": "Partial"}
    command = "drivers:moderation-sightengine-image"
    unknown = "00000000-0000-0000-0000-000000000000"
    try:
        assert json.loads(artisan(env, command, first)) == partial
        artisan(env | {"AWS_TEMPORARY_URL": "https://wrong.fixture.invalid"},
                command, first, successful=False)
        artisan(env, "config:cache")
        artisan(env, "route:cache")
        cached = env | {"MODERATION_FIXTURE_SIGHTENGINE_IMAGE": "http://127.0.0.1:1"}
        with (root / "http.log").open("w+") as log:
            process = TEXT["HTTP"]["serve"](binary, cached, log)
            try:
                assert post(body=original) == partial
                assert post(version=latest) == partial
                assert post(400, version=first, disk="local")["error"].startswith("InvalidInput")
                assert post(400, version="null")["error"].startswith("InvalidInput")
                supplier.mode = "frames"
                assessed = post(body=ANIMATION)
                assert assessed["decision"] == "Review" and assessed["combined"] == "Review"
                assert assessed["ocr"] == "Partial" and assessed["visual"] == "Partial"
                findings = {item["provider"]: item for item in assessed["labels"]}
                assert set(findings) == {"violence/prob", "text-content-2.0/email", "text-content-2.0/insult"}
                assert findings["violence/prob"]["confidence"] == {"value": .93, "maximum": 1.0}
                supplier.mode = "wrong-score"
                assert post(502, body=changed) == {"error": "Protocol"}
                supplier.mode = "no-models"
                assert post(body=changed) == partial | {"visual": "Unavailable", "ocr": "Unavailable"}
                assert post(400, version=unknown) == {"error": "Rejected"}
            finally:
                TEXT["HTTP"]["stop"](process)
            log.seek(0)
            recorded = log.read()
            assert all(value not in recorded for value in (PRIVATE, SECRET, "X-Amz-Signature"))
        assert supplier.calls == [(first, 200, original), (None, 200, original), (latest, 200, changed),
                                  (None, 200, ANIMATION), (None, 200, changed), (None, 200, changed),
                                  (unknown, 404, None)]
    finally:
        supplier.shutdown()
        supplier.server_close()
        thread.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-sightengine-image-") as work, \
            IMAGES["TOPOLOGY"]["object_storage"](True) as (env, request):
        exercise(args.binary.resolve(), Path(work), env, request)
    print("Sightengine 图像：HTTP／Artisan、multipart、私有 S3 版本、OCR 及帧取样通过")
