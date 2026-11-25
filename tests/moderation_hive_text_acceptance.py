"""Hive V2 Enterprise 文本替身，验证 UTF-16、项目绑定和风险证据。"""

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
from urllib.parse import parse_qs

ROOT = Path(__file__).resolve().parents[1]
HTTP = runpy.run_path(str(ROOT / "tests/webhooks_acceptance.py"))
CURRENT = "hive-private-test-token"
ROTATED = "hive-rotated-test-token"
PRIVATE = "hive-hidden-matched-value"
ORIGINAL = "中文 + & / \U0001f642"
PROJECT = 48172
CLASSES = ("hate", "spam", "violence")


class Supplier(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        self.server.unexpected.append(self.path)
        self.send_error(405)

    def do_POST(self):
        assert self.path == "/api/v2/task/sync"
        assert self.headers["Content-Type"] == "application/x-www-form-urlencoded"
        token = self.headers["Authorization"]
        assert token in {"Token " + CURRENT, "Token " + ROTATED}
        body = self.rfile.read(int(self.headers["Content-Length"])).decode()
        form = {key: values[0] for key, values in parse_qs(body, strict_parsing=True).items() if len(values) == 1}
        assert "text_data" in form and set(form) <= {"text_data", "options"}
        text = form["text_data"]
        assert len(text.encode("utf-16-le")) // 2 <= 1024
        language = json.loads(form.get("options", "{}"))
        assert language in ({}, {"input_language": "es"})
        with self.server.lock:
            self.server.calls.append((text, language, token))
        task = "fixture-task-v2"
        content = {
            "input": {"id": task, "model_type": "TEXT_CLASSIFICATION", "project_id": PROJECT, "text": PRIVATE},
            "language": language.get("input_language", "en").upper(),
            "moderated_classes": CLASSES,
            "output": [{"classes": [{"class": name, "score": 0} for name in CLASSES]}],
        }
        item = {"status": {"code": "0", "message": "SUCCESS"}, "response": content}
        result = {"id": task, "code": 200, "project_id": PROJECT, "status": [item]}
        status = {"unauthorized": 401, "rate": 429, "redirect": 302}.get(text, 200)
        if text == "risk":
            content["output"].append({"classes": [
                {"class": "hate", "score": 3}, {"class": "spam", "score": 0}, {"class": "violence", "score": 2}]})
            content["text_filters"] = [{"value": PRIVATE, "type": "profanity"}]
            content["pii_entities"] = [{"value": PRIVATE + "@example.invalid", "type": "email"}]
            content["custom_classes"] = [{"value": PRIVATE, "class": PRIVATE}]
        elif text == "unsupported":
            content["output"][0]["classes"][0]["score"] = -1
        elif text == "incomplete":
            content["output"][0]["classes"].pop()
        elif text == "bad-score":
            content["output"][0]["classes"][0]["score"] = 4
        elif text == "duplicate":
            content["output"][0]["classes"].append({"class": "hate", "score": 0})
        elif text == "wrong-language":
            content["language"] = "XX"
        elif text == "wrong-model":
            content["input"]["model_type"] = "VISUAL_CLASSIFICATION"
        elif text == "wrong-project":
            result["project_id"] = PROJECT + 1
        elif text == "wrong-task":
            content["input"]["id"] = "another-task"
        elif text == "wrong-inner-project":
            content["input"]["project_id"] = PROJECT + 1
        elif text == "rejected":
            item["status"] = {"code": "1", "message": PRIVATE}
            item["response"] = None
        payload = json.dumps(result).encode()
        self.send_response(status)
        if status == 302:
            self.send_header("Location", "/unexpected")
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def exercise(binary, work):
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    supplier = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Supplier)
    supplier.lock, supplier.calls, supplier.unexpected = threading.Lock(), [], []
    thread = threading.Thread(target=supplier.serve_forever, daemon=True)
    thread.start()
    env = os.environ | {
        "APP_BASE_PATH": str(work), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(work / "config.json"), "APP_ROUTES_CACHE": str(work / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "MODERATION_FIXTURE_HIVE": f"http://127.0.0.1:{supplier.server_port}",
        "FIXTURE_HIVE_TOKEN": CURRENT,
    }

    def artisan(current, *arguments, successful=True):
        process = subprocess.run([str(binary), "artisan", *arguments], env=current,
                                 capture_output=True, text=True, timeout=20)
        assert (process.returncode == 0) == successful, process.stdout + process.stderr
        assert PRIVATE not in process.stdout + process.stderr
        return process.stdout

    def request(text, expected=200, language=None):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            headers = {"Accept": "application/json"}
            if language:
                headers["X-Fixture-Language"] = language
            connection.request("POST", "/api/drivers/moderation/hive", text.encode(), headers)
            response = connection.getresponse()
            content = response.read()
            assert response.status == expected, (expected, response.status, content[:200])
            assert PRIVATE.encode() not in content
            return json.loads(content)
        finally:
            connection.close()

    command = "drivers:moderation-hive"
    allowed = {"decision": "Allow", "request_id": "fixture-task-v2", "labels": []}
    try:
        assert json.loads(artisan(env, command, ORIGINAL)) == allowed
        artisan(env, "config:cache")
        artisan(env, "route:cache")
        serialized = (work / "config.json").read_text()
        assert "FIXTURE_HIVE_TOKEN" in serialized and CURRENT not in serialized and ROTATED not in serialized
        cached = env | {"MODERATION_FIXTURE_HIVE": "http://127.0.0.1:1", "FIXTURE_HIVE_TOKEN": ROTATED}
        assert json.loads(artisan(cached, command, ORIGINAL)) == allowed
        artisan(cached | {"FIXTURE_HIVE_TOKEN": ""}, command, ORIGINAL, successful=False)
        with (work / "http.log").open("w+") as log:
            application = HTTP["serve"](binary, cached, log)
            try:
                assert request(ORIGINAL, language="es-MX") == allowed
                assert request("🙂" * 512) == allowed
                assert request("🙂" * 512 + "a", 400)["error"].startswith("InvalidInput")
                assert request(ORIGINAL, 400, "zh-Hant")["error"].startswith("InvalidInput")
                risk = request("risk")
                assert risk["decision"] == "Review"
                assert {label["provider"] for label in risk["labels"]} == {
                    "hate", "violence", "text_filters", "pii_entities", "custom_classes"}
                scores = {label["provider"]: label["confidence"]["value"]
                          for label in risk["labels"] if "confidence" in label}
                assert scores == {"hate": 3.0, "violence": 2.0}
                for text in ("unsupported", "incomplete"):
                    assert request(text) == allowed | {"decision": None}
                assert request("wrong-language", language="es") == allowed | {"decision": None}
                for text in ("bad-score", "duplicate", "wrong-model", "wrong-project",
                             "wrong-task", "wrong-inner-project"):
                    assert request(text, 502) == {"error": "Protocol"}
                assert request("rejected", 400) == {"error": "Rejected"}
                assert request("unauthorized", 401) == {"error": "Unauthorized"}
                assert request("rate", 429) == {"error": "Throttled"}
                assert request("redirect", 400) == {"error": "Rejected"}
            finally:
                HTTP["stop"](application)
            log.seek(0)
            traces = log.read()
            assert all(value not in traces for value in (PRIVATE, CURRENT, ROTATED, ORIGINAL))
        assert supplier.calls[:3] == [
            (ORIGINAL, {}, "Token " + CURRENT), (ORIGINAL, {}, "Token " + ROTATED),
            (ORIGINAL, {"input_language": "es"}, "Token " + ROTATED)]
        assert len(supplier.calls) == 18 and not supplier.unexpected
    finally:
        supplier.shutdown()
        supplier.server_close()
        thread.join(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-hive-") as directory:
        exercise(args.binary.resolve(), Path(directory))
    print("Hive V2 文本：HTTP／Artisan、项目绑定、UTF-16、覆盖与凭据轮换通过")
