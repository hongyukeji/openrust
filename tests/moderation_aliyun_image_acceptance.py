"""阿里云图片替身签名验收，实际读取同一私有 S3 对象的不同版本。"""

import argparse
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
TOPOLOGY = runpy.run_path(str(ROOT / "tests/webhooks_topology_acceptance.py"))
ALIYUN = runpy.run_path(str(ROOT / "tests/moderation_aliyun_acceptance.py"))
ORIGIN = "https://moderation-media.fixture.invalid"
KEY = "versions/image.png"


def png(red, green, blue):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(bytes((0, red, green, blue)))) + chunk(b"IEND", b""))


class Supplier(ALIYUN["Supplier"]):
    def do_POST(self):
        parameters, _ = self.verification("ImageModeration", "baselineCheck")
        assert set(parameters) == {"imageUrl"}
        public = urlsplit(parameters["imageUrl"])
        query = parse_qs(public.query, strict_parsing=True)
        version, = query["versionId"]
        assert f"{public.scheme}://{public.netloc}" == ORIGIN
        assert query["X-Amz-Algorithm"] == ["AWS4-HMAC-SHA256"]
        assert 0 < int(query["X-Amz-Expires"][0]) <= 300
        assert public.path.endswith("/" + KEY)

        # 模拟厂商从 HTTPS 公共入口下载：转发原始签名路径到隔离存储，并还原 SDK 签名时的 Host。
        backend = self.server.storage
        connection = http.client.HTTPConnection(backend.hostname, backend.port, timeout=5)
        try:
            connection.request("GET", public.path + "?" + public.query, headers={"Host": backend.netloc})
            response = connection.getresponse()
            payload = response.read()
        finally:
            connection.close()
        with self.server.lock:
            self.server.results.append((version, response.status, payload if response.status == 200 else None))
        if response.status == 200:
            result = {"RiskLevel": "none", "Result": []}
            if version == self.server.latest:
                result = {"RiskLevel": "high", "Result": [{"Label": "violent_explosion", "Confidence": 88.5}]}
            if self.server.response_mode == "invalid-confidence":
                result = {"RiskLevel": "high", "Result": [{"Label": "violent_explosion", "Confidence": 101}]}
            elif self.server.response_mode == "missing-results":
                result = {"RiskLevel": "none"}
            elif self.server.response_mode == "sampled":
                result = {"RiskLevel": "none", "Result": [], "FrameNum": 1}
            elif self.server.response_mode == "ambiguous":
                result = {"RiskLevel": "none", "Result": [{"Label": "nonLabel"}]}
            envelope = {"Code": 200, "Data": result, "RequestId": "image-reference"}
        else:
            assert response.status == 404
            envelope = {"Code": 400, "RequestId": "missing-image"}
        body = json.dumps(envelope).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def exercise(binary, root, storage_env, storage_request):
    storage_request("PUT", query="versioning", content=(
        b'<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">'
        b'<Status>Enabled</Status></VersioningConfiguration>'
    ))
    original, changed = png(0, 123, 42), png(205, 4, 7)
    storage_request("PUT", KEY, content=original)
    storage_request("PUT", KEY, content=changed)
    version_document = XML.fromstring(storage_request("GET", query="versions&prefix=versions%2Fimage.png"))
    versions = {version.findtext("{*}IsLatest"): version.findtext("{*}VersionId")
                for version in version_document.findall("{*}Version") if version.findtext("{*}Key") == KEY}
    first, latest = versions["false"], versions["true"]
    assert first != latest and first != "null" and latest != "null"
    storage_request("GET", KEY, status=403, signed=False)
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        app_port = listener.getsockname()[1]
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Supplier)
    supplier.lock, supplier.nonces, supplier.results = threading.Lock(), set(), []
    supplier.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    supplier.latest = latest
    supplier.response_mode = None
    thread = threading.Thread(target=supplier.serve_forever, daemon=True)
    thread.start()

    access_id = next(iter(ALIYUN["CREDENTIALS"]))
    secret, token = ALIYUN["CREDENTIALS"][access_id]
    env = os.environ | storage_env | {
        "APP_BASE_PATH": str(root), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(app_port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "MODERATION_FIXTURE_ALIYUN": f"http://127.0.0.1:{supplier.server_port}",
        "AWS_TEMPORARY_URL": ORIGIN, "MODERATION_FIXTURE_IMAGE_ORIGIN": ORIGIN,
        "FIXTURE_ALIYUN_ID": access_id, "FIXTURE_ALIYUN_SECRET": secret, "FIXTURE_ALIYUN_TOKEN": token,
    }

    def command(current, *tokens, successful=True):
        result = subprocess.run([str(binary), "artisan", *tokens], env=current, capture_output=True,
                                text=True, timeout=25)
        assert (result.returncode == 0) == successful, result.stdout + result.stderr
        return result.stdout

    def post(version, status, disk="s3"):
        connection = http.client.HTTPConnection("127.0.0.1", app_port, timeout=10)
        try:
            connection.request("POST", "/api/drivers/moderation/aliyun-image", headers={
                "Accept": "application/json", "X-Fixture-Version": version, "X-Fixture-Disk": disk,
            })
            response = connection.getresponse()
            body = response.read()
            assert response.status == status, (status, response.status, body[:200])
            return json.loads(body)
        finally:
            connection.close()

    allowed = {"decision": "Allow", "request_id": "image-reference", "labels": []}
    try:
        assert json.loads(command(env, "drivers:moderation-aliyun", "--image", first)) == allowed
        wrong_origin = env | {"AWS_TEMPORARY_URL": "https://other-media.fixture.invalid"}
        command(wrong_origin, "drivers:moderation-aliyun", "--image", first, successful=False)
        command(env, "config:cache")
        command(env, "route:cache")
        cached = env | {"AWS_TEMPORARY_URL": "https://ignored-after-cache.fixture.invalid"}
        with (root / "http.log").open("w+") as log:
            process = ALIYUN["HTTP"]["serve"](binary, cached, log)
            try:
                assert post(first, 200) == allowed
                assert post(latest, 200) == {
                    "decision": "Block", "request_id": "image-reference",
                    "labels": [{"category": "violence", "provider": "violent_explosion",
                                "confidence": {"value": 88.5, "maximum": 100.0}}],
                }
                assert post(first, 400, disk="local")["error"].startswith("InvalidInput")
                assert post("null", 400)["error"].startswith("InvalidInput")
                supplier.response_mode = "invalid-confidence"
                assert post(latest, 502) == {"error": "Protocol"}
                supplier.response_mode = "missing-results"
                assert post(first, 502) == {"error": "Protocol"}
                supplier.response_mode = "sampled"
                incomplete = {"decision": None, "request_id": "image-reference", "labels": []}
                assert post(first, 200) == incomplete
                supplier.response_mode = "ambiguous"
                assert post(first, 200) == incomplete
                supplier.response_mode = None
                # 同样格式的未登记版本必须无法被下载，厂商拒绝后无成功结论。
                assert post("00000000-0000-0000-0000-000000000000", 400) == {"error": "Rejected"}
            finally:
                ALIYUN["HTTP"]["stop"](process)
            log.seek(0)
            recorded = log.read()
            assert "X-Amz-Signature" not in recorded and secret not in recorded and token not in recorded
        assert supplier.results == [(first, 200, original), (first, 200, original),
                                    (latest, 200, changed), (latest, 200, changed), (first, 200, original),
                                    (first, 200, original), (first, 200, original),
                                    ("00000000-0000-0000-0000-000000000000", 404, None)]
    finally:
        supplier.shutdown()
        supplier.server_close()
        thread.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-aliyun-image-") as directory, \
            TOPOLOGY["object_storage"](True) as (storage_env, storage_request):
        exercise(args.binary.resolve(), Path(directory), storage_env, storage_request)
    print("阿里云图片：真实 HTTP／Artisan、ACS3、私有 S3 指定版本与置信分通过")
