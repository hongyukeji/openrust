"""证实具名阿里云文本驱动实际出站 ACS3 规格、错误和缓存后的凭据装配。"""

import argparse
import hashlib
import hmac
import http.client
import http.server
import json
import os
from pathlib import Path
import re
import runpy
import socket
import subprocess
import tempfile
import threading
import time
from urllib.parse import parse_qs


ROOT = Path(__file__).resolve().parents[1]
HTTP = runpy.run_path(str(ROOT / "tests/webhooks_acceptance.py"))
BASE = "原文 ＋ / ~ 🙂"
CREDENTIALS = {
    "initial-access-identity": ("initial-isolated-key", "initial-isolated-token"),
    "rotated-access-identity": ("rotated-isolated-key", "rotated-isolated-token"),
}


class Supplier(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        self.server.unexpected.append(self.path)
        self.send_error(405)

    def verification(self, action, service):
        assert self.path == "/"
        body = self.rfile.read(int(self.headers["Content-Length"]))
        assert self.headers["Content-Type"] == "application/x-www-form-urlencoded"
        digest = hashlib.sha256(body).hexdigest()
        assert self.headers["x-acs-content-sha256"] == digest
        assert self.headers["x-acs-action"] == action
        assert self.headers["x-acs-version"] == "2022-03-02"
        assert re.fullmatch(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ", self.headers["x-acs-date"])
        nonce = self.headers["x-acs-signature-nonce"]
        auth = re.fullmatch(
            r"ACS3-HMAC-SHA256 Credential=([^,]+),SignedHeaders=([^,]+),Signature=([a-f0-9]{64})",
            self.headers["Authorization"],
        )
        assert auth
        access_id, signed, signature = auth.groups()
        secret, token = CREDENTIALS[access_id]
        assert self.headers["x-acs-security-token"] == token
        names = signed.split(";")
        assert names == ["content-type", "host", "x-acs-action", "x-acs-content-sha256", "x-acs-date",
                         "x-acs-security-token", "x-acs-signature-nonce", "x-acs-version"]
        canonical = "POST\n/\n\n" + "".join(f"{name}:{self.headers[name]}\n" for name in names)
        canonical += f"\n{signed}\n{digest}"
        to_sign = "ACS3-HMAC-SHA256\n" + hashlib.sha256(canonical.encode()).hexdigest()
        assert hmac.compare_digest(signature, hmac.new(secret.encode(), to_sign.encode(), hashlib.sha256).hexdigest())

        form = parse_qs(body.decode(), strict_parsing=True)
        assert set(form) == {"Service", "ServiceParameters"} and form["Service"] == [service]
        parameters = json.loads(form["ServiceParameters"][0])
        with self.server.lock:
            assert nonce not in self.server.nonces
            self.server.nonces.add(nonce)
        return parameters, access_id

    def do_POST(self):
        parameters, access_id = self.verification("TextModeration", "comment_detection")
        assert set(parameters) == {"content"}
        text = parameters["content"]
        with self.server.lock:
            self.server.calls.append((text, access_id))

        status = {"unauthorized": 403, "rate": 429, "unavailable": 503, "redirect": 302}.get(text, 200)
        code = {"business-permission": 408, "quota": 588, "business-timeout": 581}.get(text, 200)
        data = {"Labels": ""}
        if text == "risk":
            data = {"Labels": "sexual_content, future_label", "Reason": '{"riskLevel":"high"}'}
        if text == "low":
            data = {"labels": "ad", "reason": '{"riskLevel":"low"}'}
        if text == "incomplete-reason":
            data = {"Labels": "", "Reason": '{"riskTips":"fixture"}'}
        if text == "missing":
            data = {}
        if text == "timeout":
            time.sleep(.65)
        response = json.dumps({"Code": code, "Data": data, "RequestId": "job-reference"}).encode()
        if text == "broken":
            response = b"broken json"
        if text == "oversized":
            response = b" " * (1024 * 1024 + 1)
        if text == "bad-reason":
            response = b'{"Code":200,"Data":{"Labels":"","Reason":"invalid"},"RequestId":"req"}'
        try:
            self.send_response(status)
            if text == "redirect":
                self.send_header("Location", "/unexpected")
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(response)))
            self.end_headers()
            self.wfile.write(response)
        except (BrokenPipeError, ConnectionResetError):
            if text not in ("timeout", "oversized"):
                raise


def exercise(binary, root):
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Supplier)
    supplier.lock, supplier.calls, supplier.nonces, supplier.unexpected = threading.Lock(), [], set(), []
    thread = threading.Thread(target=supplier.serve_forever, daemon=True)
    thread.start()
    initial, rotated = CREDENTIALS

    def credentials(access_id):
        secret, token = CREDENTIALS[access_id]
        return {"FIXTURE_ALIYUN_ID":access_id, "FIXTURE_ALIYUN_SECRET":secret, "FIXTURE_ALIYUN_TOKEN":token}

    env = os.environ | {
        "APP_BASE_PATH": str(root), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "MODERATION_FIXTURE_ALIYUN": f"http://127.0.0.1:{supplier.server_port}",
    } | credentials(initial)

    def command(current, *tokens, successful=True):
        result = subprocess.run([str(binary), "artisan", *tokens], env=current, capture_output=True, text=True, timeout=20)
        assert (result.returncode == 0) == successful, result.stdout + result.stderr
        return result.stdout

    def request(text, status, language=None):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            headers = {"Accept": "application/json"}
            if language:
                headers["X-Fixture-Language"] = language
            connection.request("POST", "/api/drivers/moderation/aliyun", text.encode(), headers)
            response = connection.getresponse()
            data = response.read()
            assert response.status == status, (status, response.status, data[:200])
            return json.loads(data)
        finally:
            connection.close()

    clean = {"decision":"Allow", "request_id":"job-reference", "labels":[]}
    try:
        assert json.loads(command(env, "drivers:moderation-aliyun", BASE)) == clean
        command(env, "config:cache")
        command(env, "route:cache")
        serialized = (root / "config.json").read_text()
        assert "FIXTURE_ALIYUN_SECRET" in serialized
        for identifier, (secret, token) in CREDENTIALS.items():
            assert all(value not in serialized for value in (identifier, secret, token))
        cached = env | {"MODERATION_FIXTURE_ALIYUN":"http://127.0.0.1:1"} | credentials(rotated)
        assert json.loads(command(cached, "drivers:moderation-aliyun", BASE)) == clean
        command(cached | {"FIXTURE_ALIYUN_SECRET":""}, "drivers:moderation-aliyun", BASE, successful=False)
        with (root / "http.log").open("w+") as log:
            process = HTTP["serve"](binary, cached, log)
            try:
                assert request(BASE, 200) == clean
                assert request(BASE, 200, "zh-hans") == clean
                assert request("langage hors profil", 400, "fr")["error"].startswith("InvalidInput")
                assert request("🙂" * 600, 200) == clean
                assert request("🙂" * 601, 400)["error"].startswith("InvalidInput")
                assert request("risk", 200) == {"decision":"Block", "request_id":"job-reference", "labels":[
                    {"category":"sexual", "provider":"sexual_content"}, {"category":"unknown", "provider":"future_label"},
                ]}
                assert request("low", 200)["decision"] == "Review"
                assert request("incomplete-reason", 200)["decision"] == "Review"
                for text, status, error in (
                    ("unauthorized", 401, "Unauthorized"), ("business-permission", 401, "Unauthorized"),
                    ("rate", 429, "Throttled"), ("quota", 429, "Throttled"),
                    ("unavailable", 502, "ServiceUnavailable"), ("redirect", 400, "Rejected"),
                    ("broken", 502, "Protocol"), ("missing", 502, "Protocol"),
                    ("bad-reason", 502, "Protocol"), ("oversized", 502, "Protocol"),
                    ("timeout", 504, "Timeout"), ("business-timeout", 504, "Timeout"),
                ):
                    assert request(text, status) == {"error": error}
            finally:
                HTTP["stop"](process)
            log.seek(0)
            output = log.read()
            assert BASE not in output
            for identifier, (secret, token) in CREDENTIALS.items():
                assert all(value not in output for value in (identifier, secret, token))
        assert supplier.calls[:3] == [(BASE, initial), (BASE, rotated), (BASE, rotated)]
        assert len(supplier.calls) == 20 and supplier.unexpected == []
    finally:
        supplier.shutdown()
        supplier.server_close()
        thread.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-aliyun-") as directory:
        exercise(args.binary.resolve(), Path(directory))
    print("阿里云文本：真实 HTTP／Artisan、ACS3 独立验签、凭据轮换、映射和失败边界通过")
