"""验证独立 HTTP、Artisan 与数据库队列 Worker 的默认词条来源。"""
import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import socket
import subprocess
import time
from urllib.error import HTTPError, URLError
from urllib.request import urlopen


def execute(args):
    project = Path(__file__).resolve().parents[1]
    stage = args.stage.resolve()
    assert stage.is_relative_to(project / "target/tmp")
    stage.mkdir(parents=True, exist_ok=False)
    (stage / "lang/fr").mkdir(parents=True)
    (stage / "lang/en").mkdir()
    (stage / "lang/fr/messages.json").write_text(
        '{"title":"File title","only_file":"File only","empty":"File nonempty"}'
    )
    (stage / "lang/en/messages.json").write_text('{"fallback":"Fallback language"}')
    (stage / "lang/fr.json").write_text('{"Fixture literal.":"Phrase fichier."}')
    env = os.environ | {
        "APP_BASE_PATH": str(stage), "APP_ENV": "production", "APP_DEBUG": "false",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "DB_CONNECTION": args.driver, "DB_PREFIX": "", "DB_MASK_BINDINGS": "true",
        "CACHE_STORE": "array", "SESSION_DRIVER": "array", "QUEUE_CONNECTION": "database",
        "APP_DISABLED_RUNNERS": "queue,schedule,reverb,grpc", "LOG_CHANNEL": "null",
        "APP_CONFIG_CACHE": str(stage / "config.json"),
    }
    env.pop("DB_URL", None)
    if args.driver == "sqlite":
        database = stage / "localization.sqlite"
        database.touch()
        env["DB_DATABASE"] = str(database)
    else:
        provider = {"pgsql": "POSTGRES", "mysql": "MYSQL"}[args.driver]
        assert env[f"OPENRUST_REQUIRE_{provider}"] == "1"
        assert env[f"OPENRUST_ACCEPTANCE_{provider}_CONTAINER"]
        env["DB_URL"] = env[f"OPENRUST_TEST_{provider}_URL"]

    binary = args.binary.resolve()
    off = {"L10N_ENABLED": "0", "APP_CONFIG_CACHE": str(stage / "uncached.json")}
    on = {"L10N_ENABLED": "1"}

    def artisan(*command, extra=None, status=0):
        result = subprocess.run(
            [str(binary), "artisan", "--no-ansi", *command], cwd=stage,
            env=env | (extra or off), capture_output=True, text=True, timeout=45,
        )
        assert result.returncode == status, (command, result.stdout, result.stderr)
        return result.stdout, result.stderr

    @contextmanager
    def server():
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]
        server_env = env | on | {
            "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "L10N_FIXTURE_HTTP": "1",
        }
        with (stage / "http.log").open("w") as logfile:
            process = subprocess.Popen([str(binary)], cwd=stage, env=server_env, stdout=logfile, stderr=logfile)
            try:
                base = f"http://127.0.0.1:{port}"
                for _ in range(150):
                    try:
                        with urlopen(base + "/api/localization/show", timeout=2):
                            break
                    except URLError:
                        assert process.poll() is None, "应用启动失败"
                        time.sleep(.1)
                else:
                    raise AssertionError("等待应用监听超时")
                yield base + "/api/localization/"
            finally:
                process.terminate()
                try:
                    process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()

    def get(url):
        with urlopen(url, timeout=10) as response:
            return json.load(response)

    def expected(title):
        return {
            "title": title, "only_file": "File only", "fallback": "Fallback language",
            "empty": "File nonempty" if title == "File title" else "",
            "literal": "Phrase fichier." if title == "File title" else "Phrase SQL.",
        }

    assert json.loads(artisan("localization:test")[0]) == expected("File title")
    _, error = artisan("localization:test", extra=on, status=1)
    assert "无法加载应用词条" in error

    artisan("vendor:publish", "--tag=localization-database-migrations")
    migration, = (stage / "database/migrations").glob("*_create_language_lines_table.rs")
    assert migration.read_bytes() == (
        project / "vendor/openrust-localization-database/stubs/create_language_lines_table.stub"
    ).read_bytes()
    artisan("vendor:publish", "--tag=localization-database-migrations")
    assert tuple(migration.parent.glob("*_create_language_lines_table.rs")) == (migration,)
    artisan("migrate", "--force")
    assert migration.stem in artisan("migrate:status")[0]
    artisan("lang:publish")
    assert (stage / "lang/fr/validation.json").is_file()
    artisan("localization:test", "seed", "SQL first")
    assert json.loads(artisan("localization:test", extra=on)[0]) == expected("SQL first")

    with server() as base:
        assert get(base + "show") == expected("SQL first")
        artisan("localization:test", "invalid", "Not published")
        try:
            get(base + "refresh")
            raise AssertionError("invalid source did not fail")
        except HTTPError as error:
            assert error.code == 500
        assert get(base + "show") == expected("SQL first")
        artisan("localization:test", "clean")
        assert get(base + "refresh") == expected("File title")
        artisan("localization:test", "seed", "SQL second")
        assert get(base + "show") == expected("File title")
        assert get(base + "refresh") == expected("SQL second")

    artisan("config:cache", extra=on)
    config = (stage / "config.json").read_text()
    assert '"resources"' in config and "SQL second" not in config
    artisan("localization:test", "seed", "SQL third")
    cached = {"L10N_ENABLED": "0"}
    assert json.loads(artisan("localization:test", extra=cached)[0]) == expected("SQL third")
    receipt = stage / "worker.json"
    artisan("localization:test", "queue", str(receipt), extra=cached)
    artisan("queue:work", "database", "--stop-when-empty", "--sleep=0", extra=cached)
    assert json.loads(receipt.read_text()) == expected("SQL third")
    assert json.loads(artisan("localization:test")[0]) == expected("File title")
    print(f"{args.driver}: publication, migration, boot failure, HTTP refresh, Artisan, cached Worker PASS")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--stage", required=True, type=Path)
    parser.add_argument("--driver", required=True, choices=["sqlite", "mysql", "pgsql"])
    execute(parser.parse_args())
