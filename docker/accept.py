#!/usr/bin/env python3
"""Run foreground Docker acceptance with an isolated Buildx builder or database fixtures."""

import argparse
import fcntl
from functools import partial
import hashlib
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import subprocess
import sys
import time
import uuid


PREFIX = "openrust-accept-"
LABEL = "io.openrust.acceptance.run"


def docker(env, *args, lease=None):
    inherited = () if lease is None else (lease.fileno(),)
    return subprocess.check_output(
        ("docker", *args), env=env, text=True, timeout=60, pass_fds=inherited
    ).splitlines()


def containers(call, name, *, all_states):
    args = ("--all",) if all_states else ()
    ids = set()
    for ownership in (f"{LABEL}={name}", f"com.docker.compose.project={name}"):
        ids.update(call("ps", *args, "--filter", f"label={ownership}", "--quiet"))
    return sorted(ids)


def run_env(base, directory):
    name = directory.name
    env = dict(
        base,
        BUILDX_CONFIG=str(directory / "buildx"),
        BUILDX_BUILDER=name,
        COMPOSE_PROJECT_NAME=name,
        OPENRUST_ACCEPTANCE_RUN=name,
        OPENRUST_ACCEPTANCE_LABEL=f"{LABEL}={name}",
        OPENRUST_ACCEPTANCE_IMAGE=f"openrust/acceptance:{name}",
    )
    env.pop("OPENRUST_ACCEPTANCE_MYSQL_URL", None)
    env.pop("OPENRUST_ACCEPTANCE_REDIS_URL", None)
    env.pop("OPENRUST_ACCEPTANCE_REDIS_CONTAINER", None)
    return env


def fixture_volume(call, env, service):
    name = env["OPENRUST_ACCEPTANCE_RUN"]
    volume = f"{name}-{service}-data"
    if call("volume", "ls", "--filter", f"name=^{volume}$", "--quiet"):
        raise RuntimeError(f"{volume}: 测试卷名称已被占用")
    call("volume", "create", "--label", env["OPENRUST_ACCEPTANCE_LABEL"], volume)
    return volume


def fixture_address(call, container, port, cancellation):
    deadline = time.monotonic() + 120
    while not cancellation and time.monotonic() < deadline:
        status, = call("inspect", "--format", "{{.State.Status}} {{.State.Health.Status}}", container)
        if status == "running healthy":
            address, = call("port", container, f"{port}/tcp")
            return address
        if status != "running starting":
            raise RuntimeError(f"{container}: 启动状态为 {status}")
        time.sleep(0.5)
    if not cancellation:
        raise RuntimeError(f"{container}: 启动超时")


def start_mysql(env, lease, cancellation):
    call = partial(docker, env, lease=lease)
    volume = fixture_volume(call, env, "mysql")
    container = f"{env['OPENRUST_ACCEPTANCE_RUN']}-mysql"
    mysql_env = {
        "MYSQL_DATABASE": "openrust_acceptance",
        "MYSQL_USER": "acceptance",
        "MYSQL_PASSWORD": uuid.uuid4().hex,
    }
    docker(
        dict(env, **mysql_env),
        "run", "--detach", "--pull", "missing", "--name", container,
        "--label", env["OPENRUST_ACCEPTANCE_LABEL"], "--publish", "127.0.0.1::3306",
        "--mount", f"type=volume,src={volume},dst=/var/lib/mysql",
        "--env", "MYSQL_RANDOM_ROOT_PASSWORD=yes",
        "--env", "MYSQL_DATABASE", "--env", "MYSQL_USER", "--env", "MYSQL_PASSWORD",
        "--health-cmd", 'MYSQL_PWD="$MYSQL_PASSWORD" mysql --protocol=TCP -h 127.0.0.1 -u "$MYSQL_USER" "$MYSQL_DATABASE" -e "SELECT 1"',
        "--health-interval", "1s", "--health-timeout", "3s", "--health-retries", "90",
        "mysql:8.4", lease=lease,
    )
    if address := fixture_address(call, container, 3306, cancellation):
        env["OPENRUST_TEST_MYSQL_URL"] = (
            f"mysql://{mysql_env['MYSQL_USER']}:{mysql_env['MYSQL_PASSWORD']}"
            f"@{address}/{mysql_env['MYSQL_DATABASE']}"
        )
        env["OPENRUST_ACCEPTANCE_MYSQL_URL"] = env["OPENRUST_TEST_MYSQL_URL"]
        env["OPENRUST_REQUIRE_MYSQL"] = "1"


def start_redis(env, lease, cancellation):
    call = partial(docker, env, lease=lease)
    volume = fixture_volume(call, env, "redis")
    # Docker can change an automatically published port when the container restarts.
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        host_port = probe.getsockname()[1]
    container, = call(
        "run", "--detach", "--pull", "missing", "--name", f"{env['OPENRUST_ACCEPTANCE_RUN']}-redis",
        "--label", env["OPENRUST_ACCEPTANCE_LABEL"], "--publish", f"127.0.0.1:{host_port}:6379",
        "--mount", f"type=volume,src={volume},dst=/data",
        "--health-cmd", "redis-cli ping | grep -qx PONG",
        "--health-interval", "1s", "--health-timeout", "3s", "--health-retries", "90",
        "redis:8", "redis-server", "--appendonly", "yes",
    )
    if address := fixture_address(call, container, 6379, cancellation):
        env["OPENRUST_ACCEPTANCE_REDIS_URL"] = f"redis://{address}/0"
        env["OPENRUST_ACCEPTANCE_REDIS_CONTAINER"] = container


def cleanup(base, directory, lease, *, recovering):
    env = run_env(base, directory)
    call = partial(docker, env, lease=lease)
    name = directory.name
    builder = ("--builder", name)
    node = f"buildx_buildkit_{name}0"
    node_filter = ("--filter", f"name=^/{node}$", "--quiet")
    registered = name in call("buildx", "ls", "--format", "{{.Name}}")
    node_present = call("ps", "--all", *node_filter)
    cache_volume = call("volume", "ls", "--filter", f"name=^{node}_state$", "--quiet")
    if not registered and (node_present or cache_volume):
        raise RuntimeError(f"{name}: 缺少 builder 登记，保留 Docker 资源供复查")

    if recovering:
        if containers(call, name, all_states=False):
            return False
        if registered and call("ps", *node_filter):
            builds = call("buildx", "history", "ls", *builder, "--format", "{{.Status}}")
            if any(status not in {"Completed", "Error"} for status in builds):
                return False
            cache = call("buildx", "du", *builder, "--format", "{{.Reclaimable}}")
            if any(value != "true" for value in cache):
                return False

    if registered:
        call("buildx", "rm", "--force", name)
    if call("ps", "--all", *node_filter) or call(
        "volume", "ls", "--filter", f"name=^{node}_state$", "--quiet"
    ):
        raise RuntimeError(f"{name}: builder 资源仍然存在，保留登记以便复查")

    owned = containers(call, name, all_states=True)
    if owned:
        force = () if recovering else ("--force",)
        call("container", "rm", *force, *owned)
    # Only disposable volumes explicitly labeled for this run are eligible.
    # Docker refuses to remove a volume still referenced by another container.
    volumes = call("volume", "ls", "--filter",
                   f"label={env['OPENRUST_ACCEPTANCE_LABEL']}", "--quiet")
    for volume in volumes:
        call("volume", "rm", volume)
    networks = call("network", "ls", "--filter", f"label=com.docker.compose.project={name}", "--quiet")
    for network in networks:
        call("network", "rm", network)
    image = env["OPENRUST_ACCEPTANCE_IMAGE"]
    if call("image", "ls", "--filter", f"reference={image}", "--filter",
              f"label={env['OPENRUST_ACCEPTANCE_LABEL']}", "--quiet"):
        call("image", "rm", "--no-prune", image)
    shutil.rmtree(directory)
    return True


def recover_idle(base, state):
    for directory in state.iterdir():
        if not re.fullmatch(PREFIX + r"[0-9a-f]{16}", directory.name):
            continue
        try:
            with (directory / "lease").open("rb") as lease:
                fcntl.flock(lease, fcntl.LOCK_EX | fcntl.LOCK_NB)
                if (directory / "ready").is_file() and cleanup(base, directory, lease, recovering=True):
                    print(f"已回收闲置验收 {directory.name}", flush=True)
        except (FileNotFoundError, BlockingIOError):
            continue
        except (subprocess.SubprocessError, OSError, RuntimeError) as error:
            print(f"保留待复查的验收 {directory.name}: {error}", file=sys.stderr)


def run_workload(command, env, lease, cancellation):
    with subprocess.Popen(command, env=env, start_new_session=True, pass_fds=(lease.fileno(),)) as job:
        while not cancellation:
            try:
                return job.wait(timeout=0.2)
            except subprocess.TimeoutExpired:
                continue
        try:
            os.killpg(job.pid, cancellation[0])
            job.wait(timeout=10)
        except ProcessLookupError:
            job.wait()
        except subprocess.TimeoutExpired:
            os.killpg(job.pid, signal.SIGKILL)
            job.wait()
        return 128 + cancellation[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mysql", action="store_true", help="start a disposable MySQL 8.4 fixture")
    parser.add_argument("--redis", action="store_true", help="start a disposable Redis 8 fixture")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command
    if command[:1] == ["--"]:
        command = command[1:]
    if not command:
        parser.error("provide the foreground acceptance command after --")

    cancellation = []
    for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(sig, lambda received, _frame: cancellation.append(received))

    base = os.environ.copy()
    if not base.get("DOCKER_HOST") and not base.get("DOCKER_CONTEXT"):
        base["DOCKER_CONTEXT"], = docker(base, "context", "show")
    daemon, = docker(base, "info", "--format", "{{.ID}}")
    if not daemon:
        raise RuntimeError("Docker daemon did not report an ID")
    state_home = Path(base.get("XDG_STATE_HOME") or Path.home() / ".local" / "state")
    state = (state_home / "openrust" / "docker-acceptance" /
             hashlib.sha256(daemon.encode()).hexdigest())
    state = state.resolve()
    state.mkdir(mode=0o700, parents=True, exist_ok=True)
    recover_idle(base, state)
    if cancellation:
        return 128 + cancellation[0]

    directory = state / (PREFIX + uuid.uuid4().hex[:16])
    directory.mkdir(mode=0o700)
    env = run_env(base, directory)
    with (directory / "lease").open("xb") as lease:
        fcntl.flock(lease, fcntl.LOCK_EX)
        (directory / "ready").touch()
        print(f"开始 Docker 验收 {directory.name}", flush=True)
        status = 1
        try:
            if args.mysql:
                start_mysql(env, lease, cancellation)
            if args.redis and not cancellation:
                start_redis(env, lease, cancellation)
            if not (args.mysql or args.redis):
                docker(
                    env, "buildx", "create", "--driver", "docker-container",
                    "--name", directory.name, lease=lease,
                )
            if not cancellation:
                status = run_workload(command, env, lease, cancellation)
        finally:
            try:
                cleanup(base, directory, lease, recovering=False)
                print(f"已清理 Docker 验收 {directory.name}", flush=True)
            except (subprocess.SubprocessError, OSError, RuntimeError) as error:
                print(f"验收 {directory.name} 清理未完成，下次将复查: {error}", file=sys.stderr)
                status = status or 1
    if cancellation:
        return 128 + cancellation[0]
    return status if status >= 0 else 128 - status


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (subprocess.SubprocessError, OSError, RuntimeError) as error:
        sys.exit(str(error))
