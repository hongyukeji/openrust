"""Exercise Pulse on a real HTTP stream that the client closes before EOF."""
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time


def wait_for_address(process, log_path):
    deadline = time.monotonic() + 10
    prefix = "Server running on [http://"
    while time.monotonic() < deadline and process.poll() is None:
        for line in log_path.read_text().splitlines():
            if line.startswith(prefix):
                host, port = line.removeprefix(prefix).split("]", 1)[0].rsplit(":", 1)
                return host, int(port)
        time.sleep(0.025)
    raise AssertionError(log_path.read_text())


def records(database):
    with sqlite3.connect(database) as connection:
        return connection.execute(
            "SELECT type, key, value FROM pulse_entries "
            "WHERE type IN ('slow_request', 'slow_user_request', 'user_request') ORDER BY id"
        ).fetchall()


def main():
    binary = str(Path(sys.argv[1]).resolve())
    with tempfile.TemporaryDirectory(prefix="openrust-pulse-transport-") as directory:
        root = Path(directory)
        database = root / "database.sqlite"
        database.touch()
        env = dict(os.environ, APP_BASE_PATH=directory, PULSE_PROBE_TRANSPORT="1")
        subprocess.run([binary, "artisan", "migrate", "--force"], env=env, check=True, timeout=15)
        log_path = root / "server.log"
        with log_path.open("w") as log, subprocess.Popen(
            [binary, "artisan", "serve", "--host=127.0.0.1", "--port=0", "--tries=0"],
            env=env, stdout=log, stderr=subprocess.STDOUT,
        ) as server:
            try:
                address = wait_for_address(server, log_path)
                with socket.create_connection(address, timeout=5) as client:
                    client.sendall(
                        b"GET /api/stream HTTP/1.1\r\nHost: 127.0.0.1\r\n"
                        b"X-Test-Operator: operator\r\nConnection: close\r\n\r\n"
                    )
                    data = bytearray()
                    while b"pulse-stream-started" not in data:
                        chunk = client.recv(4096)
                        assert chunk, data
                        data.extend(chunk)
                    assert data.startswith(b"HTTP/1.1 200 "), data
                    assert b"transfer-encoding: chunked" in data.lower(), data
                    time.sleep(0.15)
                    assert not records(database)
                    client.shutdown(socket.SHUT_RDWR)

                deadline = time.monotonic() + 5
                while time.monotonic() < deadline:
                    observed = records(database)
                    if len(observed) >= 3:
                        break
                    time.sleep(0.025)
                slow = [(json.loads(key), value) for kind, key, value in observed if kind == "slow_request"]
                assert len(slow) == 1 and slow[0][0][:2] == ["GET", "/api/stream"], observed
                assert slow[0][1] >= 150, observed
                assert sorted((kind, key) for kind, key, _ in observed if kind != "slow_request") == [
                    ("slow_user_request", "42"), ("user_request", "42"),
                ], observed
                print("PASS: client disconnect persisted one lifecycle duration and both user counters")
            finally:
                server.terminate()
                try:
                    server.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait(timeout=5)


if __name__ == "__main__":
    main()
