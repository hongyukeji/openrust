"""真实进程验证配置缓存、文件变更、重启读取与清除后的读取。"""

import argparse
import errno
import hashlib
import json
import os
import re
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
    marker = root / ".config-cache-task-root"
    if (
        root.parent.parent not in temporary_roots
        or not root.parent.name.startswith("config-cache-")
        or marker.is_symlink()
        or not marker.is_file()
        or marker.read_text(encoding="utf-8").strip() != str(root)
    ):
        raise SystemExit("仅允许带有匹配路径标记的任务临时应用副本")
    return root


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def restore_copy(dotenv: Path, original: bytes | None) -> list[Exception]:
    errors = []
    try:
        if dotenv.is_symlink():
            raise OSError("symlink")
        if original is None:
            dotenv.unlink(missing_ok=True)
        else:
            dotenv.write_bytes(original)
    except OSError:
        errors.append(RuntimeError("副本 dotenv 恢复失败"))
    try:
        restored = digest(dotenv) if dotenv.exists() else None
        expected = hashlib.sha256(original).hexdigest() if original is not None else None
        if dotenv.is_symlink() or restored != expected:
            errors.append(RuntimeError("副本 dotenv 恢复 SHA 不符"))
    except OSError:
        errors.append(RuntimeError("副本 dotenv 恢复 SHA 无法核对"))
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
    parser.add_argument("route", help="GET 路径")
    parser.add_argument("field", help="响应 JSON 点路径")
    parser.add_argument("before", help="缓存时预期值")
    parser.add_argument("after", help="清除后预期值")
    parser.add_argument("--expected-binary-sha256", required=True)
    parser.add_argument(
        "--bad-content",
        help="可选：先把 .env 改为无效 dotenv，验证缓存命中跳过解析并仍能重启 GET",
    )
    args = parser.parse_args()
    root = require_task_copy(args.root)
    binary = args.binary.resolve(strict=True)
    if digest(binary) != args.expected_binary_sha256:
        raise SystemExit("二进制 SHA-256 与本次构建不符")
    if not all(re.fullmatch(r"[A-Za-z0-9_-]+", value) for value in (args.before, args.after)):
        raise SystemExit("APP_NAME 验收值只允许 ASCII 字母、数字、下划线和连字符")
    dotenv = root / ".env"
    if dotenv.is_symlink():
        raise SystemExit(".env 不能是符号链接")
    original = dotenv.read_bytes() if dotenv.exists() else None
    original_sha = hashlib.sha256(original).hexdigest() if original is not None else None
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    with tempfile.TemporaryDirectory(prefix="config-cache-") as temporary:
        env = os.environ.copy()
        env.pop("APP_NAME", None)
        env.pop("APP_ENV", None)
        env["APP_CONFIG_CACHE"] = str(Path(temporary) / "config.json")
        env["SERVER_PORT"] = str(port)
        url = f"http://127.0.0.1:{port}{args.route}"
        primary = None
        try:
            dotenv.write_text(f"APP_ENV=local\nAPP_NAME={args.before}\n", encoding="utf-8")
            run(binary, root, env, "config:cache")
            if not Path(env["APP_CONFIG_CACHE"]).is_file():
                raise AssertionError("构建后缺少配置缓存")
            if args.bad_content is not None:
                dotenv.write_text(args.bad_content, encoding="utf-8")
                bad_cached = read_once(binary, root, env, url, args.field)
                if bad_cached != args.before:
                    raise AssertionError("坏 dotenv 缓存启动值不符")
            dotenv.write_text(f"APP_ENV=local\nAPP_NAME={args.after}\n", encoding="utf-8")
            cached = read_once(binary, root, env, url, args.field)
            if cached != args.before:
                raise AssertionError("缓存启动值不符")
            run(binary, root, env, "config:clear")
            if Path(env["APP_CONFIG_CACHE"]).exists():
                raise AssertionError("清除后配置缓存仍存在")
            fresh = read_once(binary, root, env, url, args.field)
            if fresh != args.after:
                raise AssertionError("清除后启动值不符")
            result = {"cached": cached, "cleared": fresh}
            if args.bad_content is not None:
                result["bad_dotenv_cached"] = bad_cached
        except BaseException as error:
            primary = error
        finally:
            recovery_errors = restore_copy(dotenv, original)
        finish(primary, recovery_errors)
        result["binary_sha256"] = digest(binary)
        result["dotenv_sha256_restored"] = original_sha
        print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    main()
