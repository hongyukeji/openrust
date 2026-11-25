"""通过真实 Artisan 与隔离 HTTP 适配服务逐家核对翻译协议。"""
import argparse
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import threading
import time
from urllib.parse import parse_qs, urlsplit

ORIGINAL = "  SYNTHETIC_SECRET_INPUT 你好 "
TRANSLATED = "SYNTHETIC_SECRET_OUTPUT bonjour"
KEY = "SYNTHETIC_SECRET_KEY"
ERROR_BODY = {"error": "SYNTHETIC_SECRET_ECHO", "message": ORIGINAL}


@contextmanager
def server():
    cases, calls, failures = [], [], []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            raw = self.rfile.read(int(self.headers["Content-Length"]))
            index = len(calls)
            calls.append(self.path)
            if index >= len(cases):
                failures.append("unexpected extra HTTP request")
                self.send_response(500)
                self.end_headers()
                return
            case = cases[index]
            try:
                path = urlsplit(self.path)
                assert path.path == case["path"]
                assert parse_qs(path.query) == case.get("query", {})
                content_type = self.headers["Content-Type"].split(";")[0]
                assert content_type == case.get("content_type", "application/json")
                if content_type == "application/json":
                    data = json.loads(raw)
                else:
                    data = {key: items[0] for key, items in parse_qs(raw.decode()).items()}
                if "check" in case:
                    case["check"](self.headers, data)
                else:
                    assert data == case["request"], (data, case["request"])
                    assert self.headers.get("Authorization") == case.get("auth")
            except Exception as error:
                failures.append(repr(error))
            if case.get("delay"):
                time.sleep(case["delay"])
            self.send_response(case.get("status", 200))
            self.send_header("Content-Type", "application/json")
            self.send_header("X-Unsafe-Echo", "SYNTHETIC_SECRET_HEADER")
            if case.get("status") == 307:
                self.send_header("Location", f"http://127.0.0.1:{self.server.server_port}/never")
            self.end_headers()
            try:
                self.wfile.write(json.dumps(case.get("response", ERROR_BODY)).encode())
            except (BrokenPipeError, ConnectionResetError):
                pass

    http = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=http.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{http.server_port}", cases, calls, failures
    finally:
        http.shutdown()
        http.server_close()
        thread.join()


def runner(binary, stage):
    env = os.environ | {
        "APP_BASE_PATH": str(stage), "APP_ENV": "production", "APP_DEBUG": "false",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "CACHE_STORE": "array", "SESSION_DRIVER": "array", "LOG_CHANNEL": "null",
        "APP_CONFIG_CACHE": str(stage / "config.json"), "TRANSLATE_TEST_KEY": KEY,
        "TRANSLATE_TEST_OBSERVATIONS": str(stage / "observations.log"),
        "TRANSLATE_DRIVER": "protocol",
    }
    env.pop("TRANSLATE_TEST_LABEL", None)
    env.pop("DB_URL", None)

    def artisan(profile, *arguments, error=None, extra=None):
        result = subprocess.run(
            [str(binary), "artisan", "--no-ansi", *arguments], cwd=stage, text=True,
            env=env | {"TRANSLATE_TEST_PROFILES": json.dumps({"protocol": profile})} | (extra or {}),
            capture_output=True, timeout=12,
        )
        if error is None:
            assert result.returncode == 0, (arguments[:1], result.stderr)
        else:
            diagnostic = result.stdout + result.stderr
            assert result.returncode == 1 and error in diagnostic, diagnostic
            assert "SYNTHETIC_SECRET" not in diagnostic
        return result.stdout

    def events():
        path = stage / "observations.log"
        return path.read_text().splitlines() if path.exists() else []

    return artisan, events


def round_trip(artisan, events, profile, cases, calls, faults, scenario, *, target, source=None,
               outcome=None, error=None, expected_observations=None):
    before = len(events())
    count = len(calls)
    if scenario is not None:
        cases.append(scenario)
    arguments = ["translate:request", ORIGINAL, target]
    if source is not None:
        arguments.append(source)
    stdout = artisan(profile, *arguments, error=error)
    if outcome is not None:
        assert json.loads(stdout) == outcome, stdout
    assert len(calls) == count + (scenario is not None), calls
    assert not faults, faults
    if expected_observations is None:
        expected_observations = ["request", "response"] if scenario is not None else []
    assert events()[before:] == expected_observations, events()[before:]


def batch_a(binary, stage):
    artisan, events = runner(binary, stage)

    with server() as (base, cases, calls, faults):
        def submit(profile, scenario, **options):
            round_trip(artisan, events, profile, cases, calls, faults, scenario, **options)

        deep = {"driver": "deepl", "plan": "free", "base_url": base + "/deep",
                "api_key_env": "TRANSLATE_TEST_KEY", "timeout_ms": 3000}
        deep_auto = {
            "path": "/deep/v2/translate",
            "auth": f"DeepL-Auth-Key {KEY}",
            "request": {"text": [ORIGINAL], "target_lang": "EN-GB"},
            "response": {"translations": [{"text": TRANSLATED, "detected_source_language": "FR"}]},
        }
        submit(deep, deep_auto, target="en-GB", outcome={"text": TRANSLATED, "source": "fr"})
        deep_pro = deep | {"plan": "pro", "base_url": base + "/pro"}
        submit(deep_pro, deep_auto | {
            "path": "/pro/v2/translate",
            "request": {"text": [ORIGINAL], "target_lang": "ZH-HANT", "source_lang": "FR"},
        }, target="zh-Hant", source="fr", outcome={"text": TRANSLATED, "source": "fr"})
        submit(deep, None, target="en-AU", error="UnsupportedLanguage")
        submit(deep | {"base_url": "http://example.test"}, None, target="en-GB",
               error="InvalidConfiguration")
        submit(deep, deep_auto | {"response": {"translations": []}},
               target="en-GB", error="InvalidResponse")
        for code, category in [(400, "Rejected"), (403, "Authentication"), (456, "Quota"),
                               (429, "RateLimited"), (500, "Service"), (307, "Service")]:
            submit(deep, deep_auto | {"status": code, "response": ERROR_BODY},
                   target="en-GB", error=category)
        submit(deep | {"timeout_ms": 140}, deep_auto | {"delay": .45}, target="en-GB",
               error="Timeout", expected_observations=["request", "failure"])
        print("DeepL: auth, Free/Pro mapping, source detection, errors, timeout, single send and redaction PASS")

        libre = {"driver": "libretranslate", "base_url": base + "/libre",
                 "api_key_env": "TRANSLATE_TEST_KEY", "timeout_ms": 3000}
        libre_auto = {
            "path": "/libre/translate",
            "request": {"q": ORIGINAL, "source": "auto", "target": "zt", "format": "text", "api_key": KEY},
            "response": {"translatedText": TRANSLATED,
                         "detectedLanguage": {"language": "zh", "confidence": 99}},
        }
        submit(libre, libre_auto, target="zh-Hant", outcome={"text": TRANSLATED, "source": "zh-Hans"})
        anonymous = {key: value for key, value in libre.items() if key != "api_key_env"}
        submit(anonymous, libre_auto | {
            "request": {"q": ORIGINAL, "source": "fr", "target": "pb", "format": "text"},
            "response": {"translatedText": TRANSLATED},
        }, target="pt-BR", source="fr", outcome={"text": TRANSLATED, "source": None})
        submit(libre, None, target="pt-AO", error="UnsupportedLanguage")
        submit(libre, libre_auto | {"response": {"translatedText": []}},
               target="zh-Hant", error="InvalidResponse")
        for code, category in [(400, "Rejected"), (403, "Authentication"),
                               (429, "RateLimited"), (500, "Service"), (307, "Service")]:
            submit(libre, libre_auto | {"status": code, "response": ERROR_BODY},
                   target="zh-Hant", error=category)
        submit(libre | {"timeout_ms": 140}, libre_auto | {"delay": .45}, target="zh-Hant",
               error="Timeout", expected_observations=["request", "failure"])

        artisan(libre, "config:cache")
        cached = (stage / "config.json").read_text()
        assert "TRANSLATE_TEST_KEY" in cached and "SYNTHETIC_SECRET" not in cached
        submit({"driver": "unknown"}, libre_auto, target="zh-Hant",
               outcome={"text": TRANSLATED, "source": "zh-Hans"})
        assert len(calls) == len(cases)
        print("LibreTranslate: optional key, auto and explicit source, script mapping, errors, timeout, single send and redaction PASS")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--stage", required=True, type=Path)
    args = parser.parse_args()
    stage = args.stage.resolve()
    project = Path(__file__).resolve().parents[1]
    assert stage.is_relative_to(project / "target/tmp")
    stage.mkdir(parents=True, exist_ok=False)
    batch_a(args.binary.resolve(), stage)
