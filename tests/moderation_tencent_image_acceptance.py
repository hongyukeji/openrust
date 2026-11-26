"""以真实 HTTP、Artisan 和私有版本化 S3 验证腾讯 IMS 协议。"""

import argparse
import base64
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
TENCENT = runpy.run_path(str(ROOT / "tests/moderation_tencent_text_acceptance.py"))
IMAGES = runpy.run_path(str(ROOT / "tests/moderation_aliyun_image_acceptance.py"))
ORIGIN, KEY = IMAGES["ORIGIN"], IMAGES["KEY"]
MARKER = TENCENT["PRIVATE_MARKER"]


class Supplier(TENCENT["Supplier"]):
    def do_POST(self):
        data, access_id = self.verify("ImageModeration", "ims")
        assert access_id == "fixture-tencent-id"
        assert data["Type"] == "IMAGE" and data["BizType"] == "images"
        output = {"RequestId": "ims-image", "BizType": "images", "Suggestion": "Pass",
                  "Label": "Normal", "Score": 0, "LabelResults": None}
        version = None
        status = 200
        if "FileContent" in data:
            assert set(data) == {"Type", "BizType", "FileContent"}
            content = base64.b64decode(data["FileContent"], validate=True)
        else:
            assert set(data) == {"Type", "BizType", "FileUrl"}
            public = urlsplit(data["FileUrl"])
            assert f"{public.scheme}://{public.netloc}" == ORIGIN and public.path.endswith("/" + KEY)
            query = parse_qs(public.query, strict_parsing=True)
            version, = query["versionId"]
            assert query["X-Amz-Algorithm"] == ["AWS4-HMAC-SHA256"]
            assert 0 < int(query["X-Amz-Expires"][0]) <= 300
            status, content = IMAGES["download_media"](self.server, public)
        with self.server.lock:
            self.server.calls.append((version, status, content if status == 200 else None))
            mode = self.server.mode
        if status != 200:
            assert status == 404
            output = {"Error": {"Code": "ResourceUnavailable.ImageDownloadError", "Message": MARKER}}
        elif mode == "risk":
            output["LabelResults"] = [{"Label": "Terror", "SubLabel": "Knife", "Suggestion": "Review", "Score": 93}]
            output["OcrResults"] = [{"Label": "Ad", "SubLabel": "Promotion", "Suggestion": "Block", "Score": 98,
                                     "Text": MARKER, "Details": [{"Text": MARKER, "Keywords": [MARKER]}]}]
            output["ObjectResults"] = [{"Label": "Ad", "SubLabel": "QR", "Suggestion": "Review", "Score": 80}]
            output["LibResults"] = [{"Label": "NewRisk", "Suggestion": "Pass", "Score": 55}]
        elif mode == "bad-score":
            output["OcrResults"] = [{"Label": "Ad", "Suggestion": "Block", "Score": 101}]
        elif mode == "wrong-policy":
            output["BizType"] = "other"
        body = json.dumps({"Response": output}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def exercise(binary, root, storage_env, storage, *, media_ca=None):
    storage("PUT", query="versioning=", content=(
        b'<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">'
        b'<Status>Enabled</Status></VersioningConfiguration>'))
    original, changed = IMAGES["png"](15, 16, 17), IMAGES["png"](80, 81, 82)
    storage("PUT", KEY, content=original)
    storage("PUT", KEY, content=changed)
    document = XML.fromstring(storage("GET", query="versions="))
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
    IMAGES["configure_media"](supplier, storage_env, (first, latest), media_ca)
    thread = threading.Thread(target=supplier.serve_forever, daemon=True)
    thread.start()
    access_id = "fixture-tencent-id"
    secret, token = TENCENT["CREDENTIALS"][access_id]
    env = os.environ | storage_env | {
        "APP_BASE_PATH": str(root), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(app_port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "MODERATION_FIXTURE_TENCENT_IMAGE": f"http://127.0.0.1:{supplier.server_port}",
        "AWS_TEMPORARY_URL": ORIGIN, "MODERATION_FIXTURE_IMAGE_ORIGIN": ORIGIN,
        "FIXTURE_TENCENT_ID": access_id, "FIXTURE_TENCENT_SECRET": secret, "FIXTURE_TENCENT_TOKEN": token,
    }

    def artisan(current, *tokens, successful=True):
        result = subprocess.run([str(binary), "artisan", *tokens], env=current, capture_output=True,
                                text=True, timeout=25)
        assert (result.returncode == 0) == successful, result.stdout + result.stderr
        return result.stdout

    def post(status, version=None, disk="s3", body=None):
        connection = http.client.HTTPConnection("127.0.0.1", app_port, timeout=10)
        try:
            headers = {"Accept": "application/json"}
            if version is not None:
                headers.update({"X-Fixture-Version": version, "X-Fixture-Disk": disk})
            connection.request("POST", "/api/drivers/moderation/tencent-image", body, headers)
            response = connection.getresponse()
            content = response.read()
            assert response.status == status, (status, response.status, content[:200])
            assert MARKER.encode() not in content
            return json.loads(content)
        finally:
            connection.close()

    incomplete = {"decision": None, "combined": None, "request_id": "ims-image",
                  "labels": [], "visual": "Partial", "ocr": "Unavailable"}
    unknown = "00000000-0000-0000-0000-000000000000"
    try:
        assert json.loads(artisan(env, "drivers:moderation-tencent", "--image", first)) == incomplete
        artisan(env | {"AWS_TEMPORARY_URL": "https://wrong.fixture.invalid"},
                "drivers:moderation-tencent", "--image", first, successful=False)
        artisan(env, "config:cache")
        artisan(env, "route:cache")
        with (root / "http.log").open("w+") as log:
            process = TENCENT["HTTP"]["serve"](binary, env, log)
            try:
                assert post(200, body=original) == incomplete
                assert post(200, latest) == incomplete
                assert post(400, body=b"too short")["error"].startswith("InvalidInput")
                assert post(400, first, disk="local")["error"].startswith("InvalidInput")
                assert post(400, "null")["error"].startswith("InvalidInput")
                supplier.mode = "risk"
                risk = post(200, first)
                assert risk["decision"] == "Block" and risk["combined"] == "Block" and risk["ocr"] == "Partial"
                assert {(item["provider"], item["confidence"]["value"]) for item in risk["labels"]} == {
                    ("Terror/Knife", 93), ("Ad/Promotion", 98), ("Ad/QR", 80), ("NewRisk", 55)}
                supplier.mode = "bad-score"
                assert post(502, body=changed) == {"error": "Protocol"}
                supplier.mode = "wrong-policy"
                assert post(502, latest) == {"error": "Protocol"}
                assert post(502, unknown) == {"error": "ServiceUnavailable"}
            finally:
                TENCENT["HTTP"]["stop"](process)
            log.seek(0)
            recorded = log.read()
            assert all(value not in recorded for value in (MARKER, "X-Amz-Signature", secret, token))
        assert supplier.calls == [(first, 200, original), (None, 200, original), (latest, 200, changed),
                                  (first, 200, original), (None, 200, changed), (latest, 200, changed),
                                  (unknown, 404, None)]
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
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-tencent-image-") as work, \
            IMAGES["storage_fixture"](media_ca) as (env, request):
        exercise(args.binary.resolve(), Path(work), env, request, media_ca=media_ca)
    print("腾讯云图片：HTTP／Artisan、IMS TC3、私有 S3 版本、OCR 与部分覆盖通过")
