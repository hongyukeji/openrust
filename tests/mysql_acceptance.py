"""为验收命令提供独立 MySQL 8.4 实例。"""

import argparse
import json
import os
import secrets
import signal
import subprocess
import sys
import time

LABEL = "org.openrust.acceptance.run"
DATA_DIR = "/var/lib/mysql"
DATABASE = "openrust_acceptance"


def docker(*args, timeout=20):
    # 单独的进程组让终端取消先交给本脚本，再完成资源交接。
    result = subprocess.run(["docker", *args], capture_output=True, text=True,
                            timeout=timeout, start_new_session=True)
    if result.returncode:
        raise RuntimeError(f"Docker {args[0]} 失败，退出码 {result.returncode}")
    return result.stdout.strip()


def start(run_id, password):
    image, = json.loads(docker("image", "inspect", "mysql:8.4"))
    if set(image["Config"].get("Volumes") or {}) != {DATA_DIR}:
        raise RuntimeError("MySQL 镜像声明的数据目录与验收夹具不符")
    # 绑定已核对的镜像 ID，避免标签变化后意外新增匿名卷。
    return docker("run", "--detach", "--rm", "--name", f"openrust-acceptance-{run_id}",
                  "--label", f"{LABEL}={run_id}", "--tmpfs", f"{DATA_DIR}:rw,size=1g",
                  "--publish", "127.0.0.1::3306",
                  "--env", f"MYSQL_ROOT_PASSWORD={password}", "--env", "MYSQL_ROOT_HOST=%",
                  "--env", f"MYSQL_DATABASE={DATABASE}", image["Id"], timeout=60)


def ready(container, cancelled):
    deadline = time.monotonic() + 90
    while not cancelled() and time.monotonic() < deadline:
        try:
            version = docker(
                "exec", container, "sh", "-ec",
                'MYSQL_PWD="$MYSQL_ROOT_PASSWORD" mysql --protocol=TCP -h 127.0.0.1 '
                '-uroot -Nse "SELECT VERSION()" "$MYSQL_DATABASE"', timeout=8,
            )
        except subprocess.TimeoutExpired:
            pass
        except RuntimeError:
            if docker("inspect", "--format", "{{.State.Running}}", container) != "true":
                raise RuntimeError("验收 MySQL 在就绪前退出") from None
        else:
            if not version.startswith("8.4."):
                raise RuntimeError("验收要求 MySQL 8.4")
            print(f"MySQL {version}", flush=True)
            return
        time.sleep(0.25)
    if not cancelled():
        raise RuntimeError("等待验收 MySQL 就绪超时")


def owned_containers(run_id):
    return docker("container", "ls", "--all", "--quiet", "--no-trunc",
                  "--filter", f"label={LABEL}={run_id}").splitlines()


def cleanup(run_id):
    for container in owned_containers(run_id):
        try:
            docker("container", "rm", "-f", container)
        except RuntimeError:
            if container in owned_containers(run_id):
                raise
    if owned_containers(run_id):
        raise RuntimeError("验收容器仍存在")


def signal_group(child, signum):
    try:
        os.killpg(child.pid, signum)
    except ProcessLookupError:
        pass


def run_workload(command, env, cancelled):
    with subprocess.Popen(command, env=env, start_new_session=True) as child:
        while not cancelled():
            try:
                code = child.wait(timeout=0.2)
            except subprocess.TimeoutExpired:
                continue
            if not cancelled():
                return code if code >= 0 else 128 - code
        signal_group(child, cancelled())
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            pass
        # 即使主进程先退出，仍要收尾它的同组子进程。
        signal_group(child, signal.SIGKILL)
        child.wait()
        return 128 + cancelled()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    command = parser.parse_args().command
    if command[:1] == ["--"]:
        command = command[1:]
    if not command:
        parser.error("请在 -- 后提供验收命令")

    received = None

    def cancel(signum, _frame):
        nonlocal received
        received = received or signum

    for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(signum, cancel)

    run_id = secrets.token_hex(16)
    password = secrets.token_hex(24)
    print(f"MySQL 验收批次 {LABEL}={run_id}", flush=True)
    result = 1
    try:
        container = start(run_id, password)
        ready(container, lambda: received)
        if not received:
            address = docker("port", container, "3306/tcp")
            host, separator, port = address.partition(":")
            if host != "127.0.0.1" or not separator or not port.isascii() or not port.isdigit():
                raise RuntimeError("验收 MySQL 未映射到独立的回环端口")
            url = f"mysql://root:{password}@{address}/{DATABASE}"
            env = dict(os.environ, OPENRUST_ACCEPTANCE_MYSQL_CONTAINER=container,
                       OPENRUST_TEST_MYSQL_URL=url, OPENRUST_REQUIRE_MYSQL="1")
            result = run_workload(command, env, lambda: received)
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
        print(f"MySQL 验收失败（{type(error).__name__}，批次 {run_id}）", file=sys.stderr)
        if isinstance(error, RuntimeError):
            print(str(error), file=sys.stderr)
    finally:
        try:
            cleanup(run_id)
        except (OSError, RuntimeError, subprocess.TimeoutExpired):
            print(f"无法确认验收容器已清理，请核对 Docker 标签 {LABEL}={run_id}", file=sys.stderr)
            result = result or 1
    return 128 + received if received else result


if __name__ == "__main__":
    sys.exit(main())
