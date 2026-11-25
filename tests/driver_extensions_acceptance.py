"""真实 HTTP／独立 Artisan 验证外部存储及短信 Provider，配置缓存前后共用相同实现。"""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
from queue import Queue
import socket
import subprocess
import tempfile
from threading import Thread
import time
from urllib.error import HTTPError, URLError
from urllib.request import urlopen

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument("binary", type=Path)
args = parser.parse_args()
receipts = Queue()

class Supplier(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        assert self.path == "/send"
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        assert body == {"to": "+12025550123", "text": "probe"}, body
        receipts.put(body)
        data = b'{"accepted":true,"id":"example-receipt"}'
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


def run(binary, env, *arguments):
    result = subprocess.run([str(binary), "artisan", *arguments], env=env,
                            capture_output=True, text=True, timeout=30)
    assert result.returncode == 0, result.stdout + result.stderr
    return result.stdout


def check(value, counter):
    assert value["counter"] == counter, value
    assert value["url"] == "https://cdn.example/assets/counter", value
    assert value["sms"] == {"accepted": True, "id": "example-receipt"}, value
    receipts.get(timeout=5)


def http(binary, env, log):
    port = int(env["APP_PORT"])
    process = subprocess.Popen([str(binary)], env=env | {"DRIVER_HTTP":"1", "APP_HOST":"127.0.0.1", "APP_PORT":str(port)}, stdout=log, stderr=log)
    try:
        deadline = time.monotonic() + 20
        while True:
            assert process.poll() is None, "HTTP 进程提前退出"
            try:
                with urlopen(f"http://127.0.0.1:{port}/drivers", timeout=2) as response:
                    assert response.status == 200
                    return json.load(response)
            except HTTPError:
                raise
            except URLError:
                if time.monotonic() >= deadline:
                    raise
                time.sleep(.05)
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def main():
    binary = args.binary.resolve()
    with tempfile.TemporaryDirectory(dir=root / "target/tmp", prefix="driver-consumer-") as temp:
        app = Path(temp)
        server = ThreadingHTTPServer(("127.0.0.1",0), Supplier)
        thread = Thread(target=server.serve_forever)
        thread.start()
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        env = os.environ | {"APP_HOST":"127.0.0.1", "APP_PORT":str(port)} | {"APP_BASE_PATH":str(app), "APP_ENV":"testing", "APP_DEBUG":"false",
            "CACHE_STORE":"file", "SESSION_DRIVER":"array", "APP_DISABLED_RUNNERS":"queue,schedule", "APP_SUPPORTED_LOCALES":"",
            "APP_KEY":"base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            "SMS_EXAMPLE_ENDPOINT":f"http://127.0.0.1:{server.server_port}/send"}
        try:
            with (app / "http.log").open("w+") as log:
                check(json.loads(run(binary,env,"drivers:probe")),1)
                check(http(binary,env,log),2)
                run(binary,env,"config:cache")
                # 缓存仍保存原配置；HTTP 与 Artisan 各自重新注册外部工厂。
                cached = env | {"SMS_EXAMPLE_ENDPOINT":"http://127.0.0.1:1/unreachable"}
                check(json.loads(run(binary,cached,"drivers:probe")),3)
                check(http(binary,cached,log),4)
                run(binary,cached,"config:clear")
                assert receipts.empty()
            print("真实 HTTP／Artisan：配置缓存前后四次外部存储读写与短信请求通过")
        except BaseException:
            print((app / "http.log").read_text())
            raise
        finally:
            server.shutdown()
            thread.join()
            server.server_close()

if __name__ == "__main__":
    main()
