"""Verify Pail's default dev process against a compiled binary and real MySQL 8.4."""

import base64
import secrets
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

from artisan_dev_terminal import Terminal, available_port


def wait(check, description, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if check():
            return
        time.sleep(0.03)
    raise AssertionError(description)


def request(port, path):
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}{path}", timeout=0.5) as response:
            return response.status
    except urllib.error.HTTPError as error:
        return error.code
    except OSError:
        return None


def sessions(root):
    return list((root / "storage/pail").glob("*.pail"))


def captured_http_error(root):
    for path in sessions(root):
        for line in path.read_text().splitlines():
            try:
                record = json.loads(line)
            except json.JSONDecodeError:
                continue
            origin = record.get("context", {}).get("__pail", {}).get("origin", {})
            if origin.get("type") == "http" and origin.get("path") == "/":
                return record
    return None


def descendants(pid):
    path = Path(f"/proc/{pid}/task/{pid}/children")
    try:
        children = [int(value) for value in path.read_text().split()]
    except FileNotFoundError:
        return []
    return children + [child for parent in children for child in descendants(parent)]


def alive(pid):
    try:
        status = Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1]
    except FileNotFoundError:
        return False
    return status.split()[0] != "Z"


def assert_stopped(root, port, pids):
    assert not sessions(root), "Pail left an owned session"
    assert all(not alive(pid) for pid in pids), "dev descendants survived shutdown"
    with socket.socket() as probe:
        assert probe.connect_ex(("127.0.0.1", port)) != 0, "HTTP port stayed bound"


def json_mode(binary, root, env):
    port = available_port()
    env = dict(env, SERVER_PORT=str(port), APP_PORT=str(port))
    process = subprocess.Popen([binary, "artisan", "dev", "--json"], env=env,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               start_new_session=True)
    events = []
    errors = bytearray()

    def consume():
        for line in process.stdout:
            events.append(json.loads(line))

    reader = threading.Thread(target=consume)
    error_reader = threading.Thread(target=lambda: errors.extend(process.stderr.read()))
    reader.start()
    error_reader.start()
    try:
        wait(lambda: request(port, "/up") == 200, "dev HTTP did not start")
        wait(lambda: len(sessions(root)) == 1, "default logs process did not create a session")
        # Isolated resources/views has no welcome view: exercise the real HTTP error reporter.
        assert request(port, "/") == 500
        wait(lambda: captured_http_error(root), "HTTP report did not reach Pail")
        record = captured_http_error(root)
        assert "View not found." in json.dumps(record["context"]["exception"]), record
        wait(lambda: any(e.get("label") == "logs" and record["message"] in e.get("text", "")
                         for e in events), "Pail did not stream the HTTP report through dev")
        started = {e["label"]: e for e in events if e["type"] == "start"}
        assert set(started) == {"server", "queue", "logs"}, started
        assert started["logs"]["command"] == "./artisan pail --timeout=0"
        assert not descendants(started["logs"]["pid"]), "Pail unexpectedly spawned tail or Cargo"
        pids = descendants(process.pid)
        assert len(pids) >= 4, pids  # server supervisor + HTTP, worker, Pail
        process.send_signal(signal.SIGTERM)
        assert process.wait(timeout=12) == 0, bytes(errors)
        reader.join(timeout=5)
        error_reader.join(timeout=5)
        assert not reader.is_alive() and not error_reader.is_alive()
        assert events[-1]["type"] == "done" and events[-1]["code"] == 0
        assert not any(e["type"] in {"failed", "error"} for e in events), events
        assert_stopped(root, port, pids)
        print("MySQL 8.4 dev JSON: default server/queue/logs, real HTTP report, no tail child, SIGTERM cleanup PASS", flush=True)
    finally:
        if process.poll() is None:
            for pid in reversed(descendants(process.pid)):
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            process.kill()
            process.wait()
        reader.join(timeout=5)
        error_reader.join(timeout=5)


def terminal_mode(binary, root, env, mode):
    port = available_port()
    env = dict(env, SERVER_PORT=str(port), APP_PORT=str(port))
    with Terminal([binary, "artisan", "dev", mode], env, 24) as terminal:
        terminal.expect(lambda: request(port, "/up") == 200 and len(sessions(root)) == 1,
                        "dev HTTP/Pail did not start")
        assert request(port, "/") == 500
        terminal.expect(lambda: captured_http_error(root), "HTTP exception was not captured")
        if mode == "--tabs":
            terminal.send(b"3")
        record = captured_http_error(root)
        assert "View not found." in json.dumps(record["context"]["exception"]), record
        terminal.expect(lambda: record["message"].encode() in terminal.output, "logs did not display the HTTP report")
        pids = descendants(terminal.process.pid)
        terminal.send(b"\x03")
        terminal.finish()
        assert_stopped(root, port, pids)
    print(f"MySQL 8.4 dev {mode}: real HTTP -> logs, keyboard Ctrl+C, terminal restoration and process cleanup PASS", flush=True)


def main():
    if sys.platform != "linux":
        raise SystemExit("This process-tree acceptance requires Linux; Windows remains in the release matrix")
    parsed = urllib.parse.urlsplit(os.environ["OPENRUST_TEST_MYSQL_URL"])
    assert parsed.scheme == "mysql" and os.environ.get("OPENRUST_REQUIRE_MYSQL") == "1"
    binary = str(Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/main").resolve())
    with tempfile.TemporaryDirectory(prefix="pail-dev-mysql-") as directory:
        root = Path(directory)
        for path in ["public", "resources/views", "storage/app/public"]:
            (root / path).mkdir(parents=True)
        env = dict(os.environ, APP_BASE_PATH=str(root), APP_ENV="local", APP_DEBUG="false",
                   APP_KEY="base64:" + base64.b64encode(secrets.token_bytes(32)).decode(),
                   DB_CONNECTION="mysql", DB_HOST=parsed.hostname, DB_PORT=str(parsed.port),
                   DB_USERNAME=urllib.parse.unquote(parsed.username), DB_PASSWORD=urllib.parse.unquote(parsed.password),
                   DB_DATABASE=parsed.path.lstrip("/"), QUEUE_CONNECTION="database", LOG_CHANNEL="single",
                   CACHE_STORE="array", SESSION_DRIVER="array", TERM="xterm-256color")
        for key in ["APP_CONFIG_CACHE", "APP_PACKAGES_CACHE", "PAIL_TESTS", "VAPOR_SSM_PATH", "NO_COLOR", "FORCE_COLOR"]:
            env.pop(key, None)
        listed = subprocess.run([binary, "artisan", "dev:list", "--json"], env=env,
                                capture_output=True, timeout=15, check=True)
        assert [item["name"] for item in json.loads(listed.stdout)] == ["server", "queue", "logs"]
        assert not sessions(root)
        subprocess.run([binary, "artisan", "migrate", "--force"], env=env,
                       check=True, capture_output=True, timeout=30)
        json_mode(binary, root, env)
        terminal_mode(binary, root, env, "--tabs")
        terminal_mode(binary, root, env, "--stream")
        result = subprocess.run([binary, "artisan", "inspire"], env=env,
                                check=True, capture_output=True, timeout=15)
        assert result.stdout.strip()
        print("Independent Artisan inspire after dev shutdown PASS", flush=True)


if __name__ == "__main__":
    main()
