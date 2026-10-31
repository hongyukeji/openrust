"""真实进程验证配置缓存、文件变更、重启读取与清除后的读取。"""

import argparse
import errno
import hashlib
import json
import os
import signal
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path


def field(value: object, dotted: str) -> object:
    for segment in dotted.split("."):
        value = value[int(segment)] if isinstance(value, list) else value[segment]
    return value


def require_task_copy(root_arg: Path) -> Path:
    if root_arg.is_symlink():
        raise SystemExit("应用副本不能是符号链接")
    root = root_arg.resolve(strict=True)
    temporary_roots = {
        Path(tempfile.gettempdir()).resolve(strict=True),
        Path("/tmp").resolve(strict=True),
    }
    marker = root / ".openrust-config-cache-task-root"
    if (
        root.parent.parent not in temporary_roots
        or not root.parent.name.startswith("openrust-config-cache-")
        or marker.is_symlink()
        or not marker.is_file()
        or marker.read_text(encoding="utf-8").strip() != str(root)
    ):
        raise SystemExit("仅允许带有匹配路径标记的任务临时应用副本")
    return root


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def restore_copy(config: Path, original: bytes) -> list[Exception]:
    errors = []
    try:
        if config.is_symlink():
            raise OSError("symlink")
        config.write_bytes(original)
    except OSError:
        errors.append(RuntimeError("副本配置恢复失败"))
    try:
        if config.is_symlink() or digest(config) != hashlib.sha256(original).hexdigest():
            errors.append(RuntimeError("副本配置恢复 SHA 不符"))
    except OSError:
        errors.append(RuntimeError("副本配置恢复 SHA 无法核对"))
    return errors


def finish(primary: BaseException | None, recovery_errors: list[Exception]) -> None:
    if recovery_errors:
        if primary is not None:
            raise BaseExceptionGroup("验收与副本恢复均失败", [primary, *recovery_errors]) from None
        raise ExceptionGroup("副本恢复失败", recovery_errors) from None
    if primary is not None:
        raise primary.with_traceback(primary.__traceback__)


def run(binary: Path, root: Path, env: dict[str, str], *args: str) -> None:
    subprocess.run([str(binary), *args], cwd=root, env=env, check=True, timeout=60)


def read_once(binary: Path, root: Path, env: dict[str, str], url: str, key: str) -> object:
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with tempfile.TemporaryFile() as errors:
        process = subprocess.Popen(
            [str(binary), "serve"], cwd=root, env=env,
            stdout=subprocess.DEVNULL, stderr=errors,
        )
        value = None
        request_error = None
        try:
            deadline = time.monotonic() + 20
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    raise RuntimeError("应用在响应前退出")
                try:
                    with opener.open(url, timeout=1) as response:
                        value = field(json.load(response), key)
                        break
                except urllib.error.HTTPError:
                    raise
                except (urllib.error.URLError, TimeoutError, ConnectionError):
                    time.sleep(0.2)
            else:
                raise RuntimeError("应用在期限内未响应")
        except Exception as error:
            request_error = error
        cleanup_errors = []
        if process.poll() is None:
            try:
                process.send_signal(signal.SIGINT)
            except ProcessLookupError:
                pass
        try:
            process.communicate(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            cleanup_errors.append("应用未在 SIGINT 后十秒内关闭，已强制终止")
            try:
                process.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                cleanup_errors.append("应用在强制终止后仍未在五秒内退出")
        if process.returncode != 0:
            cleanup_errors.append(f"应用退出码为 {process.returncode}，预期为 0")
        with socket.socket() as sock:
            sock.settimeout(1)
            try:
                port_result = sock.connect_ex(("127.0.0.1", int(env["SERVER_PORT"])))
            except OSError as error:
                cleanup_errors.append(f"无法核对应用端口：{error}")
            else:
                if port_result != errno.ECONNREFUSED:
                    cleanup_errors.append(f"应用端口仍可连接或状态异常：{port_result}")
        if cleanup_errors:
            cleanup_error = RuntimeError("；".join(cleanup_errors))
            if request_error is not None:
                raise ExceptionGroup("请求与应用关闭均失败", [request_error, cleanup_error])
            raise cleanup_error
        if request_error is not None:
            raise request_error
        return value


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("binary", type=Path)
    parser.add_argument("config", type=Path, help="相对应用根的配置文件")
    parser.add_argument("find", help="原文件中唯一待替换的文本")
    parser.add_argument("replace", help="替换后的文本")
    parser.add_argument("route", help="GET 路径")
    parser.add_argument("field", help="响应 JSON 点路径")
    parser.add_argument("before", help="缓存时预期值")
    parser.add_argument("after", help="清除后预期值")
    parser.add_argument("--expected-binary-sha256", required=True)
    parser.add_argument(
        "--bad-content",
        help="可选：先把配置文件改为无效 TOML，验证缓存命中仍能重启 GET",
    )
    args = parser.parse_args()
    root = require_task_copy(args.root)
    binary = args.binary.resolve(strict=True)
    if digest(binary) != args.expected_binary_sha256:
        raise SystemExit("二进制 SHA-256 与本次构建不符")
    config_path = root / args.config
    if config_path.is_symlink():
        raise SystemExit("配置文件必须属于应用根且不是符号链接")
    config = config_path.resolve(strict=True)
    if not config.is_relative_to(root):
        raise SystemExit("配置文件必须属于应用根且不是符号链接")
    original = config.read_bytes()
    original_sha = digest(config)
    text = original.decode("utf-8")
    if text.count(args.find) != 1:
        raise SystemExit("待替换文本必须恰好出现一次")
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    with tempfile.TemporaryDirectory(prefix="openrust-config-cache-") as temporary:
        env = os.environ.copy()
        env["APP_CONFIG_CACHE"] = str(Path(temporary) / "config.json")
        env["SERVER_PORT"] = str(port)
        url = f"http://127.0.0.1:{port}{args.route}"
        primary = None
        try:
            run(binary, root, env, "config:cache")
            if args.bad_content is not None:
                config.write_text(args.bad_content, encoding="utf-8")
                bad_cached = read_once(binary, root, env, url, args.field)
                if bad_cached != args.before:
                    raise AssertionError("坏 TOML 缓存启动值不符")
            config.write_text(text.replace(args.find, args.replace), encoding="utf-8")
            cached = read_once(binary, root, env, url, args.field)
            if cached != args.before:
                raise AssertionError("缓存启动值不符")
            run(binary, root, env, "config:clear")
            fresh = read_once(binary, root, env, url, args.field)
            if fresh != args.after:
                raise AssertionError("清除后启动值不符")
            result = {"cached": cached, "cleared": fresh}
            if args.bad_content is not None:
                result["bad_toml_cached"] = bad_cached
        except BaseException as error:
            primary = error
        finally:
            recovery_errors = restore_copy(config, original)
        finish(primary, recovery_errors)
        result["binary_sha256"] = digest(binary)
        result["config_sha256_restored"] = original_sha
        print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    main()
