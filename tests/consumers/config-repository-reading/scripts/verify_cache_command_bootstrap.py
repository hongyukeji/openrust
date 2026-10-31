"""在独立应用副本中验证配置维护命令先引导、后执行的文件边界。"""

import argparse
import hashlib
import json
import os
import subprocess
import tempfile
from pathlib import Path


def command(binary: Path, root: Path, env: dict[str, str], name: str) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        [str(binary), name], cwd=root, env=env, capture_output=True, timeout=60
    )


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


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


def restore_copy(config: Path, original: bytes, dotenv: Path, old_dotenv: bytes | None) -> list[Exception]:
    errors = []
    try:
        if config.is_symlink():
            raise OSError("symlink")
        config.write_bytes(original)
    except OSError:
        errors.append(RuntimeError("副本配置恢复失败"))
    try:
        if dotenv.is_symlink():
            raise OSError("symlink")
        if old_dotenv is None:
            dotenv.unlink(missing_ok=True)
        else:
            dotenv.write_bytes(old_dotenv)
    except OSError:
        errors.append(RuntimeError("副本 dotenv 恢复失败"))
    try:
        if config.is_symlink() or digest(config) != hashlib.sha256(original).hexdigest():
            errors.append(RuntimeError("副本配置恢复 SHA 不符"))
    except OSError:
        errors.append(RuntimeError("副本配置恢复 SHA 无法核对"))
    try:
        if dotenv.is_symlink() or (digest(dotenv) if dotenv.exists() else None) != (
            hashlib.sha256(old_dotenv).hexdigest() if old_dotenv is not None else None
        ):
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


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path, help="自有临时应用物理副本")
    parser.add_argument("binary", type=Path)
    parser.add_argument("config", type=Path, help="相对应用根的 TOML 文件")
    parser.add_argument("--expected-binary-sha256", required=True)
    args = parser.parse_args()
    root = require_task_copy(args.root)
    binary = args.binary.resolve(strict=True)
    require(digest(binary) == args.expected_binary_sha256, "二进制 SHA-256 与本次构建不符")
    source_path = root / args.config
    if source_path.is_symlink():
        raise SystemExit("配置文件不能是符号链接")
    config = source_path.resolve(strict=True)
    if not config.is_relative_to(root):
        raise SystemExit("配置文件必须属于应用根")
    original = config.read_bytes()
    dotenv = root / ".env"
    if dotenv.is_symlink():
        raise SystemExit(".env 不能是符号链接")
    old_dotenv = dotenv.read_bytes() if dotenv.exists() else None
    original_sha = digest(config)
    dotenv_sha = digest(dotenv) if old_dotenv is not None else None
    with tempfile.TemporaryDirectory(prefix="openrust-cache-command-") as temporary:
        cache = Path(temporary) / "selected.json"
        other_cache = Path(temporary) / "other.json"
        other_cache.write_bytes(b"other-cache-preserved")
        env = os.environ.copy()
        env["APP_CONFIG_CACHE"] = str(cache)
        cases = []
        primary = None
        try:
            for source in ("dotenv", "toml"):
                config.write_bytes(original)
                if old_dotenv is None:
                    dotenv.unlink(missing_ok=True)
                else:
                    dotenv.write_bytes(old_dotenv)
                require(command(binary, root, env, "config:cache").returncode == 0, "正常缓存构建失败")
                require(cache.is_file(), "正常构建后缺少缓存")
                if source == "dotenv":
                    dotenv.write_text("broken dotenv line", encoding="utf-8")
                else:
                    config.write_text("broken toml [", encoding="utf-8")
                require(command(binary, root, env, "config:clear").returncode == 0, f"坏 {source} 时 clear 失败")
                require(not cache.exists(), f"坏 {source} 时 clear 未删除选中缓存")
                require(other_cache.read_bytes() == b"other-cache-preserved", "clear 删除了其他缓存")

                config.write_bytes(original)
                if old_dotenv is None:
                    dotenv.unlink(missing_ok=True)
                else:
                    dotenv.write_bytes(old_dotenv)
                require(command(binary, root, env, "config:cache").returncode == 0, "再次构建缓存失败")
                if source == "dotenv":
                    dotenv.write_text("broken dotenv line", encoding="utf-8")
                else:
                    config.write_text("broken toml [", encoding="utf-8")
                require(command(binary, root, env, "config:cache").returncode != 0, f"坏 {source} 时 cache 应失败")
                require(not cache.exists(), f"坏 {source} 时旧缓存应已清除")
                require(other_cache.read_bytes() == b"other-cache-preserved", "fresh 失败删除了其他缓存")
                require(
                    {path.name for path in Path(temporary).iterdir()} == {"other.json"},
                    "fresh 失败留下临时缓存或其他文件",
                )
                cases.append(source)

            config.write_bytes(original)
            if old_dotenv is None:
                dotenv.unlink(missing_ok=True)
            else:
                dotenv.write_bytes(old_dotenv)
            require(digest(config) == original_sha, "坏缓存用例的 TOML 来源未恢复")
            require(
                (digest(dotenv) if dotenv.exists() else None) == dotenv_sha,
                "坏缓存用例的 dotenv 来源未恢复",
            )
            require(command(binary, root, env, "config:cache").returncode == 0, "干净来源构建缓存失败")
            require(cache.is_file(), "干净来源未生成缓存")
            cache.write_bytes(b"secret-marker not json")
            for name in ("config:clear", "config:cache"):
                result = command(binary, root, env, name)
                require(result.returncode != 0, f"坏缓存时 {name} 应失败")
                output = result.stdout + result.stderr
                require("缓存文件格式无效".encode() in output, f"{name} 未报缓存格式错误")
                require(b"secret-marker" not in output, f"{name} 泄露坏缓存内容")
                require(cache.read_bytes() == b"secret-marker not json", f"{name} 偷删坏缓存")
                require(other_cache.read_bytes() == b"other-cache-preserved", f"{name} 改动其他缓存")
        except BaseException as error:
            primary = error
        finally:
            recovery_errors = restore_copy(config, original, dotenv, old_dotenv)
        finish(primary, recovery_errors)
        print(json.dumps({
            "bad_sources": cases,
            "corrupt_cache_preserved": True,
            "binary_sha256": digest(binary),
            "config_sha256_restored": original_sha,
            "dotenv_sha256_restored": dotenv_sha,
        }, ensure_ascii=False))


if __name__ == "__main__":
    main()
