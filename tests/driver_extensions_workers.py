"""复用短信公开自定义工厂夹具，验证独立 Worker 不依赖派发端注册或凭据。"""
import argparse
import os
from pathlib import Path
from queue import Queue
import runpy
import secrets
import subprocess
import tempfile
from threading import Thread
import time

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument("binary", type=Path)
args = parser.parse_args()
api = runpy.run_path(str(root / "tests/sms_worker.py"))


def docker(*arguments):
    result = subprocess.run(["docker", *arguments], capture_output=True, text=True, timeout=30)
    assert result.returncode == 0, result.stderr
    return result.stdout.strip()


def main():
    driver, url = api["database_settings"]()
    container = "driver-extensions-redis-" + secrets.token_hex(6)
    docker("run", "--rm", "--detach", "--name", container,
           "--label", "org.openrust.acceptance.scope=driver-extensions", "--tmpfs", "/data:rw,size=128m",
           "--publish", "127.0.0.1::6379", "redis:8")
    try:
        deadline = time.monotonic() + 20
        while True:
            if docker("exec", container, "redis-cli", "PING") == "PONG":
                break
            assert time.monotonic() < deadline
            time.sleep(.1)
        info = docker("exec",container,"redis-cli","INFO","server")
        assert "redis_version:8." in info, info
        port = docker("port",container,"6379/tcp").rsplit(":",1)[1]
        os.environ.update(SMS_DEMO_REDIS_CONTAINER=container, SMS_DEMO_REDIS_PORT=port)
        requests = Queue()
        with tempfile.TemporaryDirectory(dir=root / "target/tmp", prefix="driver-worker-") as temp:
            if driver == "sqlite":
                os.environ["OPENRUST_SMS_SQLITE_PATH"] = str(Path(temp)/"database.sqlite")
            env = os.environ | {"APP_BASE_PATH":temp, "APP_ENV":"testing", "DB_CONNECTION":driver,
                "CACHE_STORE": {"sqlite":"file", "pgsql":"database", "mysql":"redis"}[driver],
                "DB_DATABASE":os.environ.get("OPENRUST_SMS_SQLITE_PATH","openrust_acceptance"),
                "REDIS_HOST":"127.0.0.1", "REDIS_PORT":port, "REDIS_DB":"0"}
            if url:
                env["DB_URL"] = url
            else:
                env.pop("DB_URL",None)
            with api["MAKE_SERVER"](0, lambda *receipt: requests.put(receipt)) as server:
                thread = Thread(target=server.serve_forever)
                thread.start()
                env |= {"SMS_ALPHA_URL":f"http://127.0.0.1:{server.server_port}/alpha",
                        "SMS_BETA_URL":f"http://127.0.0.1:{server.server_port}/beta"}
                try:
                    for backend in ("database", "redis"):
                        api["check_backend"](str(args.binary.resolve()),backend,env,requests)
                finally:
                    server.shutdown()
                    thread.join()
            print(f"自定义短信 Provider: {driver}／{env['CACHE_STORE']}，database＋Redis 队列独立 Worker 通过",flush=True)
    finally:
        docker("rm","--force",container)

if __name__ == "__main__":
    main()
