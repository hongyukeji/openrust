"""Sightengine v1 文本规则与 ML 的本地供应商协议验收。"""

import argparse
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
import time
from urllib.parse import parse_qs

ROOT = Path(__file__).resolve().parents[1]
HTTP = runpy.run_path(str(ROOT / "tests/webhooks_acceptance.py"))
SECRET = "sightengine-local-only-secret+/"
ROTATED = "sightengine-rotated-only-secret+/"
PRIVATE = "provider-response-private-matched-word"
ORIGINAL = "测试原文 + & / 🙂"
GENERAL_CLASSES = ["sexual", "discriminatory", "insulting", "violent", "toxic"]


class Supplier(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        self.server.unexpected.append(self.path)
        self.send_error(405)

    def do_POST(self):
        assert self.path == "/1.0/text/check.json"
        assert self.headers["Content-Type"] == "application/x-www-form-urlencoded"
        raw = self.rfile.read(int(self.headers["Content-Length"])).decode()
        form = {key: values[0] for key, values in parse_qs(raw, strict_parsing=True).items() if len(values) == 1}
        assert form.pop("api_user") == "local-user"
        secret = form.pop("api_secret")
        assert secret in {SECRET, ROTATED}
        text, mode, lang = (form.pop(name) for name in ("text", "mode", "lang"))
        assert len(text) <= 10_000
        with self.server.lock:
            self.server.calls.append((text, mode, lang, secret))
        result = {"status": "success", "request": {"id": "sightengine-reference"}}
        status = {"unauthorized": 401, "rate": 429, "server": 503, "redirect": 302}.get(text, 200)
        if mode == "rules":
            assert form == {"categories": "profanity,personal"}
            assert lang in {"en", "zh", "en,zh"}
            result |= {"profanity": {"matches": []}, "personal": {"matches": []}}
            if text == "hit":
                result["profanity"]["matches"].append({"type": "insult", "match": PRIVATE, "intensity": "low"})
            elif text == "partial":
                result.pop("personal")
            elif text == "malformed":
                result["personal"] = {"matches": False}
            elif text == "invalid-label":
                result["personal"]["matches"].append({"type": PRIVATE + " as content"})
        else:
            assert mode == "ml" and form == {"models": "general,self-harm"} and lang == "en"
            classes = GENERAL_CLASSES + ["self-harm"]
            result["moderation_classes"] = {"available": classes, **dict.fromkeys(classes, .01)}
            if text == "hit":
                result["moderation_classes"]["toxic"] = .8
            elif text == "partial":
                del result["moderation_classes"]["toxic"]
            elif text == "bad-score":
                result["moderation_classes"]["toxic"] = 1.1
            elif text == "unknown-class":
                result["moderation_classes"]["available"].append("new")
                result["moderation_classes"]["new"] = .6
        if text == "timeout":
            time.sleep(.65)
        if status != 200 or text == "failed-success":
            result = {"status": "failure", "request": {"id": "failure"}, "error": {"message": PRIVATE}}
        body = json.dumps(result).encode()
        try:
            self.send_response(status)
            if status == 302:
                self.send_header("Location", "/leak")
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            assert text == "timeout"


def exercise(binary, directory):
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Supplier)
    supplier.lock, supplier.calls, supplier.unexpected = threading.Lock(), [], []
    thread = threading.Thread(target=supplier.serve_forever, daemon=True)
    thread.start()
    env = os.environ | {
        "APP_BASE_PATH": str(directory), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(directory / "config.json"), "APP_ROUTES_CACHE": str(directory / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "MODERATION_FIXTURE_SIGHTENGINE": f"http://127.0.0.1:{supplier.server_port}",
        "FIXTURE_SIGHTENGINE_USER": "local-user", "FIXTURE_SIGHTENGINE_SECRET": SECRET,
    }

    def artisan(current, *arguments, successful=True):
        process = subprocess.run([str(binary), "artisan", *arguments], env=current,
                                 capture_output=True, text=True, timeout=20)
        assert (process.returncode == 0) == successful, process.stdout + process.stderr
        return process.stdout

    def request(text, expect=200, *, language=None, profile=None):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            headers = {"Accept": "application/json"}
            if language:
                headers["X-Fixture-Language"] = language
            if profile:
                headers["X-Fixture-Profile"] = profile
            connection.request("POST", "/api/drivers/moderation/sightengine", text.encode(), headers)
            response = connection.getresponse()
            data = response.read()
            assert response.status == expect, (expect, response.status, data[:200])
            assert PRIVATE.encode() not in data
            return json.loads(data)
        finally:
            connection.close()

    command = "drivers:moderation-sightengine"
    clean = {"decision": "Allow", "request_id": "sightengine-reference", "labels": []}
    try:
        assert json.loads(artisan(env, command, ORIGINAL)) == clean
        artisan(env, "config:cache")
        artisan(env, "route:cache")
        serialized = (directory / "config.json").read_text()
        assert "FIXTURE_SIGHTENGINE_SECRET" in serialized and SECRET not in serialized and ROTATED not in serialized
        cached = env | {"MODERATION_FIXTURE_SIGHTENGINE": "http://127.0.0.1:1", "FIXTURE_SIGHTENGINE_SECRET": ROTATED}
        assert json.loads(artisan(cached, command, ORIGINAL)) == clean
        artisan(cached | {"FIXTURE_SIGHTENGINE_SECRET": ""}, command, ORIGINAL, successful=False)
        with (directory / "http.log").open("w+") as log:
            application = HTTP["serve"](binary, cached, log)
            try:
                assert request(ORIGINAL, language="zh-Hant") == clean
                assert request("🙂" * 10_000) == clean
                assert request("🙂" * 10_001, 400)["error"].startswith("InvalidInput")
                assert request(ORIGINAL, 400, language="ru")["error"].startswith("InvalidInput")
                assert request("hit") == clean | {
                    "decision": "Review", "labels": [{"category": "profanity", "provider": "profanity/insult"}]}
                assert request("partial")["decision"] is None
                for text, status, error in (
                    ("malformed", 502, "Protocol"), ("invalid-label", 502, "Protocol"),
                    ("unauthorized", 401, "Unauthorized"), ("rate", 429, "Throttled"),
                    ("server", 502, "ServiceUnavailable"), ("redirect", 400, "Rejected"),
                    ("timeout", 504, "Timeout"), ("failed-success", 502, "Protocol"),
                ):
                    assert request(text, status) == {"error": error}
                ml = {"profile": "sightengine-ml"}
                assert request("hello", language="en-US", **ml) == clean
                assert request(ORIGINAL, 400, language="zh-Hans", **ml)["error"].startswith("InvalidInput")
                assert request("hit", **ml) == clean | {"decision": "Review", "labels": [
                    {"category": "harassment", "provider": "toxic", "confidence": {"value": .8, "maximum": 1.0}}]}
                assert request("partial", **ml)["decision"] is None
                assert request("unknown-class", **ml)["decision"] is None
                assert request("bad-score", 502, **ml) == {"error": "Protocol"}
                assert request("hello", 502, profile="sightengine-bad-ml")["error"].startswith("InvalidConfiguration")
            finally:
                HTTP["stop"](application)
            log.seek(0)
            trace = log.read()
            assert all(value not in trace for value in (PRIVATE, SECRET, ROTATED, ORIGINAL))
        assert supplier.calls[:3] == [(ORIGINAL, "rules", "en,zh", SECRET),
                                      (ORIGINAL, "rules", "en,zh", ROTATED),
                                      (ORIGINAL, "rules", "zh", ROTATED)]
        assert len(supplier.calls) == 19 and not supplier.unexpected
    finally:
        supplier.shutdown()
        supplier.server_close()
        thread.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-sightengine-") as directory:
        exercise(args.binary.resolve(), Path(directory))
    print("Sightengine 文本：实际 HTTP／Artisan、规则／ML、缓存、UTF-8 与部分覆盖通过")
