"""从应用公开入口验证文本翻译包注册、配置缓存和独立 Worker。"""
import argparse
from contextlib import contextmanager, ExitStack
import json
import os
from pathlib import Path
import socket
import subprocess
import time
from urllib.error import URLError
from urllib.request import urlopen


def exercise(binary, stage):
    project = Path(__file__).resolve().parents[1]
    assert stage.is_relative_to(project / "target/tmp")
    stage.mkdir(parents=True, exist_ok=False)

    def application(name, label=None):
        directory = stage / name
        directory.mkdir()
        database = directory / "app.sqlite"
        database.touch()
        env = os.environ | {
            "APP_BASE_PATH": str(directory), "APP_ENV": "production", "APP_DEBUG": "false",
            "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            "APP_DISABLED_RUNNERS": "queue,schedule,reverb,grpc",
            "CACHE_STORE": "array", "SESSION_DRIVER": "array", "LOG_CHANNEL": "null",
            "DB_CONNECTION": "sqlite", "DB_DATABASE": str(database), "DB_PREFIX": "",
            "QUEUE_CONNECTION": "database", "APP_CONFIG_CACHE": str(directory / "config.json"),
        }
        for key in ("DB_URL", "TRANSLATE_DRIVER", "TRANSLATE_TEST_LABEL"):
            env.pop(key, None)
        if label is not None:
            env["TRANSLATE_DRIVER"] = "personal"
            env["TRANSLATE_TEST_LABEL"] = label
        return directory, env

    def artisan(app, *command, extra=None, status=0):
        directory, env = app
        result = subprocess.run(
            [str(binary), "artisan", "--no-ansi", *command], cwd=directory,
            env=env | (extra or {}), capture_output=True, text=True, timeout=45,
        )
        assert result.returncode == status, (command, result.stdout, result.stderr)
        return result.stdout, result.stderr

    @contextmanager
    def http(app):
        directory, env = app
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]
        base = f"http://127.0.0.1:{port}"
        with (directory / "http.log").open("w") as logfile:
            process = subprocess.Popen(
                [str(binary)], cwd=directory, stdout=logfile, stderr=logfile,
                env=env | {"APP_HOST": "127.0.0.1", "APP_PORT": str(port), "TRANSLATE_TEST_HTTP": "1"},
            )
            try:
                for _ in range(150):
                    try:
                        with urlopen(base + "/up", timeout=2):
                            break
                    except URLError:
                        assert process.poll() is None, "应用启动失败"
                        time.sleep(.1)
                else:
                    raise AssertionError("等待应用监听超时")
                yield base + "/api/translate/"
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

    def expected(label):
        return {"default": f"{label}:en-US: 你好 ", "named": "office:fr:你好", "source": "zh-Hans"}

    first = application("first", "first")
    second = application("second", "second")
    empty = application("empty")
    with ExitStack() as stack:
        a, b, e = [stack.enter_context(http(app)) for app in (first, second, empty)]
        for base in (a, b, e):
            assert get(base + "lang") == {"text": "translate.missing", "requests": 0}
        assert get(a + "probe") == expected("first")
        assert get(b + "probe") == expected("second")
        assert get(a + "lang")["requests"] == 2
        assert get(b + "lang")["requests"] == 2
        assert get(e + "lang")["requests"] == 0

    assert json.loads(artisan(first, "translate:test")[0]) == expected("first")
    _, error = artisan(empty, "translate:test", status=1)
    assert "未配置默认文本翻译服务" in error
    artisan(first, "vendor:publish", "--tag=translate-config")
    assert (first[0] / "config/translate.rs").is_file()

    artisan(first, "migrate", "--force")
    artisan(first, "config:cache")
    config = (first[0] / "config.json").read_text()
    assert '"translate"' in config and "你好" not in config
    changes = {"TRANSLATE_TEST_LABEL": "overridden", "TRANSLATE_DRIVER": "missing"}
    assert json.loads(artisan(first, "translate:test", extra=changes)[0]) == expected("first")
    receipt = first[0] / "worker.json"
    artisan(first, "translate:test", str(receipt), extra=changes)
    artisan(first, "queue:work", "database", "--stop-when-empty", "--sleep=0", extra=changes)
    assert json.loads(receipt.read_text()) == expected("first")
    artisan(first, "config:clear")
    assert json.loads(artisan(first, "translate:test", extra={"TRANSLATE_TEST_LABEL": "updated"})[0]) == expected("updated")
    print("外部驱动、具名解析、跨应用 HTTP、无意外调用、独立 Artisan、配置缓存及 Worker PASS")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--stage", required=True, type=Path)
    args = parser.parse_args()
    exercise(args.binary.resolve(), args.stage.resolve())
