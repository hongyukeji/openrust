"""在隔离网络验证 New Relic 直连和自定义 HTTPS 收集器。

将 REGIONS 的域名及测试域名解析到回环，为 TLS 提供对应证书和信任根。
OTLP 解码依赖与 new_relic_acceptance.py 相同。
"""

import argparse
from contextlib import contextmanager
from http.server import ThreadingHTTPServer
from ipaddress import ip_address
import os
from pathlib import Path
import socket
import ssl
import subprocess
import tempfile
import threading
import time

from new_relic_acceptance import CollectorHandler, PRIVATE, attrs, cli, decode, request

REGIONS = {
    "us": "otlp.nr-data.net",
    "eu": "otlp.eu01.nr-data.net",
    "jp": "otlp.jp.nr-data.net",
    "us-gov": "gov-otlp.nr-data.net",
}
COLLECTOR = "otlp-collector.fixture.test"
WRONG_NAME = "wrong-name.fixture.test"


class TlsHandler(CollectorHandler):
    def do_POST(self):
        host = self.headers.get("Host")
        keys = self.headers.get_all("api-key", [])
        expected = [] if host == COLLECTOR else ["private-license"]
        correct = host == self.connection.fixture_sni and keys == expected
        with self.server.lock:
            self.server.observed.append((host, self.path, correct))
        if self.server.redirect and self.path == "/v1/traces":
            self.send_response(302)
            self.send_header("Location", f"https://{COLLECTOR}/redirected")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        if keys:
            del self.headers["api-key"]
        super().do_POST()


@contextmanager
def tls_collector(certificate, key):
    with ThreadingHTTPServer(("127.0.0.1", 443), TlsHandler) as collector:
        collector.requests = []
        collector.observed = []
        collector.responses = {}
        collector.lock = threading.Lock()
        collector.redirect = False
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.minimum_version = ssl.TLSVersion.TLSv1_2
        context.load_cert_chain(certificate, key)

        def record_name(connection, name, _):
            connection.fixture_sni = name

        context.set_servername_callback(record_name)
        collector.socket = context.wrap_socket(collector.socket, server_side=True)
        server = threading.Thread(target=collector.serve_forever)
        server.start()
        try:
            yield collector
        finally:
            collector.shutdown()
            server.join()


def verify_http(binary, env, root):
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    env = env | {"APP_HOST": "127.0.0.1", "APP_PORT": str(port), "NEW_RELIC_TEST_HTTP": "1"}
    cli(binary, env, "config:cache")
    cli(binary, env, "route:cache")
    with (root / "http.log").open("wb") as log:
        process = subprocess.Popen([binary], env=env, stdout=log, stderr=log)
        try:
            for _ in range(100):
                assert process.poll() is None
                try:
                    assert request(port, "/api/probe/log") == 201
                    assert request(port, "/api/probe/fail") == 500
                    return
                except ConnectionRefusedError:
                    time.sleep(0.05)
            raise AssertionError("HTTP 验收应用未启动")
        finally:
            process.terminate()
            try:
                assert process.wait(timeout=15) == 0
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
                raise


def main(binary, certificate, key, ca, unrelated_ca):
    for host in (*REGIONS.values(), COLLECTOR, WRONG_NAME):
        addresses = socket.getaddrinfo(host, 443, type=socket.SOCK_STREAM)
        assert addresses and all(ip_address(item[4][0]).is_loopback for item in addresses), host

    assert "TMPDIR" in os.environ, "将 TMPDIR 指向项目内隔离验收目录"
    with tempfile.TemporaryDirectory(dir=os.environ["TMPDIR"], prefix="new-relic-tls-") as temp, \
            tls_collector(certificate, key) as collector:
        root = Path(temp)
        database = root / "app.sqlite"
        database.touch()
        env = {
            "PATH": os.environ["PATH"], "TMPDIR": temp, "SSL_CERT_FILE": str(ca),
            "APP_BASE_PATH": temp, "APP_ENV": "testing", "APP_DEBUG": "false",
            "APP_CONFIG_CACHE": str(root / "config.json"), "APP_ROUTES_CACHE": str(root / "routes.json"),
            "APP_DISABLED_RUNNERS": "queue,schedule", "CACHE_STORE": "file", "SESSION_DRIVER": "array",
            "DB_CONNECTION": "sqlite", "DB_DATABASE": str(database), "QUEUE_CONNECTION": "database",
            "NEW_RELIC_ENABLED": "true", "NEW_RELIC_LOGS_ENABLED": "true",
            "NEW_RELIC_LICENSE_KEY": "private-license",
            "OTEL_EXPORTER_OTLP_HEADERS": "api-key=private-otel-header,x-private=private-otel-header",
        }
        for region, host in REGIONS.items():
            isolated = env | {"NEW_RELIC_REGION": region}
            cli(binary, isolated, "config:cache")
            assert all(marker not in (root / "config.json").read_bytes() for marker in PRIVATE)
            start = len(collector.observed)
            cli(binary, isolated, "telemetry:probe")
            seen = collector.observed[start:]
            assert set(seen) == {
                (host, "/v1/traces", True), (host, "/v1/metrics", True),
            }, (region, seen)
            print(f"{region}: TLS、SNI、api-key、OTLP 路径与凭据缓存隔离通过", flush=True)
            cli(binary, isolated, "config:clear")

        cli(binary, env, "migrate", "--force")
        verify_http(binary, env, root)
        cli(binary, env, "telemetry:enqueue")
        cli(binary, env, "queue:work", "database", "--queue=telemetry", "--once")
        spans, metrics, logs = decode(collector.requests.copy())
        assert any(span.kind == 2 and attrs(span.attributes)["http.response.status_code"] == 500
                   for _, span in spans)
        assert any(span.name == "artisan telemetry:probe" for _, span in spans)
        assert any(span.kind == 5 and attrs(span.attributes)["openrust.queue.result"] == "success"
                   for _, span in spans)
        assert any(metric.name == "openrust.queue.job.duration" for _, metric in metrics)
        assert any(log.body.string_value == "linked fixture record" and log.trace_id for _, log in logs)
        assert all(matched for _, _, matched in collector.observed)
        cli(binary, env, "config:clear")
        print("缓存后实际 HTTP、Artisan、Worker 和三种信号解码通过", flush=True)

        custom = env | {"NEW_RELIC_COLLECTOR_ENDPOINT": f"https://{COLLECTOR}"}
        start = len(collector.observed)
        cli(binary, custom, "telemetry:probe")
        assert {(host, path, matched) for host, path, matched in collector.observed[start:]} == {
            (COLLECTOR, "/v1/traces", True), (COLLECTOR, "/v1/metrics", True),
        }
        collector.redirect = True
        start = len(collector.observed)
        cli(binary, env, "telemetry:probe")
        assert set(collector.observed[start:]) == {
            (REGIONS["us"], "/v1/traces", True), (REGIONS["us"], "/v1/metrics", True),
        }
        collector.redirect = False
        start = len(collector.observed)
        cli(binary, env | {"SSL_CERT_FILE": str(unrelated_ca)}, "telemetry:probe")
        cli(binary, custom | {"NEW_RELIC_COLLECTOR_ENDPOINT": f"https://{WRONG_NAME}"}, "telemetry:probe")
        assert len(collector.observed) == start
        print("自定义收集器无许可证，重定向未跟随，证书和域名失配均拒绝发送", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("binary", "certificate", "key", "ca", "unrelated_ca"):
        parser.add_argument(name, type=Path)
    arguments = parser.parse_args()
    main(arguments.binary.resolve(), arguments.certificate, arguments.key, arguments.ca, arguments.unrelated_ca)
