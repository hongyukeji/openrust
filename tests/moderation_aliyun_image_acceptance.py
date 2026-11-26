"""阿里云图片替身签名验收，实际读取同一私有 S3 对象的不同版本。"""

import argparse
from contextlib import contextmanager
import http.client
import http.server
import json
import os
from pathlib import Path
import runpy
import socket
import ssl
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


@contextmanager
def storage_fixture(media_ca):
    if media_ca is None:
        with TOPOLOGY["object_storage"](True) as fixture:
            yield fixture
        return
    keys = ("AWS_ENDPOINT", "AWS_BUCKET", "AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY")
    env = {key: os.environ[key] for key in keys}
    env.update(AWS_SESSION_TOKEN="", AWS_DEFAULT_REGION="us-east-1",
               AWS_USE_PATH_STYLE_ENDPOINT="true")
    request = TOPOLOGY["s3_requests"](*(env[key] for key in keys))
    request("PUT", content=b"")
    yield env, request


def configure_media(server, storage_env, versions, media_ca):
    server.storage = urlsplit(storage_env["AWS_ENDPOINT"])
    server.media_versions = versions
    server.media_tls = ssl.create_default_context(cafile=media_ca) if media_ca else None
    server.tls_verified = False


def download_media(server, source):
    def get(connection, path, headers=None):
        try:
            connection.request("GET", path, headers=headers or {})
            response = connection.getresponse()
            return response.status, response.read()
        finally:
            connection.close()

    target = source.path + "?" + source.query
    if getattr(server, "media_tls", None) is None:
        backend = server.storage
        return get(http.client.HTTPConnection(backend.hostname, backend.port, timeout=5),
                   target, {"Host": backend.netloc})

    def https(context):
        return http.client.HTTPSConnection(source.hostname, source.port, context=context, timeout=5)

    with server.lock:
        if not server.tls_verified:
            try:
                get(https(ssl.create_default_context()), target)
            except ssl.SSLCertVerificationError:
                pass
            else:
                raise AssertionError("私有 CA 未配置时仍可下载")
            assert get(https(server.media_tls), source.path)[0] == 403
            version, = parse_qs(source.query)["versionId"]
            assert version in server.media_versions
            alternate = next(value for value in server.media_versions if value != version)
            replaced = target.replace(f"versionId={version}", f"versionId={alternate}", 1)
            assert replaced != target and get(https(server.media_tls), replaced)[0] == 403
            signature, = parse_qs(source.query)["X-Amz-Signature"]
            forged = ("0" if signature[0] != "0" else "1") + signature[1:]
            assert get(https(server.media_tls), target.replace(signature, forged, 1))[0] == 403
            status, payload = get(https(server.media_tls), target)
            assert status == 200 and len(payload) >= 16
            assert get(https(server.media_tls), target, {"Range": "bytes=0-15"}) == (206, payload[:16])
            server.tls_verified = True
            return status, payload
    return get(https(server.media_tls), target)


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

        status, payload = download_media(self.server, public)
        with self.server.lock:
            self.server.results.append((version, status, payload if status == 200 else None))
        if status == 200:
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
            assert status == 404
            envelope = {"Code": 400, "RequestId": "missing-image"}
        body = json.dumps(envelope).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def exercise(binary, root, storage_env, storage_request, *, media_ca=None):
    storage_request("PUT", query="versioning=", content=(
        b'<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">'
        b'<Status>Enabled</Status></VersioningConfiguration>'
    ))
    original, changed = png(0, 123, 42), png(205, 4, 7)
    storage_request("PUT", KEY, content=original)
    storage_request("PUT", KEY, content=changed)
    version_document = XML.fromstring(storage_request("GET", query="versions="))
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
    configure_media(supplier, storage_env, (first, latest), media_ca)
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
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-aliyun-image-") as directory, \
            storage_fixture(media_ca) as (storage_env, storage_request):
        exercise(args.binary.resolve(), Path(directory), storage_env, storage_request, media_ca=media_ca)
    print("阿里云图片：真实 HTTP／Artisan、ACS3、私有 S3 指定版本与置信分通过")
