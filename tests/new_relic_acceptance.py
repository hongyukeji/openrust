"""验证真实 HTTP、独立 Artisan 和 Worker 输出的 OTLP protobuf。

运行时通过 PYTHONPATH 提供 opentelemetry-proto==1.43.0。
"""
import argparse
from contextlib import contextmanager
from http.client import HTTPConnection
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time

from opentelemetry.proto.collector.metrics.v1.metrics_service_pb2 import ExportMetricsServiceRequest
from opentelemetry.proto.collector.trace.v1.trace_service_pb2 import ExportTraceServiceRequest


ROOT = Path(__file__).resolve().parents[1]
PRIVATE = (b"private-command-input", b"private-job-input", b"private-url-id",
           b"private-job-error", b"private-sql-literal", b"private_missing_table", b"private-cookie", b"private-query", b"private-license", b"private-otel-header")


class Collector(ThreadingHTTPServer):
    def __init__(self):
        super().__init__(("127.0.0.1", 0), CollectorHandler)
        self.requests = []
        self.responses = {}
        self.received_traceparents = []
        self.lock = threading.Lock()


class CollectorHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        with self.server.lock:
            self.server.received_traceparents.append(self.headers.get("traceparent"))
        self.send_response(204)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_POST(self):
        body = self.rfile.read(int(self.headers["Content-Length"]))
        with self.server.lock:
            pending = self.server.responses.get(self.path, [])
            status = pending.pop(0) if pending else 200
            self.server.requests.append((self.path, dict(self.headers), body, status))
        if status == 0:
            time.sleep(4)
            return
        self.send_response(status)
        self.send_header("Content-Type", "application/x-protobuf")
        self.send_header("Content-Length", "0")
        if status == 503:
            self.send_header("Retry-After", "1")
        self.end_headers()

    def log_message(self, *_args):
        pass


@contextmanager
def collector_server():
    with Collector() as collector:
        thread = threading.Thread(target=collector.serve_forever, daemon=True)
        thread.start()
        try:
            yield collector
        finally:
            collector.shutdown()
            thread.join()


def attrs(attributes):
    return {entry.key: getattr(entry.value, entry.value.WhichOneof("value")) for entry in attributes}


def decode(requests):
    spans, metrics = [], []
    for path, headers, body, status in requests:
        assert headers.get("content-type", headers.get("Content-Type")) == "application/x-protobuf", headers
        assert not {"api-key", "x-private"} & {header.lower() for header in headers}
        assert all(value not in body for value in PRIVATE)
        if status != 200:
            continue
        if path == "/v1/traces":
            protobuf = ExportTraceServiceRequest.FromString(body)
            for resource in protobuf.resource_spans:
                service = attrs(resource.resource.attributes)["service.name"]
                spans.extend((service, span) for scope in resource.scope_spans for span in scope.spans)
        elif path == "/v1/metrics":
            protobuf = ExportMetricsServiceRequest.FromString(body)
            for resource in protobuf.resource_metrics:
                service = attrs(resource.resource.attributes)["service.name"]
                metrics.extend((service, metric) for scope in resource.scope_metrics for metric in scope.metrics)
        else:
            raise AssertionError(path)
    return spans, metrics


def cli(binary, env, *args, exit_code=0):
    outcome = subprocess.run([binary, "artisan", *args], env=env, capture_output=True, timeout=25)
    assert outcome.returncode == exit_code, (args, outcome.returncode, outcome.stdout, outcome.stderr)
    assert b"private-license" not in outcome.stdout + outcome.stderr
    return outcome


def request(port, path):
    connection = HTTPConnection("127.0.0.1", port, timeout=5)
    try:
        connection.request("GET", path, headers={
            "traceparent": "00-01234567890123456789012345678901-0123456789012345-01",
            "Cookie": "private-cookie",
        })
        response = connection.getresponse()
        response.read()
        return response.status
    finally:
        connection.close()


def web(binary, env, root):
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    with (root / "http.log").open("wb") as log:
        process = subprocess.Popen([binary], env=env | {
            "APP_HOST": "127.0.0.1", "APP_PORT": str(port), "NEW_RELIC_TEST_HTTP": "1",
        }, stdout=log, stderr=log)
        try:
            for _ in range(100):
                assert process.poll() is None, (root / "http.log").read_text()
                try:
                    assert request(port, "/api/probe/private-url-id?value=private-query") == 204
                    break
                except ConnectionRefusedError:
                    time.sleep(0.05)
            else:
                raise AssertionError("HTTP 未监听")
            assert request(port, "/api/probe/dependency") == 202
            assert request(port, "/api/probe/fail") == 500
            assert request(port, "/private-url-id") == 404
        finally:
            process.terminate()
            try:
                assert process.wait(timeout=15) == 0, (root / "http.log").read_text()
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
                raise


def main(binary, driver):
    (ROOT / "target/tmp").mkdir(parents=True, exist_ok=True)
    with collector_server() as collector, collector_server() as other, tempfile.TemporaryDirectory(
        dir=ROOT / "target/tmp", prefix="new-relic-runtime-"
    ) as temp:
        root = Path(temp)
        env = {key: value for key, value in os.environ.items()
               if not key.startswith(("NEW_RELIC_", "OTEL_", "DB_", "APP_", "QUEUE_"))}
        env |= {
            "TMPDIR": temp, "APP_BASE_PATH": temp, "APP_ENV": "testing", "APP_DEBUG": "false",
            "APP_DISABLED_RUNNERS": "queue,schedule", "APP_CONFIG_CACHE": str(root / "configuration.json"),
            "DB_CONNECTION": driver, "DB_DATABASE": str(root / "db.sqlite"),
            "NEW_RELIC_TEST_ALLOWED_UPSTREAM": f"http://127.0.0.1:{collector.server_port}/private-url-id?token=private-query",
            "NEW_RELIC_TEST_OTHER_UPSTREAM": f"http://127.0.0.1:{other.server_port}/private-url-id",
            "NEW_RELIC_PROPAGATE_ORIGINS": f"http://127.0.0.1:{collector.server_port}",
            "CACHE_STORE": "file", "SESSION_DRIVER": "array", "QUEUE_CONNECTION": "database",
            "APP_NAME": "acceptance-app",
            "NEW_RELIC_COLLECTOR_ENDPOINT": f"http://127.0.0.1:{collector.server_port}",
            "NEW_RELIC_LICENSE_KEY": "private-license",
            "OTEL_EXPORTER_OTLP_HEADERS": "x-private=private-otel-header,api-key=private-otel-header",
        }
        if driver == "sqlite":
            (root / "db.sqlite").touch()
        else:
            env["DB_URL"] = os.environ[("OPENRUST_TEST_POSTGRES_URL" if driver == "pgsql" else "OPENRUST_TEST_MYSQL_URL")]
            env["DB_DATABASE"] = "openrust_acceptance"
        cli(binary, env, "telemetry:probe")
        assert collector.requests == [], "默认未启用的扩展发送了遥测"
        cli(binary, env, "migrate", "--force")
        cli(binary, env, "telemetry:enqueue")
        cli(binary, env, "telemetry:enqueue", "--fail")
        enabled = env | {"NEW_RELIC_ENABLED": "true"}

        collector.responses["/v1/traces"] = [503]
        web(binary, enabled, root)
        requests = collector.requests.copy()
        attempts = [item for item in requests if item[0] == "/v1/traces"]
        assert any(item[3] == 503 for item in attempts) and any(item[3] == 200 for item in attempts)
        spans, metrics = decode(requests)
        http_spans = {attrs(span.attributes)["http.response.status_code"]: span for name, span in spans
                      if name == "acceptance-app" and span.kind == 2}
        assert {204, 404, 500} <= http_spans.keys(), http_spans
        assert http_spans[204].name == "GET /api/probe/{id}", http_spans[204]
        assert http_spans[204].trace_id.hex() == "01234567890123456789012345678901"
        assert http_spans[204].parent_span_id.hex() == "0123456789012345"
        assert http_spans[404].status.code == 0 and http_spans[404].name == "GET"
        assert http_spans[500].status.code == 2
        parent = http_spans[202]
        children = [span for name, span in spans if name == "acceptance-app"
                    and span.parent_span_id == parent.span_id and span.trace_id == parent.trace_id]
        http_children = [span for span in children if attrs(span.attributes).get("http.request.method") == "GET"]
        db_children = [span for span in children if attrs(span.attributes).get("db.system.name")]
        assert len(http_children) == 2 and len(db_children) >= 4
        assert any(span.status.code == 2 for span in db_children), db_children
        assert collector.received_traceparents and collector.received_traceparents[0].split("-")[1] == parent.trace_id.hex()
        assert other.received_traceparents == [None]
        assert {"http.client.request.duration", "db.client.operation.duration"} <= {metric.name for _, metric in metrics}
        print(f"{driver} 出站 HTTP 与 SQL 子链路、失败及传播白名单通过", flush=True)
        http_metric = next(metric for _, metric in metrics if metric.name == "http.server.request.duration")
        assert http_metric.unit == "s" and http_metric.histogram.aggregation_temporality == 1
        points = [attrs(point.attributes) for point in http_metric.histogram.data_points]
        assert any(point.get("http.route") == "/api/probe/{id}" and point.get("error.type") == "500"
                   for point in points), points
        print("HTTP 路由模板、W3C 父链路、5xx、Delta 指标与 503 重试通过", flush=True)

        start = len(collector.requests)
        collector.responses["/v1/traces"] = [400]
        artisan = enabled | {"NEW_RELIC_APP_NAME": "acceptance-artisan"}
        cli(binary, artisan, "telemetry:probe")
        rejected = [item for item in collector.requests[start:] if item[0] == "/v1/traces"]
        assert len(rejected) == 1 and rejected[0][3] == 400
        cli(binary, artisan, "telemetry:probe", "private-command-input", "--fail", exit_code=1)
        cli(binary, enabled | {"NEW_RELIC_APP_NAME": "acceptance-worker"},
            "queue:work", "database", "--queue=telemetry", "--stop-when-empty", "--tries=1")
        spans, metrics = decode(collector.requests.copy())
        assert any(name == "acceptance-artisan" and span.name == "artisan telemetry:probe"
                   and span.status.code == 2 for name, span in spans)
        assert any(name == "acceptance-worker" and span.name.startswith("queue ")
                   and attrs(span.attributes)["openrust.queue.result"] == "success" for name, span in spans)
        assert any(name == "acceptance-worker" and span.name.startswith("queue ")
                   and span.status.code == 2
                   and attrs(span.attributes)["openrust.queue.result"] == "error" for name, span in spans)
        assert any(name == "acceptance-worker" and metric.name == "openrust.queue.job.duration"
                   for name, metric in metrics)
        worker_spans = [span for name, span in spans if name == "acceptance-worker"]
        job_ids = {span.span_id for span in worker_spans if span.name.startswith("queue ")}
        assert any(span.name == "artisan telemetry:probe" and span.parent_span_id in job_ids
                   for span in worker_spans), worker_spans
        print(f"独立 Artisan、{driver} Worker 的嵌套链路、拒收和退出刷新通过", flush=True)

        invalid_env = enabled.copy()
        invalid_env.pop("NEW_RELIC_COLLECTOR_ENDPOINT")
        invalid_env.pop("NEW_RELIC_LICENSE_KEY")
        missing = cli(binary, invalid_env, "telemetry:probe", exit_code=1)
        assert b"NEW_RELIC_LICENSE_KEY" in (missing.stdout + missing.stderr), (missing.stdout, missing.stderr)
        invalid_env["NEW_RELIC_COLLECTOR_ENDPOINT"] = "http://example.test"
        invalid = cli(binary, invalid_env, "telemetry:probe", exit_code=1)
        assert b"new_relic.collector_endpoint" in (invalid.stdout + invalid.stderr), (invalid.stdout, invalid.stderr)
        cli(binary, enabled, "config:cache")
        assert b"private-license" not in Path(enabled["APP_CONFIG_CACHE"]).read_bytes()
        cli(binary, env, "telemetry:probe")
        print("配置校验、凭据隔离以及配置缓存通过", flush=True)

        collector.responses["/v1/traces"] = [0]
        started = time.monotonic()
        cli(binary, enabled, "telemetry:probe")
        assert time.monotonic() - started < 12
        assert any(item[3] == 0 for item in collector.requests)
        print("导出超时期间命令保留正常退出码", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--driver", choices=("sqlite", "mysql", "pgsql"), default="sqlite")
    args = parser.parse_args()
    main(str(args.binary.resolve()), args.driver)
