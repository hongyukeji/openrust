"""Exercise the SMS example with independent Artisan dispatcher and Worker processes."""

import base64
import json
import os
from pathlib import Path
from queue import Queue
import runpy
import subprocess
import sys
import tempfile
from threading import Thread

ROOT = Path(__file__).resolve().parent.parent
MAKE_SERVER = runpy.run_path(str(ROOT / "examples/sms/mock.py"))["make_server"]
NUMBER = "+12025550123"
ALPHA_TOKEN = "worker-alpha-secret"
BETA_TOKEN = "worker-beta-secret"
DISPATCH_TOKEN = "dispatcher-only-secret"


def run(args, env=None):
    result = subprocess.run(args, cwd=ROOT, env=env, capture_output=True, text=True, timeout=45)
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
        'MYSQL_PWD="$MYSQL_ROOT_PASSWORD" mysql -uroot --batch --raw --skip-column-names "$MYSQL_DATABASE" -e "$1"',
        "sh", query,
    ])


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

    if backend == "database":
        payloads = mysql_rows(os.environ["OPENRUST_ACCEPTANCE_MYSQL_CONTAINER"], "SELECT payload FROM jobs ORDER BY id")
    else:
        payloads = redis_payload(os.environ["SMS_DEMO_REDIS_CONTAINER"], queue_name)
    assert all(token not in payloads for token in (ALPHA_TOKEN, BETA_TOKEN, DISPATCH_TOKEN))
    assert "/alpha" not in payloads and "/beta" not in payloads
    for payload, (_, message, _, profile) in zip(payloads.splitlines(), cases, strict=True):
        job = json.loads(payload)
        if backend == "redis":
            job = json.loads(job["payload"])
        delivery = job["data"]["payload"]
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


def main():
    binary = str(Path(sys.argv[1]).resolve())
    requests = Queue()
    with tempfile.TemporaryDirectory(prefix="openrust-sms-app-") as root:
        with MAKE_SERVER(0, lambda *receipt: requests.put(receipt)) as server:
            thread = Thread(target=server.serve_forever)
            thread.start()
            origin = f"http://127.0.0.1:{server.server_port}"
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
            finally:
                server.shutdown()
                thread.join()


if __name__ == "__main__":
    main()
