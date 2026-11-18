"""Exercise SMS delivery with independent Artisan dispatcher and Worker processes."""

import base64
import hashlib
import hmac
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
from queue import Queue
import runpy
import subprocess
import sys
import tempfile
from threading import Thread
from urllib.parse import parse_qs, urlsplit

ROOT = Path(__file__).resolve().parent.parent
MAKE_SERVER = runpy.run_path(str(ROOT / "tests/support/sms/mock.py"))["make_server"]
NUMBER = "+12025550123"
ALPHA_TOKEN = "worker-alpha-secret"
BETA_TOKEN = "worker-beta-secret"
DISPATCH_TOKEN = "dispatcher-only-secret"


def run(args, env=None, expected_error=None):
    result = subprocess.run(args, cwd=ROOT, env=env, capture_output=True, text=True, timeout=45)
    if expected_error is not None:
        assert result.returncode != 0 and expected_error in result.stderr, result.stderr
        return result.stdout
    if result.returncode:
        raise AssertionError(f"{args[-4:]} exited {result.returncode}\n{result.stdout}{result.stderr}")
    return result.stdout


def redis_payload(container, queue_name):
    keys = run(["docker", "exec", container, "redis-cli", "--raw", "--scan", "--pattern", f"*queues:{queue_name}"]).splitlines()
    assert len(keys) == 1, keys
    return run(["docker", "exec", container, "redis-cli", "--raw", "LRANGE", keys[0], "0", "-1"])


def mysql_rows(container, query):
    return run([
        "docker", "exec", container, "sh", "-ec",
        'MYSQL_PWD="$MYSQL_ROOT_PASSWORD" mysql -uroot --default-character-set=utf8mb4 --batch --raw --skip-column-names "$MYSQL_DATABASE" -e "$1"',
        "sh", query,
    ])


def queued_payloads(backend, queue_name, encrypted=False):
    if backend == "database":
        wire = mysql_rows(
            os.environ["OPENRUST_ACCEPTANCE_MYSQL_CONTAINER"],
            "SELECT payload FROM jobs ORDER BY id",
        )
    else:
        wire = redis_payload(os.environ["SMS_DEMO_REDIS_CONTAINER"], queue_name)
    jobs = [json.loads(row) for row in wire.splitlines()]
    if backend == "redis":
        jobs = [json.loads(job["payload"]) for job in jobs]
    return wire, [job["data"] if encrypted else job["data"]["payload"] for job in jobs]


def check_backend(binary, backend, env, requests):
    queue_name = f"sms-demo-{backend}"
    env = env | {
        "QUEUE_CONNECTION": backend,
        "DB_QUEUE": queue_name,
        "REDIS_QUEUE": queue_name,
        "SMS_ALPHA_TOKEN": ALPHA_TOKEN,
        "SMS_BETA_TOKEN": BETA_TOKEN,
    }
    if backend == "database":
        run([binary, "artisan", "migrate", "--force"], env)
    artisan = [binary, "artisan"]

    run(artisan + ["sms:demo", NUMBER, "direct-default", "--now"], env | {"SMS_DRIVER": "backup"})
    run(artisan + ["sms:demo", NUMBER, "direct-explicit", "--profile", "primary", "--now"], env | {"SMS_DRIVER": "backup"})
    beta_auth = "Basic " + base64.b64encode(f"demo:{BETA_TOKEN}".encode()).decode()
    alpha_auth = f"Bearer {ALPHA_TOKEN}"
    assert requests.get(timeout=5) == ("/beta", {"recipient": NUMBER, "message": "direct-default"}, beta_auth)
    assert requests.get(timeout=5) == ("/alpha", {"to": NUMBER, "text": "direct-explicit"}, alpha_auth)
    print(f"{backend}: Sms::driver default and explicit resolution passed", flush=True)

    cases = [
        ("primary", "default-primary", [], "primary"),
        ("backup", "default-changed-to-backup", [], "backup"),
        ("primary", "explicit-backup", ["--profile", "backup"], "backup"),
        ("backup", "explicit-primary", ["--profile", "primary"], "primary"),
    ]
    for default, message, options, _profile in cases:
        producer = env | {
            "SMS_DRIVER": default,
            "SMS_ALPHA_URL": "http://127.0.0.1:1/alpha",
            "SMS_BETA_URL": "http://127.0.0.1:1/beta",
            "SMS_ALPHA_TOKEN": DISPATCH_TOKEN,
            "SMS_BETA_TOKEN": DISPATCH_TOKEN,
        }
        output = run(artisan + ["sms:demo", NUMBER, message] + options, producer)
        assert "Demo SMS dispatched." in output, output
    assert requests.empty(), "Dispatcher contacted a provider before a Worker ran"

    wire, deliveries = queued_payloads(backend, queue_name)
    assert all(token not in wire for token in (ALPHA_TOKEN, BETA_TOKEN, DISPATCH_TOKEN))
    assert "/alpha" not in wire and "/beta" not in wire
    for delivery, (_, message, _, profile) in zip(deliveries, cases, strict=True):
        assert delivery == {"profile": profile, "message": {"to": NUMBER, "content": message}}
    print(f"{backend}: four queued profiles recorded, no endpoint or credentials in payloads", flush=True)

    for _default, message, _options, profile in cases:
        worker = env | {"SMS_DRIVER": "backup" if profile == "primary" else "primary"}
        run(artisan + ["queue:work", backend, "--once", "--tries=1"], worker)
        expected = (
            ("/alpha", {"to": NUMBER, "text": message}, alpha_auth)
            if profile == "primary"
            else ("/beta", {"recipient": NUMBER, "message": message}, beta_auth)
        )
        assert requests.get(timeout=5) == expected
    assert requests.empty()
    if backend == "database":
        assert mysql_rows(os.environ["OPENRUST_ACCEPTANCE_MYSQL_CONTAINER"], "SELECT COUNT(*) FROM jobs; SELECT COUNT(*) FROM failed_jobs").split() == ["0", "0"]
    else:
        assert not run(["docker", "exec", os.environ["SMS_DEMO_REDIS_CONTAINER"], "redis-cli", "--raw", "--scan", "--pattern", f"*queues:{queue_name}*"]).strip()
    print(f"{backend}: all four independent Workers delivered to the dispatched profile with Worker credentials", flush=True)


ALIYUN = {
    "SendSms": ("aliyun", "2017-05-25", "cn-worker-id", "cn-worker-secret"),
    "SendMessageToGlobe": (
        "alerts_global", "2018-05-01", "intl-worker-id", "intl-worker-secret",
    ),
}


def provider_server(receipts):
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            url = urlsplit(self.path)
            fields = {k: v[0] for k, v in parse_qs(url.query, strict_parsing=True).items()}
            profile = {"/sms": "smsbao", "/wsms": "smsbao-global"}[url.path]
            assert set(fields) == {"u", "p", "m", "c"}
            assert fields.pop("p") in {profile + "-worker-secret", "shared-worker-secret"}
            assert fields.pop("u") in {profile + "-user", "shared-user"}
            receipts.put((profile, fields))
            if fields["c"] == "dropped-after-acceptance":
                self.close_connection = True
                return
            if fields["c"].startswith("rate-limited-") and fields["c"] not in getattr(self.server, "rate_limits_seen", ()):
                self.server.rate_limits_seen = (*getattr(self.server, "rate_limits_seen", ()), fields["c"])
                self.send_response(429)
                self.end_headers()
                return
            if fields["c"] == "redirect":
                self.send_response(307)
                self.send_header("Location", "/should-not-follow")
                self.end_headers()
                return
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"30" if fields["c"] == "reject" else b"0\n")

        def do_POST(self):
            url = urlsplit(self.path)
            if url.path.startswith("/v2/sms/"):
                self.yunpian(url.path)
                return
            assert url.path == "/"
            assert int(self.headers.get("Content-Length", "0")) == 0
            action = self.headers["x-acs-action"]
            profile, version, key_id, secret = ALIYUN[action]
            fields = {key: values[0] for key, values in parse_qs(url.query, strict_parsing=True).items()}
            if action == "SendMessageToGlobe" and fields["Message"] == "shared-credentials":
                _, _, key_id, secret = ALIYUN["SendSms"]
            assert self.headers["x-acs-version"] == version
            digest = hashlib.sha256(b"").hexdigest()
            assert self.headers["x-acs-content-sha256"] == digest
            scheme, signature = self.headers["Authorization"].rsplit(",Signature=", 1)
            keys = scheme.removeprefix(f"ACS3-HMAC-SHA256 Credential={key_id},SignedHeaders=")
            assert keys != scheme
            headers = "".join(f"{key}:{self.headers[key]}\n" for key in keys.split(";"))
            canonical = f"POST\n/\n{url.query}\n{headers}\n{keys}\n{digest}"
            to_sign = "ACS3-HMAC-SHA256\n" + hashlib.sha256(canonical.encode()).hexdigest()
            expected = hmac.new(secret.encode(), to_sign.encode(), hashlib.sha256).hexdigest()
            assert signature == expected
            receipts.put((profile, fields))
            if action == "SendSms":
                reply = {"Code": "OK", "BizId": "cn-fixture"}
            elif fields["Message"] == "reject":
                reply = {"ResponseCode": "DENIED", "ResponseDescription": "private upstream detail"}
            elif fields["Message"] == "missing id":
                reply = {"ResponseCode": "OK"}
            else:
                reply = {"ResponseCode": "OK", "MessageId": "intl-fixture"}
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps(reply).encode())

        def yunpian(self, path):
            assert self.headers["Content-Type"].startswith("application/x-www-form-urlencoded")
            raw = self.rfile.read(int(self.headers["Content-Length"])).decode()
            fields = {k: v[0] for k, v in parse_qs(raw, strict_parsing=True, keep_blank_values=True).items()}
            profile = "yunpian-global" if fields["mobile"].startswith("+") else "yunpian"
            key = fields.pop("apikey")
            if key == "orders-cn-worker-secret":
                assert profile == "yunpian"
                profile = "orders_cn"
            else:
                assert key in {profile + "-worker-secret", "shared-worker-secret"}
            if path == "/v2/sms/tpl_single_send.json":
                assert profile == "yunpian"
                assert set(fields) == {"mobile", "tpl_id", "tpl_value"}
                fields["tpl_value"] = {k: v[0] for k, v in parse_qs(fields["tpl_value"], strict_parsing=True).items()}
            else:
                assert path == "/v2/sms/single_send.json" and set(fields) == {"mobile", "text"}
            receipts.put((profile, fields))
            if fields.get("text") == "reject":
                reply = {"code": 2, "msg": "private upstream detail"}
            elif fields.get("text") == "missing sid":
                reply = {"code": 0}
            else:
                reply = {"code": 0, "sid": 3310228978}
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps(reply).encode())

        def log_message(self, *_args):
            pass

    return ThreadingHTTPServer(("127.0.0.1", 0), Handler)


def check_aliyun(binary, backend, env, receipts, endpoint):
    queue_name = f"sms-aliyun-{backend}"
    env = env | {
        "QUEUE_CONNECTION": backend, "DB_QUEUE": queue_name, "REDIS_QUEUE": queue_name,
        "SMS_ALIYUN_ENDPOINT": endpoint,
        "SMS_ALIYUN_ACCESS_KEY_ID": "cn-worker-id",
        "SMS_ALIYUN_ACCESS_KEY_SECRET": "cn-worker-secret",
        "SMS_ALIYUN_SIGN_NAME": "示例 + 签名",
        "SMS_ALERTS_GLOBAL_ENDPOINT": endpoint,
        "SMS_ALERTS_GLOBAL_ACCESS_KEY_ID": "intl-worker-id",
        "SMS_ALERTS_GLOBAL_ACCESS_KEY_SECRET": "intl-worker-secret",
        "SMS_ALERTS_GLOBAL_SENDER_ID": "Fixture123",
        "SMS_ALERTS_GLOBAL_MESSAGE_TYPE": "NOTIFY",
    }
    artisan = [binary, "artisan"]
    china = env | {"SMS_DRIVER": "aliyun", "SMS_ALIYUN_MARKET": "mainland"}
    overseas = env | {"SMS_DRIVER": "alerts_global"}
    single_global_account = env | {
        "SMS_DRIVER": "aliyun",
        "SMS_ALIYUN_MARKET": "international",
        "SMS_ALIYUN_ACCESS_KEY_ID": "intl-worker-id",
        "SMS_ALIYUN_ACCESS_KEY_SECRET": "intl-worker-secret",
        "SMS_ALIYUN_SENDER_ID": "Fixture123",
        "SMS_ALIYUN_MESSAGE_TYPE": "NOTIFY",
    }
    inland = "13800138000"
    params = {"code": "A + 中文"}
    body = "Hello + 港澳台"
    places = ["+85255550123", "+85366123456", "+886912345678", NUMBER]
    def international(to, text):
        return (
            "alerts_global",
            {"To": to[1:], "From": "Fixture123", "Type": "NOTIFY", "Message": text},
        )

    template = ("aliyun", {
        "PhoneNumbers": inland, "SignName": "示例 + 签名",
        "TemplateCode": "SMS_1", "TemplateParam": '{"code":"A + 中文"}',
    })

    run(artisan + ["sms:demo", "+86" + inland, body, "--now"], overseas, "accepts international")
    run(artisan + ["sms:demo", places[0], json.dumps(params), "--template", "SMS_1", "--now"],
        china, "accepts Mainland China numbers only")
    run(artisan + ["sms:demo", places[0], json.dumps(params), "--template", "SMS_1", "--now"],
        overseas, "requires a text body")
    run(artisan + ["sms:demo", inland, body, "--now"], china, "requires an approved template")
    assert receipts.empty()
    run(artisan + ["sms:demo", places[0], body, "--now"], single_global_account)
    assert receipts.get(timeout=5) == international(places[0], body)
    for number in places:
        run(artisan + ["sms:demo", number, body, "--profile", "alerts_global", "--now"], china)
        assert receipts.get(timeout=5) == international(number, body)
    for rejected in ("reject", "missing id"):
        run(artisan + ["sms:demo", places[0], rejected, "--now"], overseas, "did not accept")
        assert receipts.get(timeout=5) == international(places[0], rejected)

    shared = env | {
        "SMS_DRIVER": "aliyun",
        "SMS_ALIYUN_SENDER_ID": "Fixture123",
        "SMS_ALIYUN_MESSAGE_TYPE": "NOTIFY",
    }
    run(artisan + ["sms:demo", places[0], "shared-credentials", "--now"], shared)
    assert receipts.get(timeout=5) == international(places[0], "shared-credentials")
    run(artisan + ["sms:demo", "+86" + inland, json.dumps(params), "--template", "login_code", "--named-template", "--now"], shared)
    assert receipts.get(timeout=5) == template

    auto = env | {"SMS_DRIVER": "aliyun", "SMS_TEST_SPLIT_MARKETS": "1"}
    run(artisan + ["sms:demo", places[0], body, "--now"], auto)
    assert receipts.get(timeout=5) == international(places[0], body)
    run(artisan + ["sms:demo", places[0], json.dumps({"code": "A1024"}), "--template", "login_code", "--named-template", "--now"], auto)
    assert receipts.get(timeout=5) == international(places[0], "Order A1024 is ready.")
    run(artisan + ["sms:demo", NUMBER.removeprefix("+"), body, "--now"], auto, "+ country calling code")
    run(artisan + ["sms:demo", "+86" + inland, json.dumps(params), "--template", "login_code", "--named-template", "--profile", "aliyun", "--now"], auto)
    assert receipts.get(timeout=5) == template
    run(artisan + ["sms:demo", "+86" + inland, json.dumps(params), "--template", "unknown", "--named-template", "--now"],
        auto, "template is not configured")
    assert receipts.empty()

    marker = "dispatcher-credential"
    producer = env | {
        "SMS_ALIYUN_ENDPOINT": "http://127.0.0.1:1",
        "SMS_ALERTS_GLOBAL_ENDPOINT": "http://127.0.0.1:1",
        "SMS_ALIYUN_ACCESS_KEY_ID": marker,
        "SMS_ALIYUN_ACCESS_KEY_SECRET": marker,
        "SMS_ALERTS_GLOBAL_ACCESS_KEY_ID": marker,
        "SMS_ALERTS_GLOBAL_ACCESS_KEY_SECRET": marker,
    }
    run(artisan + ["sms:demo", "+86" + inland, json.dumps(params), "--template", "SMS_1"],
        producer | {"SMS_DRIVER": "aliyun"})
    run(artisan + ["sms:demo", places[0], body, "--profile", "aliyun"],
        producer | {"SMS_DRIVER": "aliyun"})
    run(artisan + ["sms:demo", "+86" + inland, json.dumps(params), "--template", "login_code", "--named-template"], producer | {"SMS_DRIVER": "aliyun"})
    assert receipts.empty()
    wire, deliveries = queued_payloads(backend, queue_name)
    profile_values = (
        marker, "cn-worker-id", "intl-worker-id", "cn-worker-secret", "intl-worker-secret",
        endpoint, "http://127.0.0.1:1", "Fixture123",
    )
    assert all(value not in wire for value in profile_values)
    assert deliveries == [
        {"profile": "aliyun", "message": {"to": "+86" + inland, "content": "", "template": {"id": "SMS_1", "parameters": params}}},
        {"profile": "aliyun", "message": {"to": places[0], "content": body}},
        {"profile": "aliyun", "message": {"to": "+86" + inland, "content": "", "template_name": "login_code", "template": {"id": "login_code", "parameters": params}}},
    ], deliveries
    run(artisan + ["queue:work", backend, "--once", "--tries=1"], overseas)
    assert receipts.get(timeout=5) == template
    run(artisan + ["queue:work", backend, "--once", "--tries=1"], auto | {"SMS_DRIVER": "log"})
    assert receipts.get(timeout=5) == international(places[0], body)
    run(artisan + ["queue:work", backend, "--once", "--tries=1"], auto)
    assert receipts.get(timeout=5) == template
    assert receipts.empty()
    if backend == "database":
        assert mysql_rows(os.environ["OPENRUST_ACCEPTANCE_MYSQL_CONTAINER"],
                          "SELECT COUNT(*) FROM jobs; SELECT COUNT(*) FROM failed_jobs").split() == ["0", "0"]
    else:
        assert not run(["docker", "exec", os.environ["SMS_DEMO_REDIS_CONTAINER"],
                        "redis-cli", "--raw", "--scan", "--pattern", f"*queues:{queue_name}*"]).strip()
    print(f"{backend}: Aliyun mainland and international Worker delivery, market checks and credential isolation passed", flush=True)


def check_local_providers(binary, backend, env, receipts, endpoint):
    queue_name = f"sms-additional-{backend}"
    profiles = ("smsbao", "yunpian")
    env = env | {"QUEUE_CONNECTION": backend, "DB_QUEUE": queue_name, "REDIS_QUEUE": queue_name}
    artisan = [binary, "artisan"]
    mainland = "13800138000"
    international = "+85255550123"
    content = "〖示例签名〗中文 + & 42"
    variables = {"code": "42 + 中文 & 5"}

    def account(profile, market, dispatcher=False):
        prefix = f"SMS_{profile.upper()}"
        label = profile if market == "mainland" else profile + "-global"
        return env | {
            f"{prefix}_MARKET": market,
            f"{prefix}_ENDPOINT": "http://127.0.0.1:1" if dispatcher else endpoint,
            f"{prefix}_API_KEY": "dispatch-only-credential" if dispatcher else label + "-worker-secret",
            f"{prefix}_USERNAME": label + "-user",
        }

    def send(profile, market, phone, body, *extra, expected_error=None):
        return run(artisan + ["sms:demo", phone, body, "--profile", profile, *extra],
                   account(profile, market), expected_error)

    send("smsbao", "mainland", international, content, "--now", expected_error="Mainland China numbers only")
    send("smsbao", "international", "+86" + mainland, content, "--now", expected_error="accepts international")
    send("smsbao", "mainland", mainland, "{}", "--template", "12", "--now", expected_error="requires the rendered")
    send("yunpian", "international", international, "{}", "--template", "12", "--now", expected_error="requires the rendered")
    send("yunpian", "mainland", mainland, "{}", "--template", "invalid", "--now", expected_error="positive integer")
    send("yunpian", "EU", international, content, "--now", expected_error="market must be")
    assert receipts.empty()

    for profile, number, message, error in [
        ("smsbao", mainland, "reject", "(30)"),
        ("smsbao", mainland, "redirect", "HTTP (307)"),
        ("yunpian", mainland, "reject", "(2)"),
        ("yunpian", mainland, "missing sid", "invalid submission result"),
    ]:
        send(profile, "mainland", number, message, "--now", expected_error=error)
        assert receipts.get(timeout=5)[0] == profile
    send("yunpian", "mainland", "+86" + mainland, content, "--now")
    assert receipts.get(timeout=5) == ("yunpian", {"mobile": mainland, "text": content})

    for profile in profiles:
        prefix = f"SMS_{profile.upper()}"
        shared = account(profile, "auto") | {
            f"{prefix}_API_KEY": "shared-worker-secret",
            f"{prefix}_USERNAME": "shared-user",
        }
        for phone, label in (("+86" + mainland, profile), (international, profile + "-global")):
            run(artisan + ["sms:demo", phone, content, "--profile", profile, "--now"], shared)
            assert receipts.get(timeout=5)[0] == label

    cases = [
        ("smsbao", "mainland", "+86" + mainland, content, [], {"m": mainland, "c": content}),
        ("smsbao", "international", international, content, [], {"m": international, "c": content}),
        ("yunpian", "mainland", mainland, json.dumps(variables), ["--template", "12"],
         {"mobile": mainland, "tpl_id": "12", "tpl_value": {"#code#": variables["code"]}}),
        ("yunpian", "international", international, content, [], {"mobile": international, "text": content}),
    ]
    for profile, market, number, text, options, _ in cases:
        run(artisan + ["sms:demo", number, text, "--profile", profile, *options],
            account(profile, market, dispatcher=True))
    assert receipts.empty()
    wire, deliveries = queued_payloads(backend, queue_name)
    assert len(deliveries) == len(cases)
    assert all(f"{profile}{suffix}-worker-secret" not in wire
               for profile in profiles for suffix in ("", "-global"))
    assert "dispatch-only-credential" not in wire and endpoint not in wire and "http://127.0.0.1:1" not in wire
    for delivery, (profile, market, number, text, options, fields) in zip(deliveries, cases, strict=True):
        message = {"to": number, "content": text}
        if options:
            message = {"to": number, "content": "", "template": {"id": "12", "parameters": variables}}
        assert delivery == {"profile": profile, "message": message}
        run(artisan + ["queue:work", backend, "--once", "--tries=1"], account(profile, market))
        label = profile if market == "mainland" else profile + "-global"
        assert receipts.get(timeout=5) == (label, fields)
    assert receipts.empty()
    print(f"{backend}: SmsBao and Yunpian passed Worker and credential checks in both markets", flush=True)


def check_named_profiles(binary, backend, env, receipts, endpoint):
    queue_name = f"sms-named-{backend}"
    artisan = [binary, "artisan"]
    mobile = "+8613800138000"
    plain = "〖示例签名〗具名账户"
    default_config = env | {
        "QUEUE_CONNECTION": backend, "DB_QUEUE": queue_name, "REDIS_QUEUE": queue_name,
        "SMS_DRIVER": "yunpian",
        "SMS_YUNPIAN_ENDPOINT": endpoint,
        "SMS_YUNPIAN_API_KEY": "yunpian-worker-secret",
    }
    run(artisan + ["sms:demo", mobile, plain, "--now"], default_config)
    assert receipts.get(timeout=5) == ("yunpian", {"mobile": mobile[3:], "text": plain})

    profiles = default_config | {
        "SMS_DRIVER": "orders_cn",
        "SMS_ORDERS_CN_ENDPOINT": endpoint,
        "SMS_ORDERS_CN_API_KEY": "orders-cn-worker-secret",
    }
    run(artisan + ["sms:demo", mobile, plain, "--now"], profiles)
    assert receipts.get(timeout=5) == ("orders_cn", {"mobile": mobile[3:], "text": plain})
    run(artisan + ["sms:demo", mobile, plain, "--profile", "yunpian", "--now"], profiles)
    assert receipts.get(timeout=5) == ("yunpian", {"mobile": mobile[3:], "text": plain})

    producer = profiles | {
        "SMS_YUNPIAN_ENDPOINT": "http://127.0.0.1:1",
        "SMS_ORDERS_CN_ENDPOINT": "http://127.0.0.1:1",
        "SMS_YUNPIAN_API_KEY": "base-dispatch-key",
        "SMS_ORDERS_CN_API_KEY": "orders-dispatch-key",
    }
    run(artisan + ["sms:demo", mobile, plain], producer)
    run(artisan + ["sms:demo", mobile, plain, "--profile", "yunpian"], producer)
    wire, deliveries = queued_payloads(backend, queue_name)
    for value in ("base-dispatch-key", "orders-dispatch-key", "orders-cn-worker-secret",
                  "yunpian-worker-secret", "http://127.0.0.1:1", endpoint):
        assert value not in wire
    assert deliveries == [
        {"profile": profile, "message": {"to": mobile, "content": plain}}
        for profile in ("orders_cn", "yunpian")
    ]
    assert receipts.empty()
    for profile, default in (("orders_cn", "yunpian"), ("yunpian", "orders_cn")):
        run(artisan + ["queue:work", backend, "--once", "--tries=1"],
            profiles | {"SMS_DRIVER": default})
        assert receipts.get(timeout=5) == (profile, {"mobile": mobile[3:], "text": plain})
    assert receipts.empty()
    print(f"{backend}: named profiles share one driver and keep separate Worker credentials", flush=True)


def check_log_default(binary, backend, env):
    queue_name = f"sms-log-{backend}"
    env = env | {
        "QUEUE_CONNECTION": backend, "DB_QUEUE": queue_name, "REDIS_QUEUE": queue_name,
        "LOG_CHANNEL": "single", "LOG_LEVEL": "info",
    }
    env.pop("SMS_DRIVER", None)
    artisan = [binary, "artisan"]
    log = Path(env["APP_BASE_PATH"]) / "storage/logs/openrust.log"
    before = log.stat().st_size if log.exists() else 0
    run(artisan + ["sms:demo", NUMBER, "private-debug-body", "--now"], env)
    run(artisan + ["sms:demo", NUMBER, '{"otp":"private-debug-otp"}',
                   "--template", "private-template", "--named-template"], env)
    _, deliveries = queued_payloads(backend, queue_name)
    assert len(deliveries) == 1 and deliveries[0]["profile"] == "log", deliveries
    run(artisan + ["queue:work", backend, "--once", "--tries=1"],
        env | {"SMS_DRIVER": "aliyun"})
    with log.open("rb") as stream:
        stream.seek(before)
        lines = stream.read().decode().splitlines()
    entries = [line for line in lines if "SMS recorded by log driver." in line]
    assert len(entries) == 2 and all("0123" in line for line in entries), entries
    assert "text" in entries[0] and "template" in entries[1], entries
    assert all(value not in "\n".join(entries)
               for value in (NUMBER, "private-debug-body", "private-debug-otp", "private-template"))
    print(f"{backend}: default log driver recorded direct and queued SMS without private fields", flush=True)


def check_hardened_worker(binary, backend, env, receipts, endpoint):
    queue = f"sms-hardened-{backend}"
    env = env | {
        "QUEUE_CONNECTION": backend, "DB_QUEUE": queue, "REDIS_QUEUE": queue,
        "QUEUE_FAILED_DRIVER": "database-uuids",
        "APP_KEY": "base64:" + base64.b64encode(b"x" * 32).decode(),
        "SMS_DRIVER": "smsbao", "SMS_SMSBAO_ENDPOINT": endpoint,
        "SMS_SMSBAO_USERNAME": "shared-user", "SMS_SMSBAO_API_KEY": "shared-worker-secret",
        "SMS_TEST_ENCRYPT": "1", "LOG_CHANNEL": "single", "LOG_LEVEL": "info",
    }
    artisan = [binary, "artisan"]
    number = "+8613800138000"
    messages = ("delivered-private-content", "dropped-after-acceptance", f"rate-limited-{backend}")
    run(artisan + ["sms:demo", number, json.dumps({"message": messages[0]}),
                   "--template", "security_notice", "--named-template"], env)
    for message in messages[1:]:
        run(artisan + ["sms:demo", number, message], env)
    wire, jobs = queued_payloads(backend, queue, encrypted=True)
    assert len(jobs) == 3 and all(job.get("encrypted_delivery") for job in jobs)
    assert all(value not in wire for value in (number, *messages, "shared-worker-secret"))
    worker = env | {"SMS_TEST_LISTENER_FAIL": "1"}
    log = Path(env["APP_BASE_PATH"]) / "storage/logs/openrust.log"
    before = log.stat().st_size if log.exists() else 0
    run(artisan + ["queue:work", backend, "--stop-when-empty", "--tries=3"], worker)
    with log.open("rb") as stream:
        stream.seek(before)
        lines = stream.read().decode().splitlines()
    events = [line for line in lines if "The SMS " in line and ("accepted the submission" in line or "listener failed" in line)]
    assert ["accepted the submission" in line for line in events] == [True, False, True, False], events
    assert all(value not in "\n".join(events) for value in (number, *messages, "shared-worker-secret"))
    accepted = [receipts.get(timeout=5)[1]["c"] for _ in range(4)]
    assert accepted == [*messages, messages[-1]], accepted
    assert receipts.empty()
    failure = mysql_rows(os.environ["OPENRUST_ACCEPTANCE_MYSQL_CONTAINER"],
        f"SELECT payload FROM failed_jobs WHERE queue = '{queue}'")
    assert "encrypted_delivery" in failure and all(value not in failure for value in (number, *messages))
    print(f"{backend}: encrypted queue and failed payloads, redacted HTTP event, terminal unknown status, listener failure, and retryable HTTP 429 passed", flush=True)


def main():
    binary = str(Path(sys.argv[1]).resolve())
    requests = Queue()
    with tempfile.TemporaryDirectory(prefix="openrust-sms-app-") as root:
        aliyun_receipts = Queue()
        with MAKE_SERVER(0, lambda *receipt: requests.put(receipt)) as server, provider_server(aliyun_receipts) as aliyun:
            thread = Thread(target=server.serve_forever)
            thread.start()
            aliyun_thread = Thread(target=aliyun.serve_forever)
            aliyun_thread.start()
            origin = f"http://127.0.0.1:{server.server_port}"
            aliyun_endpoint = f"http://127.0.0.1:{aliyun.server_port}"
            env = dict(os.environ) | {
                "APP_BASE_PATH": root,
                "APP_ENV": "local",
                "CACHE_STORE": "array",
                "DB_CONNECTION": "mysql",
                "DB_URL": os.environ["OPENRUST_TEST_MYSQL_URL"],
                "REDIS_HOST": "127.0.0.1",
                "REDIS_PORT": os.environ["SMS_DEMO_REDIS_PORT"],
                "REDIS_DB": "0",
                "SMS_ALPHA_URL": f"{origin}/alpha",
                "SMS_BETA_URL": f"{origin}/beta",
            }
            try:
                check_backend(binary, "database", env, requests)
                check_backend(binary, "redis", env, requests)
                for backend in ("database", "redis"):
                    check_aliyun(binary, backend, env, aliyun_receipts, aliyun_endpoint)
                    check_local_providers(binary, backend, env, aliyun_receipts, aliyun_endpoint)
                    check_named_profiles(binary, backend, env, aliyun_receipts, aliyun_endpoint)
                    check_log_default(binary, backend, env)
                for backend in ("database", "redis"):
                    check_hardened_worker(binary, backend, env, aliyun_receipts, aliyun_endpoint)
            finally:
                aliyun.shutdown()
                aliyun_thread.join()
                server.shutdown()
                thread.join()


if __name__ == "__main__":
    main()
