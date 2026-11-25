"""腾讯云文本审核具名消费入口及独立 TC3 验签。"""

import argparse
import base64
from datetime import datetime, timezone
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


ROOT = Path(__file__).resolve().parents[1]
HTTP = runpy.run_path(str(ROOT / "tests/webhooks_acceptance.py"))
TEXT = "原文、ASCII + / 🌍"
PRIVATE_MARKER = "fixture-keyword-must-stay-private"
CREDENTIALS = {
    "fixture-tencent-id": ("fixture-tencent-secret", "fixture-tencent-token"),
    "rotated-tencent-id": ("rotated-tencent-secret", "rotated-tencent-token"),
}


class Supplier(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        self.server.unexpected.append(self.path)
        self.send_error(405)

    def do_POST(self):
        assert self.path == "/"
        body = self.rfile.read(int(self.headers["Content-Length"]))
        media = "application/json; charset=utf-8"
        assert self.headers["Content-Type"] == media
        assert self.headers["X-TC-Action"] == "TextModeration"
        assert self.headers["X-TC-Version"] == "2020-12-29"
        assert self.headers["X-TC-Region"] == "ap-guangzhou"
        timestamp = int(self.headers["X-TC-Timestamp"])
        day = datetime.fromtimestamp(timestamp, timezone.utc).strftime("%Y-%m-%d")
        auth = re.fullmatch(
            r"TC3-HMAC-SHA256 Credential=([^/]+)/([^,]+), SignedHeaders=content-type;host, Signature=([a-f0-9]{64})",
            self.headers["Authorization"],
        )
        assert auth
        access_id, scope, signature = auth.groups()
        secret, token = CREDENTIALS[access_id]
        assert self.headers["X-TC-Token"] == token
        assert scope == f"{day}/tms/tc3_request"
        canonical = (f"POST\n/\n\ncontent-type:{media}\nhost:{self.headers['Host']}\n\ncontent-type;host\n"
                     + hashlib.sha256(body).hexdigest())
        to_sign = f"TC3-HMAC-SHA256\n{timestamp}\n{scope}\n" + hashlib.sha256(canonical.encode()).hexdigest()
        key_date = hmac.digest(("TC3" + secret).encode(), day.encode(), "sha256")
        key_service = hmac.digest(key_date, b"tms", "sha256")
        key_signing = hmac.digest(key_service, b"tc3_request", "sha256")
        assert hmac.compare_digest(signature, hmac.new(key_signing, to_sign.encode(), hashlib.sha256).hexdigest())

        parameters = json.loads(body)
        assert set(parameters) <= {"Content", "BizType", "Type", "SourceLanguage"}
        assert parameters["Type"] == "TEXT" and parameters["BizType"] == "comments"
        text = base64.b64decode(parameters["Content"], validate=True).decode()
        language = parameters.get("SourceLanguage")
        with self.server.lock:
            self.server.calls.append((text, language, access_id))
        output = {"RequestId":"tms-reference", "BizType":"comments", "Label":"Normal", "Suggestion":"Pass",
                  "SubLabel":"", "Score":0, "DetailResults":None}
        status = {"unauthorized":403, "rate":429, "unavailable":503, "redirect":302}.get(text, 200)
        if text == "detail":
            output["DetailResults"] = [
                {"Label":"Ad", "Suggestion":"Pass", "Score":0},
                {"Label":"Illegal", "SubLabel":"Drug", "Suggestion":"Block", "Score":87,
                 "Keywords":[PRIVATE_MARKER]},
            ]
        if text == "unknown-label":
            output.update({"Label":"NewRisk", "Score":12})
        if text == "bad-score":
            output["Score"] = 101
        if text == "wrong-policy":
            output["BizType"] = "other"
        if text == "missing":
            del output["Suggestion"]
        if text == "unknown-verdict":
            output["Suggestion"] = "FutureVerdict"
        if text == "auth-error":
            output = {"Error":{"Code":"AuthFailure.SignatureFailure", "Message":PRIVATE_MARKER}}
        if text == "limit-error":
            output = {"Error":{"Code":"RequestLimitExceeded", "Message":PRIVATE_MARKER}}
        if text == "timeout":
            time.sleep(.65)
        payload = b"invalid" if text == "bad-json" else json.dumps({"Response": output}).encode()
        try:
            self.send_response(status)
            if text == "redirect":
                self.send_header("Location", "/unexpected")
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
        except (BrokenPipeError, ConnectionResetError):
            if text != "timeout":
                raise


def exercise(binary, root):
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Supplier)
    supplier.lock, supplier.calls, supplier.unexpected = threading.Lock(), [], []
    thread = threading.Thread(target=supplier.serve_forever, daemon=True)
    thread.start()
    initial, rotated = CREDENTIALS

    def credentials(access_id):
        secret, token = CREDENTIALS[access_id]
        return {"FIXTURE_TENCENT_ID":access_id, "FIXTURE_TENCENT_SECRET":secret, "FIXTURE_TENCENT_TOKEN":token}

    env = os.environ | {
        "APP_BASE_PATH": str(root), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "MODERATION_FIXTURE_TENCENT": f"http://127.0.0.1:{supplier.server_port}",
    } | credentials(initial)

    def command(current, *tokens, successful=True):
        result = subprocess.run([str(binary), "artisan", *tokens], env=current, capture_output=True,
                                text=True, timeout=20)
        assert (result.returncode == 0) == successful, result.stdout + result.stderr
        return result.stdout

    def request(text, status, *, language=None, profile=None):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            headers = {"Accept": "application/json"}
            if language:
                headers["X-Fixture-Language"] = language
            if profile:
                headers["X-Fixture-Profile"] = profile
            connection.request("POST", "/api/drivers/moderation/tencent", text.encode(), headers)
            response = connection.getresponse()
            data = response.read()
            assert response.status == status, (status, response.status, data[:200])
            assert PRIVATE_MARKER.encode() not in data
            return json.loads(data)
        finally:
            connection.close()

    clean = {"decision":"Allow", "request_id":"tms-reference", "labels":[]}
    try:
        assert json.loads(command(env, "drivers:moderation-tencent", TEXT)) == clean
        command(env, "config:cache")
        command(env, "route:cache")
        serialized = (root / "config.json").read_text()
        assert "FIXTURE_TENCENT_SECRET" in serialized
        for identifier, (secret, token) in CREDENTIALS.items():
            assert all(value not in serialized for value in (identifier, secret, token))
        cached = env | {"MODERATION_FIXTURE_TENCENT":"http://127.0.0.1:1"} | credentials(rotated)
        assert json.loads(command(cached, "drivers:moderation-tencent", TEXT)) == clean
        command(cached | {"FIXTURE_TENCENT_SECRET":""}, "drivers:moderation-tencent", TEXT, successful=False)
        with (root / "http.log").open("w+") as log:
            process = HTTP["serve"](binary, cached, log)
            try:
                assert request(TEXT, 200, language="zh-Hant") == clean
                assert request("English", 400, language="en")["error"].startswith("InvalidInput")
                assert request("English", 200, language="en-US", profile="tencent-english") == clean
                assert request("français", 400, language="fr")["error"].startswith("InvalidInput")
                assert request("🙂" * 10_000, 200) == clean
                assert request("🙂" * 10_001, 400)["error"].startswith("InvalidInput")
                assert request("detail", 200) == {
                    "decision":"Block", "request_id":"tms-reference", "labels":[
                        {"category":"contraband", "provider":"Illegal/Drug", "confidence":{"value":87.0,"maximum":100.0}},
                    ],
                }
                assert request("unknown-label", 200)["decision"] == "Review"
                for text, status, error in (
                    ("unauthorized", 401, "Unauthorized"), ("auth-error", 401, "Unauthorized"),
                    ("rate", 429, "Throttled"), ("limit-error", 429, "Throttled"),
                    ("unavailable", 502, "ServiceUnavailable"), ("redirect", 400, "Rejected"),
                    ("bad-json", 502, "Protocol"), ("missing", 502, "Protocol"),
                    ("bad-score", 502, "Protocol"), ("wrong-policy", 502, "Protocol"),
                    ("unknown-verdict", 502, "Protocol"), ("timeout", 504, "Timeout"),
                ):
                    assert request(text, status) == {"error": error}
            finally:
                HTTP["stop"](process)
            log.seek(0)
            recorded = log.read()
            assert PRIVATE_MARKER not in recorded and TEXT not in recorded
            for identifier, (secret, token) in CREDENTIALS.items():
                assert all(value not in recorded for value in (identifier, secret, token))
        assert supplier.calls[:4] == [(TEXT, None, initial), (TEXT, None, rotated),
                                     (TEXT, "zh", rotated), ("English", "en", rotated)]
        assert len(supplier.calls) == 19 and supplier.unexpected == []
    finally:
        supplier.shutdown()
        supplier.server_close()
        thread.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-tencent-text-") as directory:
        exercise(args.binary.resolve(), Path(directory))
    print("腾讯云文本：真实 HTTP／Artisan、TC3、具名语言能力、结果与缓存凭据通过")
