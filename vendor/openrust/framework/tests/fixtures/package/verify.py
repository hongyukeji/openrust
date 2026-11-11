"""Run the installed package through a real HTTP listener and Artisan."""

import json
import os
from pathlib import Path
import select
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from urllib.error import HTTPError
from urllib.request import urlopen


def write(root, name, text):
    path = root / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def artisan(binary, root, env, *arguments):
    return subprocess.run(
        [binary, "artisan", *arguments],
        cwd=root,
        env=env,
        capture_output=True,
        check=True,
        text=True,
        timeout=20,
    ).stdout


def response(base_url, path):
    try:
        with urlopen(base_url + path, timeout=5) as result:
            return result.status, result.read().decode()
    except HTTPError as error:
        with error:
            return error.code, error.read().decode()


def check_http(binary, root, env, installed):
    with subprocess.Popen(
        [binary], cwd=root, env=env, stdout=subprocess.PIPE, text=True
    ) as server:
        try:
            if not select.select([server.stdout], [], [], 10)[0]:
                raise RuntimeError("Package host did not start")
            prefix = "OpenRust listening on "
            line = server.stdout.readline()
            assert line.startswith(prefix), line
            base_url = line.removeprefix(prefix).strip()
            assert response(base_url, "/health") == (200, "ready")
            if installed:
                assert response(base_url, "/package") == (200, "App welcome")
                status, body = response(base_url, "/package/fallback")
                assert status == 200 and body.strip() == "Package fallback"
                status, body = response(base_url, "/package/messages")
                assert status == 200
                assert json.loads(body) == {
                    "title": "App title",
                    "fallback": "Package fallback",
                }
            else:
                assert response(base_url, "/package")[0] == 404
        finally:
            if server.poll() is None:
                server.terminate()
                server.wait(timeout=10)


def check_sqlite(binary, root, env):
    database = root / "probe.sqlite"
    database.touch()
    assert "0001_create_package_probe" in artisan(binary, root, env, "migrate", "--force")
    with sqlite3.connect(database) as connection:
        connection.execute("INSERT INTO package_probe DEFAULT VALUES")
        assert connection.execute("SELECT COUNT(*) FROM package_probe").fetchone() == (1,)
    assert "0001_create_package_probe" in artisan(binary, root, env, "migrate:rollback", "--force")
    with sqlite3.connect(database) as connection:
        table = connection.execute(
            "SELECT name FROM sqlite_master WHERE name = 'package_probe'"
        ).fetchone()
        assert table is None


def main():
    binary = Path(sys.argv[1]).resolve()
    fixture = Path(__file__).resolve().parent
    env = os.environ.copy()
    mysql_url = env.pop("PROBE_MYSQL_URL", None)
    env.pop("PROBE_RESOURCES", None)
    with tempfile.TemporaryDirectory(prefix="openrust-package-") as directory:
        release = Path(directory)
        assets = release / "resources-from-package"
        shutil.copytree(fixture / "package/resources", assets)
        root = release / "app"
        write(root, "resources/views/vendor/probe/welcome.html", "App welcome")
        write(root, "lang/vendor/probe/en/messages.json", '{"title":"App title"}')
        installed = dict(env, PROBE_RESOURCES=str(assets))
        check_http(binary, root, installed, True)
        assert "Verify the installed package" in artisan(binary, root, installed, "help", "package:probe")
        assert "App title" in artisan(binary, root, installed, "package:probe")
        check_sqlite(binary, root, installed)
        check_http(binary, root, env, False)
        assert "package:probe" not in artisan(binary, root, env, "list")
        if mysql_url:
            mysql = dict(installed, PROBE_MYSQL_URL=mysql_url)
            assert "0001_create_package_probe" in artisan(binary, root, mysql, "migrate", "--force")
            assert "0001_create_package_probe" in artisan(binary, root, mysql, "migrate:rollback", "--force")
    print("Package HTTP, overrides, isolation, Artisan and SQLite passed")
    if mysql_url:
        print("Package MySQL migration and rollback passed")


if __name__ == "__main__":
    main()
