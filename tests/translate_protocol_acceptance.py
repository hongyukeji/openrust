"""通过真实 Artisan 与隔离 HTTP 适配服务逐家核对翻译协议。"""
import argparse
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import hashlib
import hmac
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
                    case["check"](self.headers, data, raw)
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
        "TRANSLATE_TEST_AK": "SYNTHETIC_SECRET_ACCOUNT",
        "TRANSLATE_TEST_STS": "SYNTHETIC_SECRET_STS",
        "TRANSLATE_TEST_APP_ID": "2015063000000001",
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
    network = scenario if isinstance(scenario, list) else [scenario] if scenario is not None else []
    cases.extend(network)
    arguments = ["translate:request", ORIGINAL, target]
    if source is not None:
        arguments.append(source)
    stdout = artisan(profile, *arguments, error=error)
    if outcome is not None:
        assert json.loads(stdout) == outcome, stdout
    assert len(calls) == count + len(network), calls
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


def batch_b(binary, stage):
    artisan, events = runner(binary, stage)

    with server() as (base, cases, calls, faults):
        def submit(profile, scenario, **options):
            round_trip(artisan, events, profile, cases, calls, faults, scenario, **options)

        tencent = {"driver": "tencent", "model": "hy-mt3-plus", "base_url": base + "/tencent",
                   "api_key_env": "TRANSLATE_TEST_KEY", "timeout_ms": 3000}
        tencent_auto = {
            "path": "/tencent/v1/api/translations",
            "auth": f"Bearer {KEY}",
            "request": {"text": ORIGINAL, "model": "hy-mt3-plus", "target": "zh-TR", "stream": False},
            "response": {
                "choices": [{"finish_reason": "stop", "message": {"role": "assistant", "content": TRANSLATED}}],
                "source": "zh",
            },
        }
        submit(tencent, tencent_auto, target="zh-Hant",
               outcome={"text": TRANSLATED, "source": "zh-Hans"})
        submit(tencent, tencent_auto | {
            "request": tencent_auto["request"] | {"source": "fr", "target": "en"},
            "response": tencent_auto["response"] | {"source": "fr"},
        }, target="en", source="fr", outcome={"text": TRANSLATED, "source": "fr"})
        submit(tencent, None, target="en-GB", error="UnsupportedLanguage")
        submit(tencent, tencent_auto | {"response": {"choices": []}},
               target="zh-Hant", error="InvalidResponse")
        submit(tencent, tencent_auto | {
            "response": {"choices": [{"finish_reason": "sensitive", "message": {"content": ""}}]},
        }, target="zh-Hant", error="Rejected")
        for code, category in [(400, "Rejected"), (401, "Authentication"), (402, "Quota"),
                               (429, "RateLimited"), (500, "Service"), (307, "Service")]:
            submit(tencent, tencent_auto | {"status": code, "response": ERROR_BODY},
                   target="zh-Hant", error=category)
        submit(tencent | {"timeout_ms": 140}, tencent_auto | {"delay": .45},
               target="zh-Hant", error="Timeout", expected_observations=["request", "failure"])
        print("Tencent TokenHub: dedicated endpoint, Bearer, source, errors, timeout, single send and redaction PASS")

        aliyun = {"driver": "aliyun", "region": "cn-hangzhou", "base_url": base + "/ali",
                  "access_key_id_env": "TRANSLATE_TEST_AK", "access_key_secret_env": "TRANSLATE_TEST_KEY",
                  "security_token_env": "TRANSLATE_TEST_STS", "timeout_ms": 3000}

        def verify_aliyun(source, target, temporary=True, path="/ali/"):
            def check(headers, payload, raw):
                assert payload == {
                    "FormatType": "text", "Scene": "general", "SourceLanguage": source,
                    "SourceText": ORIGINAL, "TargetLanguage": target,
                }
                assert headers.get("x-acs-action") == "TranslateGeneral"
                assert headers.get("x-acs-version") == "2018-10-12"
                assert headers.get("x-acs-security-token") == ("SYNTHETIC_SECRET_STS" if temporary else None)
                assert headers.get("x-acs-date").endswith("Z")
                assert headers.get("x-acs-signature-nonce")
                hashed = hashlib.sha256(raw).hexdigest()
                assert headers["x-acs-content-sha256"] == hashed
                names = ["content-type", "host", "x-acs-action", "x-acs-content-sha256", "x-acs-date"]
                if temporary:
                    names.append("x-acs-security-token")
                names += ["x-acs-signature-nonce", "x-acs-version"]
                signed = ";".join(names)
                canonical = (
                    f"POST\n{path}\n\n"
                    + "".join(f"{name}:{headers[name]}\n" for name in names)
                    + f"\n{signed}\n{hashed}"
                )
                to_sign = "ACS3-HMAC-SHA256\n" + hashlib.sha256(canonical.encode()).hexdigest()
                signature = hmac.new(KEY.encode(), to_sign.encode(), hashlib.sha256).hexdigest()
                assert headers["Authorization"] == (
                    "ACS3-HMAC-SHA256 Credential=SYNTHETIC_SECRET_ACCOUNT,"
                    f"SignedHeaders={signed},Signature={signature}"
                )
            return check

        aliyun_auto = {
            "path": "/ali/", "content_type": "application/x-www-form-urlencoded",
            "check": verify_aliyun("auto", "zh-tw"),
            "response": {"Code": 200, "Message": "success",
                         "Data": {"Translated": TRANSLATED, "DetectedLanguage": "zh"}},
        }
        submit(aliyun, aliyun_auto, target="zh-Hant",
               outcome={"text": TRANSLATED, "source": "zh-Hans"})
        without_sts = {key: value for key, value in aliyun.items() if key != "security_token_env"}
        submit(without_sts, aliyun_auto | {
            "check": verify_aliyun("fr", "en", temporary=False),
            "response": {"Code": "200", "Data": {"Translated": TRANSLATED}},
        }, target="en", source="fr", outcome={"text": TRANSLATED, "source": None})
        submit(aliyun, None, target="en-GB", error="UnsupportedLanguage")
        submit(aliyun, aliyun_auto | {"response": {"Code": 200}}, target="zh-Hant", error="InvalidResponse")
        for code, category in [(10005, "UnsupportedLanguage"), (10013, "Quota"), (10001, "Timeout")]:
            submit(aliyun, aliyun_auto | {"response": ERROR_BODY | {"Code": code}},
                   target="zh-Hant", error=category)
        submit(aliyun, aliyun_auto | {
            "status": 400, "response": ERROR_BODY | {"Code": "Throttling.Api"},
        }, target="zh-Hant", error="RateLimited")
        for code, category in [(403, "Authentication"), (429, "RateLimited"),
                               (500, "Service"), (307, "Service")]:
            submit(aliyun, aliyun_auto | {"status": code, "response": ERROR_BODY},
                   target="zh-Hant", error=category)
        submit(aliyun | {"timeout_ms": 140}, aliyun_auto | {"delay": .45},
               target="zh-Hant", error="Timeout", expected_observations=["request", "failure"])
        print("Aliyun: ACS3 signature, form, STS, source, business errors, timeout, single send and redaction PASS")

        baidu = {"driver": "baidu", "base_url": base + "/baidu", "api_key_env": "TRANSLATE_TEST_KEY",
                 "app_id_env": "TRANSLATE_TEST_APP_ID", "timeout_ms": 3000}

        def verify_baidu(source, target):
            def check(headers, payload, _):
                assert headers.get("Authorization") is None
                assert set(payload) == {"q", "from", "to", "appid", "salt", "sign"}
                assert (payload["q"], payload["from"], payload["to"]) == (ORIGINAL, source, target)
                assert payload["appid"] == "2015063000000001" and payload["salt"].isdigit()
                material = payload["appid"] + ORIGINAL + payload["salt"] + KEY
                assert payload["sign"] == hashlib.md5(material.encode()).hexdigest()
            return check

        baidu_auto = {
            "path": "/baidu/api/trans/vip/translate", "content_type": "application/x-www-form-urlencoded",
            "check": verify_baidu("auto", "cht"),
            "response": {"from": "fra", "to": "cht",
                         "trans_result": [{"dst": TRANSLATED}, {"dst": "第二段"}]},
        }
        submit(baidu, baidu_auto, target="zh-Hant",
               outcome={"text": TRANSLATED + "\n第二段", "source": "fr"})
        submit(baidu, baidu_auto | {"check": verify_baidu("jp", "kor"),
                                   "response": {"from": "jp", "trans_result": [{"dst": TRANSLATED}]}},
               target="ko", source="ja", outcome={"text": TRANSLATED, "source": "ja"})
        submit(baidu, None, target="en-AU", error="UnsupportedLanguage")
        submit(baidu, baidu_auto | {"response": {"trans_result": []}},
               target="zh-Hant", error="InvalidResponse")
        for code, category in [("54001", "Authentication"), ("54003", "RateLimited"),
                               ("54004", "Quota"), ("58001", "UnsupportedLanguage"), (52001, "Timeout")]:
            submit(baidu, baidu_auto | {"response": ERROR_BODY | {"error_code": code}},
                   target="zh-Hant", error=category)
        for code, category in [(429, "RateLimited"), (500, "Service"), (307, "Service")]:
            submit(baidu, baidu_auto | {"status": code, "response": ERROR_BODY},
                   target="zh-Hant", error=category)
        submit(baidu | {"timeout_ms": 140}, baidu_auto | {"delay": .45},
               target="zh-Hant", error="Timeout", expected_observations=["request", "failure"])
        assert len(calls) == len(cases)
        print("Baidu: MD5 and form, source and segments, business errors, timeout, single send and redaction PASS")


def batch_c(binary, stage):
    artisan, events = runner(binary, stage)

    with server() as (base, cases, calls, faults):
        def submit(profile, scenario, **options):
            round_trip(artisan, events, profile, cases, calls, faults, scenario, **options)

        azure = {"driver": "azure", "region": "eastus", "base_url": base + "/azure",
                 "api_key_env": "TRANSLATE_TEST_KEY", "timeout_ms": 3000}

        def verify_azure(region):
            def check(headers, payload, _):
                assert headers.get("Authorization") is None
                assert headers.get("Ocp-Apim-Subscription-Key") == KEY
                assert headers.get("Ocp-Apim-Subscription-Region") == region
                assert payload == [{"Text": ORIGINAL}]
            return check

        azure_auto = {
            "path": "/azure/translate", "query": {"api-version": ["3.0"], "to": ["zh-Hant"]},
            "check": verify_azure("eastus"),
            "response": [{"detectedLanguage": {"language": "en", "score": .99},
                          "translations": [{"text": TRANSLATED, "to": "zh-Hant"}]}],
        }
        submit(azure, azure_auto, target="zh-Hant", outcome={"text": TRANSLATED, "source": "en"})
        global_profile = {name: value for name, value in azure.items() if name != "region"}
        submit(global_profile, azure_auto | {
            "query": {"api-version": ["3.0"], "to": ["en"], "from": ["fr"]},
            "check": verify_azure(None),
            "response": [{"translations": [{"text": TRANSLATED, "to": "en"}]}],
        }, target="en", source="fr", outcome={"text": TRANSLATED, "source": None})
        submit(azure, None, target="en-AU", error="UnsupportedLanguage")
        submit(azure, azure_auto | {"response": [{"translations": []}]},
               target="zh-Hant", error="InvalidResponse")
        for http, code, category in [(400, 400019, "UnsupportedLanguage"), (401, 401000, "Authentication"),
                                     (403, 403001, "Quota"), (429, 429001, "RateLimited"),
                                     (500, 500000, "Service"), (307, 307000, "Service")]:
            submit(azure, azure_auto | {"status": http, "response": {"error": ERROR_BODY | {"code": code}}},
                   target="zh-Hant", error=category)
        submit(azure | {"timeout_ms": 140}, azure_auto | {"delay": .45}, target="zh-Hant",
               error="Timeout", expected_observations=["request", "failure"])
        print("Azure: v3 query, subscription headers, automatic source, errors, timeout, single send and redaction PASS")

        credentials = stage / "adc.json"
        credentials.write_text(json.dumps({
            "type": "authorized_user", "client_id": "fixture-client",
            "client_secret": "SYNTHETIC_SECRET_OAUTH", "refresh_token": "SYNTHETIC_SECRET_REFRESH",
            "quota_project_id": "fixture-quota", "token_uri": base + "/oauth2/token",
        }))
        credentials.chmod(0o600)

        def google_artisan(profile, *arguments, error=None):
            return artisan(profile, *arguments, error=error, extra={
                "GOOGLE_APPLICATION_CREDENTIALS": str(credentials),
                "GOOGLE_CLOUD_QUOTA_PROJECT": "fixture-quota",
            })

        def verify_oauth(headers, payload, _):
            assert payload == {
                "grant_type": "refresh_token", "client_id": "fixture-client",
                "client_secret": "SYNTHETIC_SECRET_OAUTH", "refresh_token": "SYNTHETIC_SECRET_REFRESH",
                "scope": "https://www.googleapis.com/auth/cloud-translation",
            }

        def token(value="SYNTHETIC_SECRET_ACCESS", expires=3600):
            return {"path": "/oauth2/token",
                    "check": verify_oauth,
                    "response": {"access_token": value, "expires_in": expires, "token_type": "Bearer"}}

        def verify_google(payload, token="SYNTHETIC_SECRET_ACCESS"):
            def check(headers, data, _):
                assert data == payload
                assert headers["Authorization"] == f"Bearer {token}"
                assert headers.get("x-goog-user-project") == "fixture-quota"
            return check

        google = {"driver": "google", "project": "test-project", "location": "us-central1",
                  "base_url": base + "/google", "timeout_ms": 3000}
        auto_body = {"contents": [ORIGINAL], "targetLanguageCode": "zh-TW", "mimeType": "text/plain"}
        google_auto = {
            "path": "/google/v3/projects/test-project/locations/us-central1:translateText",
            "check": verify_google(auto_body),
            "response": {"translations": [{"translatedText": TRANSLATED, "detectedLanguageCode": "zh-CN"}]},
        }

        def submit_google(scenario, profile=google, **options):
            round_trip(google_artisan, events, profile, cases, calls, faults, scenario, **options)

        submit_google([token(), google_auto], target="zh-Hant",
                      outcome={"text": TRANSLATED, "source": "zh-Hans"})
        submit_google([token(), google_auto | {
            "path": "/google/v3/projects/test-project/locations/global:translateText",
            "check": verify_google(auto_body | {"targetLanguageCode": "en", "sourceLanguageCode": "fr"}),
            "response": {"translations": [{"translatedText": TRANSLATED}]},
        }], profile=google | {"location": "global"}, target="en", source="fr",
            outcome={"text": TRANSLATED, "source": None})
        submit_google(None, target="en-AU", error="UnsupportedLanguage")
        submit_google([token() | {"status": 400, "response": {"error": "invalid_grant"}}],
                      target="zh-Hant", error="Authentication", expected_observations=[])
        submit_google([token(), google_auto | {"response": {"translations": []}}],
                      target="zh-Hant", error="InvalidResponse")
        for code, reason, category in [(401, "INVALID_CREDENTIALS", "Authentication"),
                                        (403, "QUOTA_EXCEEDED", "Quota"), (429, "RATE_LIMIT_EXCEEDED", "RateLimited"),
                                        (500, "BACKEND_ERROR", "Service"), (307, "REDIRECTED", "Service")]:
            submit_google([token(), google_auto | {
                "status": code, "response": {"error": ERROR_BODY | {"details": [{"reason": reason}]}},
            }], target="zh-Hant", error=category)
        submit_google([token(), google_auto | {"delay": .45}], profile=google | {"timeout_ms": 180},
                      target="zh-Hant", error="Timeout", expected_observations=["request", "failure"])

        before = len(events())
        cases.extend([
            token(expires=242), google_auto, token("SYNTHETIC_SECRET_RENEWED"),
            google_auto | {"check": verify_google(auto_body, "SYNTHETIC_SECRET_RENEWED")},
        ])
        # SDK 1.19 对有效期大于 4 分钟的令牌提前 4 分钟刷新。
        repeated = google_artisan(google, "translate:request", "--repeat-after-ms=2600", ORIGINAL, "zh-Hant")
        assert json.loads(repeated) == [{"text": TRANSLATED, "source": "zh-Hans"}] * 2
        assert events()[before:] == ["request", "response", "request", "response"]
        assert len(calls) == len(cases) and not faults, faults

        cases.extend([token(), google_auto, google_auto])
        cached_token = google_artisan(google, "translate:request", "--repeat-after-ms=25", ORIGINAL, "zh-Hant")
        assert json.loads(cached_token) == [{"text": TRANSLATED, "source": "zh-Hans"}] * 2
        assert len(calls) == len(cases) and not faults, faults
        print("Google Advanced v3: ADC OAuth, cached and refreshed token, project/location, errors, timeout, single send and redaction PASS")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--stage", required=True, type=Path)
    parser.add_argument("--batch", default="a", choices=["a", "b", "c"])
    args = parser.parse_args()
    stage = args.stage.resolve()
    project = Path(__file__).resolve().parents[1]
    assert stage.is_relative_to(project / "target/tmp")
    stage.mkdir(parents=True, exist_ok=False)
    {"a": batch_a, "b": batch_b, "c": batch_c}[args.batch](args.binary.resolve(), stage)
