"""外部 Pay 驱动经真实 HTTP 和独立 Artisan 处理通知，覆盖配置缓存及业务回滚。"""

import argparse
import http.client
import json
import os
from pathlib import Path
import runpy
import socket
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
DATABASE = runpy.run_path(str(ROOT / "tests/sms_worker.py"))
HTTP = runpy.run_path(str(ROOT / "tests/webhooks_acceptance.py"))


def exercise(binary, driver, database_url, root):
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    sqlite = root / "fixture.sqlite"
    if driver == "sqlite":
        sqlite.touch()
    env = os.environ | {
        "APP_BASE_PATH": str(root), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "DB_CONNECTION": driver,
        "DB_DATABASE": str(sqlite) if driver == "sqlite" else "openrust_acceptance",
        "PAY_FIXTURE_MERCHANT": "merchant-original",
    }
    if database_url:
        env["DB_URL"] = database_url
    else:
        env.pop("DB_URL", None)

    def command(current, *tokens):
        completed = subprocess.run([str(binary), "artisan", *tokens], env=current,
                                   capture_output=True, text=True, timeout=45)
        assert completed.returncode == 0, completed.stdout + completed.stderr
        return completed.stdout

    def check_probe(current, count):
        probe = json.loads(command(current, "drivers:pay"))
        assert probe == {"payment": "merchant-original:order-1", "orders": count,
                         "notifications": count}, probe

    def callback(status, *, auth="valid", rollback=False, merchant="merchant-original", outcome=None):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            headers = {"Accept": "application/json", "Content-Type": "application/octet-stream",
                       "X-Fixture-Auth": auth}
            if rollback:
                headers["X-Fixture-Rollback"] = "1"
            connection.request("POST", "/api/drivers/pay", f"{merchant}:order-1:fx-tx:150", headers)
            response = connection.getresponse()
            body = response.read()
            assert response.status == status, (status, response.status, body[:200])
            if outcome:
                assert response.headers["X-Fixture-Outcome"] == outcome
                assert response.headers["Content-Type"] == "text/plain" and body == b"accepted by fixture"
        finally:
            connection.close()

    command(env, "migrate", "--force")
    check_probe(env, 0)
    with (root / "http.log").open("w+") as log:
        process = HTTP["serve"](binary, env, log)
        try:
            callback(400, auth="invalid")
            callback(500, rollback=True)
            check_probe(env, 0)
            callback(202, outcome="processed")
            callback(202, outcome="duplicate")
            check_probe(env, 1)
        finally:
            HTTP["stop"](process)

        command(env, "config:cache")
        command(env, "route:cache")
        cached = env | {"PAY_FIXTURE_MERCHANT": "unavailable-from-cache"}
        check_probe(cached, 1)
        process = HTTP["serve"](binary, cached, log)
        try:
            callback(400, merchant="unavailable-from-cache")
            callback(202, outcome="duplicate")
            check_probe(cached, 1)
        finally:
            HTTP["stop"](process)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    driver, database_url = DATABASE["database_settings"]()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="pay-driver-") as directory:
        exercise(args.binary.resolve(), driver, database_url, Path(directory))
    print(f"{driver}: 外部 Pay 驱动通过真实 HTTP、独立 Artisan、事务回滚、回执／去重及配置／路由缓存")
