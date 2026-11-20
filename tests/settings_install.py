"""Verify the settings-enabled application against an isolated SQLite database."""

import argparse
import json
import os
from pathlib import Path
import signal
import socket
import sqlite3
import subprocess
import tempfile
import time
import urllib.request


def run(binary, enabled=True):
    with tempfile.TemporaryDirectory(prefix="openrust-settings-install-") as directory:
        root = Path(directory)
        database = root / "database.sqlite"
        database.touch()
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        env = dict(os.environ, APP_BASE_PATH=str(root), APP_ENV="testing",
                   APP_KEY="a" * 32, APP_DEBUG="false", APP_HOST="127.0.0.1",
                   APP_PORT=str(port), DB_CONNECTION="sqlite", DB_DATABASE=str(database),
                   DB_URL="", DB_PREFIX="", CACHE_STORE="array", SESSION_DRIVER="array",
                   QUEUE_CONNECTION="sync", APP_CONFIG_CACHE=str(root / "config.json"),
                   APP_PACKAGES_CACHE=str(root / "packages.json"))

        def artisan(*args):
            result = subprocess.run([str(binary), "artisan", *args], env=env,
                                    capture_output=True, text=True, timeout=30)
            assert result.returncode == 0, f"Artisan {args[0]} failed: {result.stderr}"
            return result.stdout

        artisan("package:discover")
        packages = json.loads((root / "packages.json").read_text())
        assert ("openrust-settings" in {package["name"] for package in packages}) == enabled
        artisan("list")
        with sqlite3.connect(database) as connection:
            assert connection.execute("select name from sqlite_master where name='settings'").fetchone() is None
        artisan("migrate", "--force")
        if not enabled:
            with sqlite3.connect(database) as connection:
                assert connection.execute("select name from sqlite_master where name='settings'").fetchone() is None
            print("settings disabled: package discovery and migrations leave settings absent PASS")
            return
        with sqlite3.connect(database) as connection:
            columns = {row[1] for row in connection.execute("pragma table_info(settings)")}
            assert columns == {"id", "group", "name", "locked", "payload", "created_at", "updated_at"}
            assert connection.execute("select count(*) from settings").fetchone()[0] == 0
        assert "create_settings_table" in artisan("migrate:status")
        with sqlite3.connect(database) as connection:
            connection.execute(
                'insert into settings ("group", name, locked, payload) values (?, ?, ?, ?)',
                ("site", "name", 0, json.dumps("Administrator website")))
        artisan("migrate", "--force")
        with sqlite3.connect(database) as connection:
            stored = connection.execute(
                'select payload from settings where "group"=? and name=?',
                ("site", "name")).fetchone()
            assert json.loads(stored[0]) == "Administrator website"
        artisan("config:cache")
        cached = json.loads((root / "config.json").read_text())
        assert "__settings_version" not in json.dumps(cached)
        assert "Administrator website" not in json.dumps(cached)
        with (root / "http.log").open("w") as log:
            process = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 15
                while True:
                    assert process.poll() is None, "HTTP process exited before readiness"
                    try:
                        with urllib.request.urlopen(f"http://127.0.0.1:{port}/up", timeout=1) as response:
                            assert response.status == 200
                        break
                    except (OSError, TimeoutError):
                        assert time.monotonic() < deadline, "HTTP readiness timed out"
                        time.sleep(0.1)
            finally:
                process.send_signal(signal.SIGINT)
                process.wait(timeout=15)
        artisan("config:clear")
        artisan("migrate:rollback", "--force")
        with sqlite3.connect(database) as connection:
            assert connection.execute("select name from sqlite_master where name='settings'").fetchone() is None
        artisan("migrate", "--force")
        print("settings application: lazy boot, migration/retry/preserved values/rollback, config cache and real HTTP PASS")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--without-settings", action="store_true")
    args = parser.parse_args()
    run(args.binary.resolve(), enabled=not args.without_settings)
