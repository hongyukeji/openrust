"""内容审核回调的直连与隔离 Nginx HTTPS 入口。"""

from contextlib import contextmanager
import http.client
import os
from pathlib import Path
import socket
import ssl
import subprocess
import time


HOST = "callback.example.test"


@contextmanager
def callback_ingress(root, port):
    nginx = os.environ.get("OPENRUST_TEST_MODERATION_CALLBACK_NGINX")
    if not nginx:
        yield lambda: http.client.HTTPConnection("127.0.0.1", port, timeout=12)
        return

    certs = Path(os.environ["OPENRUST_TEST_MODERATION_CALLBACK_CERTS"]).resolve(strict=True)
    trusted = ssl.create_default_context(cafile=str(certs / "ca.pem"))
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        proxy_port = listener.getsockname()[1]
    prefix = root / "callback-nginx"
    prefix.mkdir()
    (prefix / "logs").mkdir()
    configuration = prefix / "nginx.conf"
    configuration.write_text(f"""
error_log stderr crit;
events {{}}
http {{
    access_log off;
    server {{
        listen 127.0.0.1:{proxy_port} ssl;
        server_name {HOST};
        ssl_protocols TLSv1.2 TLSv1.3;
        ssl_certificate "{certs / 'server.pem'}";
        ssl_certificate_key "{certs / 'server.key'}";
        location / {{
            proxy_pass http://127.0.0.1:{port};
            proxy_set_header Host $http_host;
            proxy_set_header X-Forwarded-Proto $scheme;
        }}
    }}
}}
""")

    class Connection(http.client.HTTPSConnection):
        def __init__(self, context):
            super().__init__(HOST, timeout=12, context=context)

        def connect(self):
            wire = socket.create_connection(("127.0.0.1", proxy_port), self.timeout)
            try:
                self.sock = self._context.wrap_socket(wire, server_hostname=HOST)
            except BaseException:
                wire.close()
                raise

    with (prefix / "nginx.log").open("w") as log:
        process = subprocess.Popen([str(Path(nginx).resolve(strict=True)), "-p", f"{prefix}/", "-c",
                                    str(configuration), "-g", "daemon off; master_process off;"],
                                   stdout=log, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic() + 8
            while True:
                assert process.poll() is None, f"Nginx 提前退出，请查看 {log.name}"
                trusted_connection = Connection(trusted)
                try:
                    trusted_connection.connect()
                    break
                except ConnectionRefusedError:
                    assert time.monotonic() < deadline, "等待回调 HTTPS 入口超时"
                    time.sleep(.05)
                finally:
                    trusted_connection.close()
            untrusted_connection = Connection(ssl.create_default_context())
            try:
                untrusted_connection.connect()
            except ssl.SSLCertVerificationError:
                pass
            else:
                raise AssertionError("回调客户端未配置私有 CA 也完成了握手")
            finally:
                untrusted_connection.close()
            yield lambda: Connection(trusted)
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
