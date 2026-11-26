"""外部 Pay 驱动的真实 HMAC 回调、双节点事务及独立 Worker 验收。"""

import argparse
from concurrent.futures import ThreadPoolExecutor, TimeoutError
from contextlib import contextmanager, suppress
import hmac
import http.client
import json
import os
from pathlib import Path
import runpy
import socket
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
DATABASE = runpy.run_path(str(ROOT / "tests/sms_worker.py"))
HTTP = runpy.run_path(str(ROOT / "tests/webhooks_acceptance.py"))
FIXTURE_KEY = b"openrust-pay-fixture-hmac"


def exercise(binary, driver, database_url, root):
    sqlite = root / "fixture.sqlite"
    if driver == "sqlite":
        sqlite.touch()
    env = os.environ | {
        "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "DB_CONNECTION": driver, "QUEUE_CONNECTION": "database",
        "DB_DATABASE": str(sqlite) if driver == "sqlite" else "openrust_acceptance",
        "PAY_FIXTURE_MERCHANT": "merchant-original",
    }
    if database_url:
        env["DB_URL"] = database_url
    else:
        env.pop("DB_URL", None)

    def node(name):
        directory = root / name
        directory.mkdir()
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        return env | {
            "APP_BASE_PATH": str(directory), "APP_PORT": str(port),
            "APP_CONFIG_CACHE": str(directory / "config.json"),
            "APP_ROUTES_CACHE": str(directory / "routes.json"),
            "PAY_FIXTURE_BARRIER": str(directory),
        }

    def command(current, *tokens):
        completed = subprocess.run([str(binary), "artisan", *tokens], env=current,
                                   capture_output=True, text=True, timeout=45)
        assert completed.returncode == 0, completed.stdout + completed.stderr
        return completed.stdout

    def check_probe(current, count):
        probe = json.loads(command(current, "drivers:pay"))
        assert probe == {"payment": "merchant-original:order-1", "orders": count,
                         "notifications": count}, probe

    def callback(current, status, *, invalid=False, tampered=False, rollback=False, hold=False,
                 observe=False, merchant="merchant-original", outcome=None):
        connection = http.client.HTTPConnection("127.0.0.1", int(current["APP_PORT"]), timeout=18)
        try:
            payload = f"{merchant}:order-1:fx-tx:150".encode()
            signature = "0" * 64 if invalid else hmac.digest(FIXTURE_KEY, payload, "sha256").hex()
            headers = {"Accept": "application/json", "Content-Type": "application/octet-stream",
                       "X-Fixture-Auth": signature}
            if rollback:
                headers["X-Fixture-Rollback"] = "1"
            if hold:
                headers["X-Fixture-Hold"] = "1"
            if observe:
                headers["X-Fixture-Observe"] = "1"
            if tampered:
                payload += b"\n"
            connection.request("POST", "/api/drivers/pay", payload, headers)
            response = connection.getresponse()
            body = response.read()
            assert response.status == status, (status, response.status, body[:200])
            if outcome:
                assert response.headers["X-Fixture-Outcome"] == outcome
                assert response.headers["Content-Type"] == "text/plain" and body == b"accepted by fixture"
        finally:
            connection.close()

    @contextmanager
    def serve(current):
        log_path = Path(current["APP_BASE_PATH"]) / "http.log"
        with log_path.open("a+") as log:
            process = HTTP["serve"](binary, current, log)
            try:
                yield process
            finally:
                if process.poll() is None:
                    HTTP["stop"](process)

    def wait_for(path):
        deadline = time.monotonic() + 5
        while not path.is_file():
            assert time.monotonic() < deadline, path
            time.sleep(.02)

    first, second = node("node-a"), node("node-b")
    command(first, "migrate", "--force")
    with serve(first):
        callback(first, 400, invalid=True)
        callback(first, 400, tampered=True)
        callback(first, 500, rollback=True)
        check_probe(second, 0)

    for current in (first, second):
        command(current, "config:cache")
        command(current, "route:cache")
        assert Path(current["APP_CONFIG_CACHE"]).stat().st_size
        assert Path(current["APP_ROUTES_CACHE"]).stat().st_size
    first, second = (current | {"PAY_FIXTURE_MERCHANT": "unavailable-from-cache"}
                     for current in (first, second))
    check_probe(second, 0)

    if driver == "sqlite":
        with serve(first):
            callback(first, 400, merchant="unavailable-from-cache")
            callback(first, 202, outcome="processed")
            callback(first, 202, outcome="duplicate")
    else:
        a_path, b_path = map(Path, (first["PAY_FIXTURE_BARRIER"], second["PAY_FIXTURE_BARRIER"]))
        entered, release = a_path / "entered", a_path / "release"
        with serve(second), ThreadPoolExecutor(max_workers=2) as pool:
            with serve(first) as process:
                crashed = pool.submit(callback, first, 202, hold=True)
                wait_for(entered)
                check_probe(second, 0)
                process.kill()
                assert process.wait(timeout=5) != 0
                with suppress(http.client.RemoteDisconnected, ConnectionResetError):
                    crashed.result(timeout=5)
                    raise AssertionError("Killed receiver unexpectedly acknowledged the callback")
            check_probe(second, 0)
            entered.unlink()
            with serve(first):
                primary = pool.submit(callback, first, 202, hold=True, outcome="processed")
                wait_for(entered)
                duplicate = pool.submit(callback, second, 202, observe=True, outcome="duplicate")
                try:
                    wait_for(b_path / "verified")
                    try:
                        duplicate.result(timeout=.2)
                    except TimeoutError:
                        pass
                    else:
                        raise AssertionError("Duplicate acknowledged before the original committed")
                    check_probe(second, 0)
                finally:
                    release.touch()
                primary.result(timeout=10)
                duplicate.result(timeout=10)
            callback(second, 400, merchant="unavailable-from-cache")
        print(f"{driver}: killed receiver rolled back; another node waited at the lock and acknowledged only after commit")

    check_probe(second, 1)
    receipt = Path(second["APP_BASE_PATH"]) / "worker.json"
    command(first, "drivers:pay", "--queue-output", str(receipt))
    assert not receipt.exists()
    command(second, "queue:work", "database", "--queue=pay-probe", "--once")
    assert json.loads(receipt.read_text()) == {
        "payment": "merchant-original:order-1", "orders": 1, "notifications": 1,
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    driver, database_url = DATABASE["database_settings"]()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="pay-driver-") as directory:
        exercise(args.binary.resolve(), driver, database_url, Path(directory))
    print(f"{driver}: HMAC 回调、独立 Artisan、事务回滚、回执／去重、缓存及独立 Worker 通过")
