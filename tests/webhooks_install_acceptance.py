"""用正式应用入口验收 Webhook 独立选择、重复初始化和包发布。"""

import argparse
import http.client
import os
from pathlib import Path
import runpy
import socket
import sqlite3
import subprocess
import time
import tomllib


ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument("--source", type=Path, required=True)
parser.add_argument("--initial", type=Path, required=True)
parser.add_argument("--profile", choices=("client", "server", "both"))
parser.add_argument("--publish-migrations", action="store_true")
args = parser.parse_args()
source = args.source.resolve()
assert source.is_relative_to(ROOT / "target/tmp")
assert not (source / ".env").exists()
target = Path(os.environ["CARGO_TARGET_DIR"]).resolve()
initial = args.initial.resolve()
assert initial != target / "debug/main"
helpers = runpy.run_path(str(ROOT / "tests/sms_worker.py"))
driver, url = helpers["database_settings"]()
assert args.profile or driver == "sqlite"
assert not args.publish_migrations or args.profile == "both"


def command(binary, *options, expected=0):
    result = subprocess.run([str(binary), "artisan", *options], cwd=source, env=env,
                            capture_output=True, text=True, timeout=240)
    assert (result.returncode == 0) == (expected == 0), (options, result.stdout, result.stderr)
    return result.stdout


def available_port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def live_http(binary, port, log_path):
    with log_path.open("w+") as log:
        application = subprocess.Popen([str(binary)], cwd=source, env=env, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 20
            while application.poll() is None and time.monotonic() < deadline:
                probe = http.client.HTTPConnection("127.0.0.1", port, timeout=3)
                try:
                    probe.request("GET", "/up")
                    response = probe.getresponse()
                    response.read()
                    assert response.status == 200
                    break
                except ConnectionRefusedError:
                    time.sleep(.05)
                finally:
                    probe.close()
            else:
                log.seek(0)
                raise AssertionError(log.read())
        finally:
            application.terminate()
            try:
                application.wait(timeout=5)
            except subprocess.TimeoutExpired:
                application.kill()
                application.wait(timeout=5)


def database_rows(database, query):
    if driver != "sqlite":
        result = helpers["database_rows"](helpers["database_container"](), query).strip()
        return result.splitlines()
    with sqlite3.connect(database) as connection:
        return [str(value) for value, in connection.execute(query)]


def package_tables(database):
    if driver == "sqlite":
        return set(database_rows(database, "SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'webhook_%'"))
    schema = "DATABASE()" if driver == "mysql" else "current_schema()"
    return set(database_rows(database, "SELECT table_name FROM information_schema.tables "
                                       f"WHERE table_schema={schema} AND table_name LIKE 'webhook_%'"))


def verify_migration_publication(binary, database, port):
    application_dir = source / "database/migrations"
    published = {}
    for package in ("webhook-client", "webhook-server"):
        command(binary, "vendor:publish", f"--tag={package}-migrations")
        vendor_dir = source / "vendor/openrust" / package / "database/migrations"
        for original in vendor_dir.glob("*.rs"):
            destination = application_dir / original.name
            data = original.read_bytes() + b"\npub(crate) use self::MIGRATION as Definition;\n"
            assert destination.read_bytes() == data, destination.name
            published[destination] = data
        if package == "webhook-client":
            server_dir = source / "vendor/openrust/webhook-server/database/migrations"
            assert all(not (application_dir / original.name).exists() for original in server_dir.glob("*.rs"))
    assert len(published) == 30
    custom = application_dir / "2026_11_25_000001_create_webhook_calls_table.rs"
    marker = b'                    table.string("name", 128);'
    assert published[custom].count(marker) == 1
    published[custom] = published[custom].replace(
        marker, marker + b'\n                    table.string("fixture_override", 32).nullable();',
    )
    custom.write_bytes(published[custom])
    for package in ("webhook-client", "webhook-server"):
        command(binary, "vendor:publish", f"--tag={package}-migrations")
    assert all(path.read_bytes() == contents for path, contents in published.items())

    def ledger():
        return database_rows(database, "SELECT migration FROM migrations ORDER BY migration")

    def columns():
        if driver == "sqlite":
            return database_rows(database, "SELECT name FROM pragma_table_info('webhook_calls')")
        schema = "DATABASE()" if driver == "mysql" else "current_schema()"
        return database_rows(database, "SELECT column_name FROM information_schema.columns "
                                       f"WHERE table_schema={schema} AND table_name='webhook_calls'")

    before = ledger()
    assert {path.stem for path in published} <= set(before)
    assert "fixture_override" not in columns()
    command(binary, "migrate", "--force")
    assert ledger() == before
    build = subprocess.run(["cargo", "build", "--locked", "--bin", "main"], cwd=source, env=env,
                           capture_output=True, text=True, timeout=240)
    assert build.returncode == 0, build.stdout + build.stderr
    command(binary, "migrate", "--force")
    assert ledger() == before and "fixture_override" not in columns()

    for package in ("webhook-server", "webhook-client"):
        command(binary, "migrate:rollback", f"--path=vendor/openrust/{package}/database/migrations", "--force")
    assert not package_tables(database)
    assert not ({path.stem for path in published} & set(ledger()))
    command(binary, "migrate", "--path=database/migrations", "--force")
    assert ledger() == before and "fixture_override" in columns()
    command(binary, "config:cache")
    command(binary, "route:cache")
    live_http(binary, port, source.parent / f"{driver}-published-http.log")
    print(f"{driver}: 发布 30 条可编译迁移、保留定制、原账本不重跑、包路径回滚及应用迁移重建通过", flush=True)


manifest = source / "Cargo.toml"
base_manifest = manifest.read_bytes()
base_features = tomllib.loads(base_manifest.decode())["features"].get("default", [])
assert not any(feature.startswith("webhook-") for feature in base_features)
user_file = source / "app/models/user.rs"
custom_model = user_file.read_bytes() + b"\n// existing application model customization\n"
user_file.write_bytes(custom_model)
custom_stub = source / "stubs/webhook_fixture.stub"
custom_stub.parent.mkdir(exist_ok=True)
custom_stub.write_text("existing application stub\n")
base_routes = (source / "routes/web.rs").read_bytes()
configs = {kind: source / "config" / f"webhook_{kind}.rs" for kind in ("client", "server")}
assert all(not path.exists() for path in configs.values())
env = os.environ | {"APP_BASE_PATH": str(source), "APP_DISABLED_RUNNERS": "queue,schedule,reverb,grpc",
                    "APP_CONFIG_CACHE": str(source / f"fixture-{driver}-initial-config.json"),
                    "APP_ROUTES_CACHE": str(source / f"fixture-{driver}-initial-routes.json")}
for key in ("APP_ENV", "DB_URL", "DB_DATABASE"):
    env.pop(key, None)

for kind in ("client", "server"):
    command(initial, "config:show", f"webhook-{kind}", expected=1)

profiles = {"client": ("client",), "server": ("server",), "both": ("client", "server")}
for selected in (profiles[args.profile],) if args.profile else profiles.values():
    label = "-".join(selected)
    env.update(APP_CONFIG_CACHE=str(source / f"fixture-{driver}-{label}-config.json"),
               APP_ROUTES_CACHE=str(source / f"fixture-{driver}-{label}-routes.json"))
    manifest.write_bytes(base_manifest)
    for path in configs.values():
        path.unlink(missing_ok=True)
    port = available_port()
    database = source / f"{label}.sqlite" if driver == "sqlite" else "openrust_acceptance"
    if driver == "sqlite":
        database.touch()
    env_path = source / ".env"
    env_path.write_text(
        f"APP_ENV=testing\nAPP_KEY=\nAPP_HOST=127.0.0.1\nAPP_PORT={port}\nAPP_URL=http://localhost:{port}\n"
        f'DB_CONNECTION={driver}\nDB_DATABASE="{database}"\nCACHE_STORE=file\nSESSION_DRIVER=array\n'
        + (f'DB_URL="{url}"\n' if url else "") +
        "QUEUE_CONNECTION=database\nLOG_CHANNEL=stderr\nLOG_LEVEL=error\n"
    )
    env_path.chmod(0o600)
    output = command(initial, "init", "--extensions=all", "--dry-run", "--no-interaction")
    assert "Selected extensions:" in output and "Webhook 接收" not in output and "Webhook 发送" not in output
    selected_features = [f"webhook-{kind}" for kind in selected]
    selector = "--extensions=" + ",".join(selected_features)
    before = env_path.read_bytes()
    output = command(initial, "init", selector, "--dry-run", "--no-interaction")
    assert f"Selected extensions: {','.join(selected_features)}." in output
    assert manifest.read_bytes() == base_manifest and env_path.read_bytes() == before
    assert not package_tables(database)

    command(initial, "init", selector, "--no-interaction")
    binary = target / "debug/main"
    defaults = tomllib.loads(manifest.read_text())["features"]["default"]
    assert sorted(defaults) == sorted(base_features + selected_features), defaults
    key = [line for line in env_path.read_text().splitlines() if line.startswith("APP_KEY=")]
    assert len(key) == 1 and "base64:" in key[0]
    tables = package_tables(database)
    assert ("webhook_calls" in tables) == ("client" in selected)
    assert ("webhook_deliveries" in tables) == ("server" in selected)
    command(binary, "package:discover")
    customized = {}
    for kind in ("client", "server"):
        enabled = kind in selected
        command(binary, "config:show", f"webhook-{kind}", expected=0 if enabled else 1)
        if enabled:
            command(binary, "vendor:publish", f"--tag=webhook-{kind}-config")
            config = configs[kind]
            reference = source / "vendor/openrust" / f"webhook-{kind}" / "config" / config.name
            assert config.read_bytes() == reference.read_bytes()
            customized[config] = reference.read_bytes() + b"\n// existing application configuration\n"
            config.write_bytes(customized[config])
            command(binary, "vendor:publish", f"--tag=webhook-{kind}-config")
    command(binary, "config:cache")
    command(binary, "route:cache")
    live_http(binary, port, source.parent / f"{label}-http.log")
    migration_count = database_rows(database, "SELECT COUNT(*) FROM migrations")

    command(binary, "init", selector, "--no-interaction")
    assert package_tables(database) == tables
    assert key == [line for line in env_path.read_text().splitlines() if line.startswith("APP_KEY=")]
    assert user_file.read_bytes() == custom_model and custom_stub.read_text() == "existing application stub\n"
    assert (source / "routes/web.rs").read_bytes() == base_routes
    assert all(config.read_bytes() == value for config, value in customized.items())
    assert database_rows(database, "SELECT COUNT(*) FROM migrations") == migration_count
    print(f"{driver}/{label}: 原生 init、独立启用、真实 HTTP、Artisan 及重复安装通过", flush=True)
    if args.publish_migrations:
        verify_migration_publication(binary, database, port)
