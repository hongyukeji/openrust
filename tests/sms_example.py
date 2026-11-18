"""Exercise the SMS example with independent Artisan dispatcher and Worker processes."""

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
MAKE_SERVER = runpy.run_path(str(ROOT / "examples/sms/mock.py"))["make_server"]
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


def queued_payloads(backend, queue_name):
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
    return wire, [job["data"]["payload"] for job in jobs]


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
        "aliyun_international", "2018-05-01", "intl-worker-id", "intl-worker-secret",
    ),
}


def aliyun_server(receipts):
    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):
            url = urlsplit(self.path)
            assert url.path == "/"
            assert int(self.headers.get("Content-Length", "0")) == 0
            action = self.headers["x-acs-action"]
            profile, version, key_id, secret = ALIYUN[action]
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
            fields = {key: values[0] for key, values in parse_qs(url.query, strict_parsing=True).items()}
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
        "SMS_ALIYUN_INTERNATIONAL_ENDPOINT": endpoint,
        "SMS_ALIYUN_INTERNATIONAL_ACCESS_KEY_ID": "intl-worker-id",
        "SMS_ALIYUN_INTERNATIONAL_ACCESS_KEY_SECRET": "intl-worker-secret",
        "SMS_ALIYUN_INTERNATIONAL_SENDER_ID": "Fixture123",
        "SMS_ALIYUN_INTERNATIONAL_MESSAGE_TYPE": "NOTIFY",
    }
    artisan = [binary, "artisan"]
    china = env | {"SMS_DRIVER": "aliyun"}
    overseas = env | {"SMS_DRIVER": "aliyun_international"}
    inland = "13800138000"
    params = {"code": "A + 中文"}
    body = "Hello + 港澳台"
    places = ["+85255550123", "+85366123456", "+886912345678", NUMBER]
    def international(to, text):
        return (
            "aliyun_international",
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
    for number in places:
        run(artisan + ["sms:demo", number, body, "--profile", "aliyun_international", "--now"], china)
        assert receipts.get(timeout=5) == international(number, body)
    for rejected in ("reject", "missing id"):
        run(artisan + ["sms:demo", places[0], rejected, "--now"], overseas, "did not accept")
        assert receipts.get(timeout=5) == international(places[0], rejected)

    marker = "dispatcher-credential"
    producer = env | {
        "SMS_ALIYUN_ENDPOINT": "http://127.0.0.1:1",
        "SMS_ALIYUN_INTERNATIONAL_ENDPOINT": "http://127.0.0.1:1",
        "SMS_ALIYUN_ACCESS_KEY_ID": marker,
        "SMS_ALIYUN_ACCESS_KEY_SECRET": marker,
        "SMS_ALIYUN_INTERNATIONAL_ACCESS_KEY_ID": marker,
        "SMS_ALIYUN_INTERNATIONAL_ACCESS_KEY_SECRET": marker,
    }
    run(artisan + ["sms:demo", inland, json.dumps(params), "--template", "SMS_1"],
        producer | {"SMS_DRIVER": "aliyun"})
    run(artisan + ["sms:demo", places[0], body, "--profile", "aliyun_international"],
        producer | {"SMS_DRIVER": "aliyun"})
    assert receipts.empty()
    wire, deliveries = queued_payloads(backend, queue_name)
    profile_values = (
        marker, "cn-worker-id", "intl-worker-id", "cn-worker-secret", "intl-worker-secret",
        endpoint, "http://127.0.0.1:1", "Fixture123",
    )
    assert all(value not in wire for value in profile_values)
    assert deliveries == [
        {"profile": "aliyun", "message": {"to": inland, "content": "", "template": {"id": "SMS_1", "parameters": params}}},
        {"profile": "aliyun_international", "message": {"to": places[0], "content": body}},
    ], deliveries
    run(artisan + ["queue:work", backend, "--once", "--tries=1"], overseas)
    assert receipts.get(timeout=5) == template
    run(artisan + ["queue:work", backend, "--once", "--tries=1"], china)
    assert receipts.get(timeout=5) == international(places[0], body)
    assert receipts.empty()
    if backend == "database":
        assert mysql_rows(os.environ["OPENRUST_ACCEPTANCE_MYSQL_CONTAINER"],
                          "SELECT COUNT(*) FROM jobs; SELECT COUNT(*) FROM failed_jobs").split() == ["0", "0"]
    else:
        assert not run(["docker", "exec", os.environ["SMS_DEMO_REDIS_CONTAINER"],
                        "redis-cli", "--raw", "--scan", "--pattern", f"*queues:{queue_name}*"]).strip()
    print(f"{backend}: Aliyun mainland and international Worker delivery, market checks and credential isolation passed", flush=True)


def main():
    binary = str(Path(sys.argv[1]).resolve())
    requests = Queue()
    with tempfile.TemporaryDirectory(prefix="openrust-sms-app-") as root:
        aliyun_receipts = Queue()
        with MAKE_SERVER(0, lambda *receipt: requests.put(receipt)) as server, aliyun_server(aliyun_receipts) as aliyun:
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
            finally:
                aliyun.shutdown()
                aliyun_thread.join()
                server.shutdown()
                thread.join()


if __name__ == "__main__":
    main()
