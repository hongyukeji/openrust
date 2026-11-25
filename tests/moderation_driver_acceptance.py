"""两种外部审核驱动通过真实 HTTP、Artisan 和独立队列 Worker 装配。"""

import argparse
import http.client
import json
import os
from pathlib import Path
import runpy
import socket
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
DATABASE = runpy.run_path(str(ROOT / "tests/sms_worker.py"))
HTTP = runpy.run_path(str(ROOT / "tests/webhooks_acceptance.py"))
EXPECTED = {"text_review": True, "image_allow": True, "ocr_missing": True,
            "no_video_on_text": True}


def exercise(binary, driver, database_url, root):
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    sqlite = root / "fixture.sqlite"
    if driver == "sqlite":
        sqlite.touch()
        os.environ["OPENRUST_SMS_SQLITE_PATH"] = str(sqlite)
    env = os.environ | {
        "APP_BASE_PATH": str(root), "APP_ENV": "testing", "APP_DEBUG": "false",
        "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "APP_SUPPORTED_LOCALES": "",
        "APP_KEY": "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
        "CACHE_STORE": "file", "SESSION_DRIVER": "array", "APP_DISABLED_RUNNERS": "queue,schedule",
        "DRIVER_HTTP": "1", "DB_CONNECTION": driver, "QUEUE_CONNECTION": "database",
        "DB_DATABASE": str(sqlite) if driver == "sqlite" else "openrust_acceptance",
        "MODERATION_FIXTURE_REGION": "original",
    }
    if database_url:
        env["DB_URL"] = database_url
    else:
        env.pop("DB_URL", None)

    def command(current, *tokens):
        completed = subprocess.run([str(binary), "artisan", *tokens], env=current,
                                   capture_output=True, text=True, timeout=45)
        assert completed.returncode == 0, completed.stdout + completed.stderr
        return completed.stdout

    command(env, "migrate", "--force")
    command(env, "vendor:publish", "--tag=moderation-config")
    template = root / "config/moderation.rs"
    assert "MODERATION_DRIVER" in template.read_text()
    with template.open("a") as stream:
        stream.write("\n// 本机定制\n")
    expected_template = template.read_bytes()
    command(env, "vendor:publish", "--tag=moderation-config")
    assert template.read_bytes() == expected_template

    def rows(query):
        return DATABASE["database_rows"](DATABASE["database_container"](), query).strip()

    for cached in (False, True):
        current = env
        if cached:
            command(env, "config:cache")
            command(env, "route:cache")
            current = env | {"MODERATION_FIXTURE_REGION": "unavailable-from-cache"}
        assert json.loads(command(current, "drivers:moderation")) == EXPECTED
        with (root / "http.log").open("a+") as log:
            process = HTTP["serve"](binary, current, log)
            try:
                connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
                try:
                    connection.request("GET", "/api/drivers/moderation", headers={"Accept": "application/json"})
                    response = connection.getresponse()
                    body = response.read()
                    assert response.status == 200, (response.status, body[:200])
                    assert json.loads(body) == EXPECTED
                finally:
                    connection.close()
            finally:
                HTTP["stop"](process)
        task = json.loads(command(current, "drivers:moderation", "--queue"))
        assert task == "original:revision-2"
        payload = rows("SELECT payload FROM jobs")
        assert payload and "original:revision-2" in payload
        assert "videos/post.mp4" not in payload and "unavailable-from-cache" not in payload
        output = root / f"worker-{cached}.json"
        command(current | {"MODERATION_FIXTURE_RESULT": str(output)}, "queue:work", "database", "--once")
        assert json.loads(output.read_text()) == {"id": task, "speech_partial": True, "allow": False}
        assert rows("SELECT COUNT(*) FROM jobs") == rows("SELECT COUNT(*) FROM failed_jobs") == "0"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    driver, database_url = DATABASE["database_settings"]()
    with tempfile.TemporaryDirectory(dir=ROOT / "target/tmp", prefix="moderation-driver-") as directory:
        exercise(args.binary.resolve(), driver, database_url, Path(directory))
    print(f"{driver}: 外部文本和媒体驱动通过 HTTP、Artisan、独立 Worker、缓存及重复发布")
