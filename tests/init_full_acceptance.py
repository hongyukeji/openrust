"""验收真正的 init:full、单机访问和两个应用副本。服务由外部夹具提供。"""
import html
import http.cookiejar
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import time
import urllib.error
import urllib.request
import uuid
from contextlib import ExitStack, contextmanager


def port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


@contextmanager
def replica(root, executable, env, bind, label):
    origin = f"http://localhost:{bind}"
    with (root.parent / f"init-full-{label}.log").open("wb") as log:
        process = subprocess.Popen([executable], cwd=root, env=env | {"APP_PORT": str(bind)}, stdout=log, stderr=log)
        try:
            for _ in range(200):
                try:
                    with urllib.request.urlopen(origin + "/up", timeout=2) as response:
                        assert response.status == 200
                    break
                except OSError:
                    assert process.poll() is None, f"{label} 提前退出"
                    time.sleep(0.05)
            else:
                raise AssertionError(f"{label} HTTP 未就绪")
            yield origin
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    assert process.wait(timeout=15) == 0
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                    raise


def request(client, origin, path, status, *, data=None, csrf=None):
    headers = {"Accept": "application/json" if data is not None else "text/html"}
    if csrf:
        headers["X-CSRF-TOKEN"] = csrf
    if data is not None:
        headers["Content-Type"] = "application/json"
    payload = None if data is None else json.dumps(data).encode()
    command = urllib.request.Request(origin + path, data=payload, headers=headers)
    try:
        response = client.open(command, timeout=8)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        body = response.read().decode()
        assert response.status == status, (path, response.status, body[:350])
        return response.url, body


def csrf(body):
    matched = re.search(r'name="_token"\s+value="([^"]+)"', body)
    assert matched, "页面缺少 CSRF 字段"
    return html.unescape(matched.group(1))


def wait_for_artifact(path, process):
    for _ in range(200):
        if path.is_file():
            return
        assert process.poll() is None, f"进程在生成 {path.name} 前退出"
        time.sleep(0.025)
    raise AssertionError(f"等待 {path.name} 超时")


def verify_coordination(root, executable, env, cli):
    artifacts = Path(env["OPENRUST_INIT_TEST_ARTIFACTS"])
    with (artifacts / "coordination.log").open("w") as log:
        lock = subprocess.Popen([executable, "artisan", "init-full-test:migration-lock"], cwd=root, env=env, stdout=log, stderr=log)
        try:
            wait_for_artifact(artifacts / "migration-ready", lock)
            assert "already running" in cli("migrate", "--isolated=12", "--no-interaction", status=12)
            interrupted = cli("init:full", status=None)
            assert "already running" in interrupted and "完整初始化通过" not in interrupted
        finally:
            (artifacts / "migration-release").touch()
            assert lock.wait(timeout=18) == 0
        cli("migrate", "--isolated=12", "--no-interaction")

        cli("init-full-test:enqueue")
        worker = [executable, "artisan", "queue:work", "redis", "--queue=init-full-recovery", "--once", "--tries=2"]
        first = subprocess.Popen(worker, cwd=root, env=env | {"OPENRUST_INIT_TEST_PAUSE_WORKER": "1"}, stdout=log, stderr=log)
        try:
            wait_for_artifact(artifacts / "worker-ready", first)
        finally:
            if first.poll() is None:
                first.kill()
            first.wait(timeout=10)
        assert not (artifacts / "worker-finished").exists()
        time.sleep(3)
        cli(*worker[2:])
        assert (artifacts / "worker-finished").is_file()

        second = int(time.time()) % 60
        if second > 54:
            time.sleep(61 - second)
        cli("schedule:run")
        cli("schedule:run")
        assert (artifacts / "scheduler").read_text() == "tick\n"


def main():
    root = Path(__file__).resolve().parents[1]
    assert root.parent.parent.parent.name == "target", "请从项目 target/tmp 的隔离源码运行"
    target = Path(os.environ["CARGO_TARGET_DIR"])
    executable = target / "debug" / ("main.exe" if os.name == "nt" else "main")
    env_file = root / ".env"
    event_log = root.parent / "init-full-commands.log"
    assert not env_file.exists(), "验收需要干净的应用源码"
    web_port = port()
    signature = "initfull" + uuid.uuid4().hex[:10]
    database_url = os.environ["OPENRUST_TEST_MYSQL_URL"]
    assert os.environ.get("OPENRUST_REQUIRE_MYSQL") == "1"
    env = dict(os.environ, APP_BASE_PATH=str(root), APP_ENV="local", APP_DEBUG="false",
               APP_HOST="127.0.0.1", APP_URL=f"http://localhost:{web_port}", DB_URL=database_url,
               DB_PREFIX="if_", REDIS_HOST=os.environ["OPENRUST_TEST_REDIS_HOST"],
               REDIS_PORT=os.environ["OPENRUST_TEST_REDIS_PORT"], REDIS_PREFIX=signature + ":",
               CACHE_PREFIX=signature + ":cache:", USER_EMAIL_VERIFICATION="false", MAIL_MAILER="log",
               SMS_DRIVER="log", APP_DISABLED_RUNNERS="queue,schedule,reverb,grpc", REVERB_ENABLED="false",
               BCRYPT_ROUNDS="4")
    for key in ["APP_KEY", "APP_CONFIG_CACHE", "APP_PACKAGES_CACHE", "APP_ROUTES_CACHE", "APP_EVENTS_CACHE", "DB_CONNECTION", "CACHE_STORE", "SESSION_DRIVER", "QUEUE_CONNECTION", "REDIS_URL"]:
        env.pop(key, None)

    artifacts = root.parent / ("coordination-" + signature)
    artifacts.mkdir()
    env.update(OPENRUST_INIT_TEST_ARTIFACTS=str(artifacts), REDIS_QUEUE_RETRY_AFTER="2")
    console = root / "routes/console.rs"
    original_console = console.read_text()
    register = "pub(crate) fn register() -> Result<(), ConsoleError> {\n"
    assert original_console.count(register) == 1
    console.write_text(original_console.replace(register, register + "    init_full_acceptance_commands()?;\n", 1)
                       + '\ninclude!("../tests/support/init_full_console.rs");\n')

    # 安装器及其子进程遇到任何 Docker/Compose 调用都失败，夹具自身从进程外准备服务。
    if os.name != "nt":
        traps = root.parent / "no-compose"
        traps.mkdir(exist_ok=True)
        marker = traps / "invoked"
        for name in ["docker", "docker-compose"]:
            flag = traps / name
            flag.write_text('#!/bin/sh\nprintf invoked > "$OPENRUST_INIT_COMPOSE_MARKER"\nexit 89\n')
            flag.chmod(0o755)
        env["OPENRUST_INIT_COMPOSE_MARKER"] = str(marker)
        env["PATH"] = str(traps) + os.pathsep + env["PATH"]

    def cli(*args, status=0, changes=None, source_entry=False):
        prefix = [str(root / "artisan")] if source_entry and os.name != "nt" else [str(executable), "artisan"]
        command = [*prefix, *args]
        output = subprocess.run(command, cwd=root, env=env | (changes or {}), stdin=subprocess.DEVNULL,
                                capture_output=True, text=True, timeout=900)
        combined = (output.stdout + output.stderr).replace(database_url, "<数据库凭据>")
        with event_log.open("a") as log:
            log.write(f"{args[0]} 退出码 {output.returncode}\n{combined}\n")
        assert database_url not in output.stdout + output.stderr, "输出泄露 DB_URL"
        assert (output.returncode == status if status is not None else output.returncode != 0), combined
        return combined

    manifest = root / "Cargo.toml"
    before = manifest.read_bytes()
    assert "init:full" in cli("list")
    assert "--dry-run" in cli("init:full", "--help")
    plan = cli("init:full", "--dry-run")
    assert "user-web" in plan and "webhook-server" in plan and "database=mysql" in plan
    assert "localization-database" in plan
    assert "dry run" in plan.lower() and not env_file.exists() and before == manifest.read_bytes()
    cli("init:full", status=None, changes={"DB_CONNECTION": "pgsql"})
    cli("init:full", status=None, changes={"APP_ENV": "production"})
    cli("init:full", status=None, changes={"DB_URL": "", "DB_DATABASE": "", "DB_USERNAME": ""})
    unavailable_port = str(port())
    cli("init:full", status=None, changes={"REDIS_PORT": unavailable_port})
    assert not env_file.exists() and before == manifest.read_bytes()

    assert "完整初始化通过" in cli("init:full", source_entry=True)
    completed = env_file.read_bytes()
    assert database_url.encode() not in completed
    for config in [b"DB_CONNECTION=mysql", b"CACHE_STORE=redis", b"SESSION_DRIVER=redis", b"QUEUE_CONNECTION=redis"]:
        assert config in completed, config
    assert re.search(rb"^APP_KEY=.+$", completed, re.M)
    installed = manifest.read_bytes()
    assert b'"webhook-client"' in installed and b'"localization-database"' in installed
    migration_files = sorted((root / "database/migrations").glob("*.rs"))

    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
    with ExitStack() as stack:
        first = stack.enter_context(replica(root, executable, env, web_port, "single"))
        request(opener, first, "/", 200)
        _, form = request(opener, first, "/register", 200)
        account = {"name": "Init Acceptance", "email": "init@example.test", "password": "init-password-123"}
        request(opener, first, "/register", 201, data=account | {"password_confirmation": account["password"]}, csrf=csrf(form))
        url, profile = request(opener, first, "/user/profile", 200)
        assert url.endswith("/user/profile"), url

        second = stack.enter_context(replica(root, executable, env, port(), "replica"))
        url, _ = request(opener, second, "/user/profile", 200)
        assert url == second + "/user/profile", url
        url, devices = request(opener, second, "/user/sessions", 200)
        assert url == second + "/user/sessions" and 'class="device-meta"' in devices
        request(opener, second, "/logout", 204, data={}, csrf=csrf(profile))
        url, login = request(opener, first, "/login", 200)
        assert url.endswith("/login"), url
        request(opener, first, "/login", 200, data={"email": account["email"], "password": account["password"]}, csrf=csrf(login))
        url, _ = request(opener, second, "/user/profile", 200)
        assert url == second + "/user/profile", url

        # 与应用相同配置的独立进程跨进程派发并消费指定的 Redis 队列。
        token = "01" + uuid.uuid4().hex[:24].upper()
        cli("init:verify", "enqueue", token, changes={"APP_PORT": str(web_port)})
        cli("queue:work", "redis", f"--queue=openrust:init:{token}", "--once", changes={"APP_PORT": second.rsplit(":", 1)[1]})
        cli("init:verify", "check", token)

    # 不存在的 Cargo 模拟构建工具失败。已安装的文件与用户定制仍可复用。
    extra = "\n// Preserve existing application customization.\n"
    web_routes = root / "routes/web.rs"
    customized = web_routes.read_text() + extra
    web_routes.write_text(customized)
    interrupted = cli("init:full", status=None, changes={"CARGO": str(artifacts / "missing-cargo")})
    assert "完整初始化通过" not in interrupted
    assert env_file.read_bytes() == completed and web_routes.read_text() == customized

    verify_coordination(root, executable, env, cli)
    assert "完整初始化通过" in cli("init:full")
    assert web_routes.read_text() == customized
    assert env_file.read_bytes() == completed and manifest.read_bytes() == installed
    assert sorted((root / "database/migrations").glob("*.rs")) == migration_files
    if os.name != "nt":
        assert not marker.exists()
    print("PASS: 干净应用一键安装、单机注册登录、两个副本共享 Redis 会话、Worker 强杀恢复、迁移互斥、调度去重和幂等重跑")


if __name__ == "__main__":
    main()
