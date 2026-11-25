"""Exercise built-in HTTP exception authentication through a real Pail command."""
import base64
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request


def run():
    work = Path(__file__).resolve().parents[1]
    binary = Path(os.environ.get("CARGO_TARGET_DIR", work / "target")) / "debug/main"
    if os.name == "nt":
        binary = binary.with_suffix(".exe")
    with tempfile.TemporaryDirectory(prefix="pail-auth-main-") as directory:
        root = Path(directory)
        for path in ["public", "resources/views", "storage/app/public"]:
            (root / path).mkdir(parents=True)
        env = dict(os.environ, APP_BASE_PATH=str(root), APP_ENV="production", APP_DEBUG="false",
                   APP_KEY="base64:" + base64.b64encode(b"a" * 32).decode(),
                   APP_DISABLED_RUNNERS="queue,schedule,reverb,grpc", SESSION_DRIVER="array",
                   CACHE_STORE="array", LOG_CHANNEL="single", NO_COLOR="1")
        for key in ["APP_CONFIG_CACHE", "APP_PACKAGES_CACHE", "APP_ROUTES_CACHE", "APP_EVENTS_CACHE", "APP_VIEW_CACHE", "VAPOR_SSM_PATH", "PAIL_TESTS"]:
            env.pop(key, None)
        if "OPENRUST_TEST_MYSQL_URL" in env:
            mysql = urllib.parse.urlsplit(env["OPENRUST_TEST_MYSQL_URL"])
            env.update(DB_CONNECTION="mysql", DB_HOST=mysql.hostname, DB_PORT=str(mysql.port),
                       DB_USERNAME=urllib.parse.unquote(mysql.username), DB_PASSWORD=urllib.parse.unquote(mysql.password),
                       DB_DATABASE=mysql.path.lstrip("/"))
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]
        env["APP_PORT"] = env["SERVER_PORT"] = str(port)
        output = root / "pail-output.log"
        with output.open("w+") as pail_log, (root / "server.log").open("w+") as server_log:
            pail = subprocess.Popen([str(binary), "artisan", "pail", "--timeout=8"], cwd=work, env=env,
                                    stdout=pail_log, stderr=subprocess.STDOUT)
            server = None
            try:
                end = time.monotonic() + 5
                while not list((root / "storage/pail").glob("*.pail")):
                    assert pail.poll() is None, output.read_text(encoding="utf-8")
                    assert time.monotonic() < end, "Pail session not created"
                    time.sleep(.02)
                session = next((root / "storage/pail").glob("*.pail"))
                server = subprocess.Popen(
                    [str(binary)], cwd=work, env=env, stdout=server_log, stderr=subprocess.STDOUT,
                    creationflags=subprocess.CREATE_NEW_CONSOLE if os.name == "nt" else 0,
                )
                def request():
                    try:
                        with urllib.request.urlopen(f"http://127.0.0.1:{port}/", timeout=2) as response:
                            return response.status, response.read().decode()
                    except urllib.error.HTTPError as error:
                        return error.code, error.read().decode()
                end = time.monotonic() + 10
                while True:
                    try:
                        status, body = request()
                        break
                    except urllib.error.URLError:
                        assert server.poll() is None, (root / "server.log").read_text(encoding="utf-8")
                        assert time.monotonic() < end
                        time.sleep(.02)
                assert status == 500, (status, body)
                records = [json.loads(line) for line in session.read_text(encoding="utf-8").splitlines()]
                event = next(record for record in records if record["context"].get("status") == 500)
                origin = event["context"]["__pail"]["origin"]
                assert origin["type"] == "http" and origin["method"] == "GET" and origin["path"] == "/", origin
                assert origin["auth_id"] is None and origin["auth_email"] is None, origin
                assert "exception" in event["context"]
                (root / "resources/views/welcome.blade.html").write_text("recovered", encoding="utf-8")
                assert request() == (200, "recovered")
                if os.name == "nt":
                    subprocess.run(
                        [sys.executable, "-B", str(Path(__file__).with_name("artisan_dev_windows.py")),
                         "interrupt", str(server.pid)], check=True, timeout=15,
                        creationflags=subprocess.DETACHED_PROCESS,
                    )
                else:
                    server.send_signal(signal.SIGINT)
                assert server.wait(timeout=10) == 0
                pail.wait(timeout=12)
                assert pail.returncode == 0, output.read_text(encoding="utf-8")
                assert not session.exists(), "Pail left its session"
                assert "GET" in output.read_text(encoding="utf-8") and "Maximum execution time exceeded." in output.read_text(encoding="utf-8"), output.read_text(encoding="utf-8")
                inspire = subprocess.run([str(binary), "artisan", "inspire"], cwd=work, env=env,
                                         capture_output=True, timeout=10)
                assert inspire.returncode == 0 and inspire.stdout.strip()
                with socket.socket() as probe:
                    assert probe.connect_ex(("127.0.0.1", port)) != 0
                print("actual main HTTP exception -> awaited guest authentication -> Pail output; recovery, timeout/session cleanup, SIGINT and independent Artisan PASS")
            finally:
                for process in [server, pail]:
                    if process is not None and process.poll() is None:
                        process.terminate()
                        try:
                            process.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            process.kill()
                            process.wait()


if __name__ == "__main__":
    run()
