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

from opentelemetry.proto.collector.logs.v1.logs_service_pb2 import (
    ExportLogsPartialSuccess, ExportLogsServiceRequest, ExportLogsServiceResponse,
)
from opentelemetry.proto.collector.metrics.v1.metrics_service_pb2 import (
    ExportMetricsPartialSuccess, ExportMetricsServiceRequest, ExportMetricsServiceResponse,
)
from opentelemetry.proto.collector.trace.v1.trace_service_pb2 import (
    ExportTracePartialSuccess, ExportTraceServiceRequest, ExportTraceServiceResponse,
)


ROOT = Path(__file__).resolve().parents[1]
PRIVATE = (b"private-command-input", b"private-job-input", b"private-url-id",
           b"private-job-error", b"private-exception-message", b"private-explicit-report", b"private-explicit-source",
           b"private-log-context", b"private-sql-literal", b"private_missing_table", b"private-cookie", b"private-query",
           b"private-license", b"private-otel-header")


class Collector(ThreadingHTTPServer):
    def __init__(self):
        super().__init__(("127.0.0.1", 0), CollectorHandler)
        self.requests = []
        self.responses = {}
        self.received_traceparents = []
        self.redirect_to = None
        self.retry_received = False
        self.lock = threading.Lock()


class CollectorHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        with self.server.lock:
            self.server.received_traceparents.append(self.headers.get("traceparent"))
            retry_failure = self.path == "/retry" and not self.server.retry_received
            self.server.retry_received |= self.path == "/retry"
        if self.path in ("/slow-body", "/interrupted-body"):
            self.send_response(200)
            self.send_header("Content-Length", "7")
            self.end_headers()
            self.wfile.flush()
            if self.path == "/slow-body":
                time.sleep(0.25)
                try:
                    self.wfile.write(b"fixture")
                except (BrokenPipeError, ConnectionResetError):
                    pass
            return
        location = self.server.redirect_to if self.path == "/redirect" else None
        self.send_response(503 if retry_failure else 302 if location else 204)
        if location:
            self.send_header("Location", location)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_POST(self):
        body = self.rfile.read(int(self.headers["Content-Length"]))
        with self.server.lock:
            pending = self.server.responses.get(self.path, [])
            status = pending.pop(0) if pending else 200
            partial = (status == 299)
            delayed = (status == 298)
            if partial or delayed:
                status = 200
            self.server.requests.append((self.path, dict(self.headers), body, status))
        if status == 0:
            time.sleep(4)
            return
        if delayed:
            time.sleep(0.5)
        reply = b""
        if partial:
            partial_responses = {
                "/v1/logs": ExportLogsServiceResponse(partial_success=ExportLogsPartialSuccess(
                    rejected_log_records=1, error_message="fixture partial success",
                )),
                "/v1/metrics": ExportMetricsServiceResponse(partial_success=ExportMetricsPartialSuccess(
                    rejected_data_points=2, error_message="fixture partial success",
                )),
                "/v1/traces": ExportTraceServiceResponse(partial_success=ExportTracePartialSuccess(
                    rejected_spans=3, error_message="fixture partial success",
                )),
            }
            reply = partial_responses[self.path].SerializeToString()
        self.send_response(status)
        self.send_header("Content-Type", "application/x-protobuf")
        self.send_header("Content-Length", str(len(reply)))
        if status in (429, 503):
            self.send_header("Retry-After", "1")
        self.end_headers()
        if reply:
            self.wfile.write(reply)

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
    spans, metrics, logs = [], [], []
    for path, headers, body, status in requests:
        assert headers.get("content-type", headers.get("Content-Type")) == "application/x-protobuf", headers
        assert not {"api-key", "x-private"} & {header.lower() for header in headers}
        assert len(body) <= 999_999, (path, len(body))
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
        elif path == "/v1/logs":
            protobuf = ExportLogsServiceRequest.FromString(body)
            for resource in protobuf.resource_logs:
                service = attrs(resource.resource.attributes)["service.name"]
                logs.extend((service, log) for scope in resource.scope_logs for log in scope.log_records)
        else:
            raise AssertionError(path)
    return spans, metrics, logs


def cli(binary, env, *args, exit_code=0):
    outcome = subprocess.run([binary, "artisan", *args], env=env, capture_output=True, timeout=25)
    assert outcome.returncode == exit_code, (args, outcome.returncode, outcome.stdout, outcome.stderr)
    assert b"private-license" not in outcome.stdout + outcome.stderr
    return outcome


def request(port, path, trace_id="01234567890123456789012345678901"):
    connection = HTTPConnection("127.0.0.1", port, timeout=5)
    try:
        connection.request("GET", path, headers={
            "traceparent": f"00-{trace_id}-0123456789012345-01",
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
            assert request(port, "/api/probe/report") == 205
            assert request(port, "/api/probe/log") == 201
            assert request(port, "/api/probe/buffer-retain", "1" * 32) == 200
            assert request(port, "/api/probe/buffer-flush", "2" * 32) == 206
            assert request(port, "/api/probe/flush") == 204
            assert request(port, "/private-url-id") == 404
        finally:
            process.terminate()
            try:
                assert process.wait(timeout=15) == 0, (root / "http.log").read_text()
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
                raise


def main(binary, driver, queue):
    (ROOT / "target/tmp").mkdir(parents=True, exist_ok=True)
    with collector_server() as collector, collector_server() as other, tempfile.TemporaryDirectory(
        dir=ROOT / "target/tmp", prefix="new-relic-runtime-"
    ) as temp:
        root = Path(temp)
        collector.redirect_to = f"http://127.0.0.1:{other.server_port}/redirected"
        env = {key: value for key, value in os.environ.items()
               if not key.startswith(("NEW_RELIC_", "OTEL_", "DB_", "APP_", "QUEUE_", "AWS_", "SQS_"))}
        env |= {
            "TMPDIR": temp, "APP_BASE_PATH": temp, "APP_ENV": "testing", "APP_DEBUG": "false",
            "APP_DISABLED_RUNNERS": "queue,schedule", "APP_CONFIG_CACHE": str(root / "configuration.json"),
            "DB_CONNECTION": driver, "DB_DATABASE": str(root / "db.sqlite"),
            "NEW_RELIC_TEST_ALLOWED_UPSTREAM": f"http://127.0.0.1:{collector.server_port}/private-url-id?token=private-query",
            "NEW_RELIC_TEST_OTHER_UPSTREAM": f"http://127.0.0.1:{other.server_port}/private-url-id",
            "NEW_RELIC_TEST_REDIRECT_UPSTREAM": f"http://127.0.0.1:{collector.server_port}/redirect",
            "NEW_RELIC_TEST_BODY_UPSTREAM": f"http://127.0.0.1:{other.server_port}",
            "NEW_RELIC_PROPAGATE_ORIGINS": f"http://127.0.0.1:{collector.server_port}",
            "CACHE_STORE": "file", "SESSION_DRIVER": "array", "QUEUE_CONNECTION": queue,
            "NEW_RELIC_TEST_QUEUE": queue,
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
        if queue == "redis":
            env.update(REDIS_HOST="127.0.0.1", REDIS_PORT=os.environ["OPENRUST_TEST_REDIS_PORT"], REDIS_DB="0")
        if queue == "beanstalkd":
            env.update(BEANSTALKD_QUEUE_HOST="127.0.0.1",
                       BEANSTALKD_QUEUE_PORT=os.environ["OPENRUST_TEST_BEANSTALKD_PORT"])
        if queue == "sqs":
            endpoint = os.environ["OPENRUST_TEST_SQS_ENDPOINT"]
            env.update(AWS_ACCESS_KEY_ID="fixture", AWS_SECRET_ACCESS_KEY="fixture",
                       AWS_DEFAULT_REGION="us-east-1", AWS_EC2_METADATA_DISABLED="true",
                       SQS_ENDPOINT=endpoint, SQS_PREFIX=f"{endpoint}/000000000000")
        cli(binary, env, "telemetry:probe")
        assert collector.requests == [], "默认未启用的扩展发送了遥测"
        cli(binary, env, "migrate", "--force")
        cli(binary, env, "telemetry:enqueue")
        cli(binary, env, "telemetry:enqueue", "--fail")
        enabled = env | {"NEW_RELIC_ENABLED": "true", "NEW_RELIC_LOGS_ENABLED": "true",
                         "NEW_RELIC_EXCEPTION_STACKTRACE": "true"}

        collector.responses["/v1/traces"] = [503, 299]
        collector.responses["/v1/metrics"] = [299]
        collector.responses["/v1/logs"] = [299]
        web(binary, enabled, root)
        requests = collector.requests.copy()
        attempts = [item for item in requests if item[0] == "/v1/traces"]
        assert any(item[3] == 503 for item in attempts) and any(item[3] == 200 for item in attempts)
        spans, metrics, logs = decode(requests)
        http_spans = {attrs(span.attributes)["http.response.status_code"]: span for name, span in spans
                      if name == "acceptance-app" and span.kind == 2}
        assert {204, 404, 500} <= http_spans.keys(), http_spans
        assert http_spans[204].name == "GET /api/probe/{id}", http_spans[204]
        assert http_spans[204].trace_id.hex() == "01234567890123456789012345678901"
        assert http_spans[204].parent_span_id.hex() == "0123456789012345"
        assert http_spans[404].status.code == 0 and http_spans[404].name == "GET"
        assert http_spans[500].status.code == 2
        events = [event for event in http_spans[500].events if event.name == "exception"]
        assert len(events) == 1 and attrs(events[0].attributes).get("exception.type")
        assert "exception.stacktrace" not in attrs(events[0].attributes)
        reported = [event for event in http_spans[205].events if event.name == "exception"]
        assert len(reported) == 3, reported
        for event in reported:
            report_fields = attrs(event.attributes)
            assert "ReportProbe" in report_fields["exception.type"]
            assert report_fields["exception.stacktrace"] and "std::io" in report_fields["exception.cause_types"]
        assert http_spans[201].name == "logged transaction"
        assert http_spans[206].name == "GET /api/probe/{id}"
        assert attrs(http_spans[201].attributes).get("custom.operation") == "fixture"
        assert any(span.name == "log error" and span.parent_span_id == http_spans[201].span_id
                   for _, span in spans)
        log = next(log for name, log in logs
                   if name == "acceptance-app" and log.body.string_value == "linked fixture record")
        assert log.body.string_value == "linked fixture record", log
        assert log.trace_id == http_spans[201].trace_id and log.span_id == http_spans[201].span_id
        assert attrs(log.attributes).get("context.operation") == "sample", log
        assert "context.secret" not in attrs(log.attributes)
        buffered = {log.body.string_value: (log.trace_id, log.span_id) for _, log in logs}
        for message, status in (("buffered fixture record", 200), ("flush fixture record", 206)):
            span = http_spans[status]
            assert buffered[message] == (span.trace_id, span.span_id)
            assert message in (root / "parallel-logs.log").read_text()
        print("实际异常报告、脱敏及结构化日志 Trace 关联通过", flush=True)
        parent = http_spans[202]
        children = [span for name, span in spans if name == "acceptance-app"
                    and span.parent_span_id == parent.span_id and span.trace_id == parent.trace_id]
        http_children = [span for span in children if attrs(span.attributes).get("http.request.method") == "GET"]
        db_children = [span for span in children if attrs(span.attributes).get("db.system.name")]
        assert len(http_children) == 10 and len(db_children) >= 4
        assert any(span.end_time_unix_nano - span.start_time_unix_nano >= 220_000_000
                   and attrs(span.attributes).get("http.response.status_code") == 200
                   for span in http_children)
        client_errors = {attrs(span.attributes).get("error.type") for span in http_children
                         if span.status.code == 2}
        assert {"timeout", "response_body", "503"} <= client_errors, client_errors
        assert sum(event.name == "openrust.operation.cancelled" for span in http_children
                   for event in span.events) == 1
        assert any(span.status.code == 2 for span in db_children), db_children
        assert len(collector.received_traceparents) == 2
        assert all(header and header.split("-")[1] == parent.trace_id.hex()
                   for header in collector.received_traceparents)
        assert other.received_traceparents == [None] * 8
        assert {"http.client.request.duration", "db.client.operation.duration"} <= {metric.name for _, metric in metrics}
        print(f"{driver} 出站 HTTP 与 SQL 子链路、查询失败、跨 Origin 重定向及传播白名单通过", flush=True)
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
        start = len(collector.requests)
        collector.responses["/v1/traces"] = [429]
        cli(binary, artisan, "telemetry:probe")
        assert [item[3] for item in collector.requests[start:] if item[0] == "/v1/traces"] == [429, 200]
        cli(binary, artisan, "telemetry:probe", "private-command-input", "--fail", exit_code=1)
        shortcut_start = len(collector.requests)
        for arguments in ((), ("list",), ("help", "telemetry:probe"),
                          ("telemetry:probe", "--help"), ("--version",)):
            cli(binary, artisan, *arguments)
        shortcut_spans, _, _ = decode(collector.requests[shortcut_start:])
        assert sorted(span.name for _, span in shortcut_spans) == [
            "artisan help", "artisan help", "artisan list", "artisan list", "artisan version",
        ]
        print("独立 Artisan 的隐式 list、显式 list、help、--help 与 --version 通过", flush=True)
        if driver == "sqlite" and queue == "database":
            overload_start = len(collector.requests)
            collector.responses["/v1/traces"] = [298]
            collector.responses["/v1/logs"] = [298]
            overload = cli(binary, enabled, "telemetry:overload").stdout.decode()
            drops = next(line.split()[1:] for line in overload.splitlines() if line.startswith("drops "))
            dropped_spans, dropped_logs = map(int, drops)
            assert dropped_spans > 0 and dropped_logs > 0, drops
            overload_spans, _, overload_logs = decode(collector.requests[overload_start:])
            assert sum(span.name == "log error" for _, span in overload_spans) + dropped_spans == 4096
            assert len(overload_logs) + dropped_logs == 4096
            print(f"实际生成 4096 条错误日志和 4096 个错误 Span，限额内送达且过载计数吻合，"
                  f"丢弃 Span {dropped_spans}、日志 {dropped_logs}", flush=True)
        cli(binary, enabled | {"NEW_RELIC_APP_NAME": "acceptance-producer"}, "telemetry:enqueue")
        cli(binary, enabled | {"NEW_RELIC_APP_NAME": "acceptance-batch"}, "telemetry:batch")
        cli(binary, enabled | {"NEW_RELIC_APP_NAME": "acceptance-worker"},
            "queue:work", queue, "--queue=telemetry", "--stop-when-empty", "--tries=1")
        spans, metrics, logs = decode(collector.requests.copy())
        assert any(name == "acceptance-artisan" and span.name == "artisan telemetry:probe"
                   and span.status.code == 2 for name, span in spans)
        assert any(name == "acceptance-worker" and span.kind == 5
                   and attrs(span.attributes)["openrust.queue.result"] == "success" for name, span in spans)
        assert any(name == "acceptance-worker" and span.kind == 5
                   and span.status.code == 2
                   and attrs(span.attributes)["openrust.queue.result"] == "error" for name, span in spans)
        assert any(name == "acceptance-worker" and metric.name == "openrust.queue.job.duration"
                   for name, metric in metrics)
        worker_spans = [span for name, span in spans if name == "acceptance-worker"]
        job_ids = {span.span_id for span in worker_spans if span.name.startswith("queue ")}
        assert any(span.name == "artisan telemetry:probe" and span.parent_span_id in job_ids
                   for span in worker_spans), worker_spans
        produced = next(span for name, span in spans if name == "acceptance-producer" and span.kind == 4)
        assert any(span.kind == 5 and any(link.trace_id == produced.trace_id and link.span_id == produced.span_id
                                         for link in span.links) for span in worker_spans), worker_spans
        batch_links = {(span.trace_id, span.span_id) for name, span in spans
                       if name == "acceptance-batch" and span.name == "queue create"}
        worker_links = {(link.trace_id, link.span_id) for span in worker_spans for link in span.links}
        assert len(batch_links) == 2 and batch_links <= worker_links
        isolated = next(span for span in worker_spans if span.name == "queue execute telemetry.batch.item")
        assert any(span.kind == 5 and span.span_id == isolated.parent_span_id
                   and span.trace_id == isolated.trace_id for span in worker_spans)
        print(f"独立 Artisan、{queue}/{driver} Worker 的生产者／批量关联、隔离子进程、拒收和退出刷新通过", flush=True)

        if queue == "database" and driver == "sqlite":
            start = len(collector.requests)
            producer = enabled | {"NEW_RELIC_APP_NAME": "acceptance-lifecycle-producer"}
            worker = enabled | {"NEW_RELIC_APP_NAME": "acceptance-lifecycle-worker",
                                "QUEUE_WORKER_TIMED_OUT_EXIT_CODE": "77"}
            cli(binary, producer, "telemetry:lifecycle", "release-once")
            cli(binary, worker, "queue:work", queue, "--queue=telemetry", "--stop-when-empty", "--tries=2")
            cli(binary, producer, "telemetry:lifecycle", "hard-timeout")
            cli(binary, worker, "queue:work", queue, "--queue=telemetry", "--stop-when-empty", "--tries=1",
                exit_code=77)
            cli(binary, producer, "telemetry:lifecycle", "after-restart")
            cli(binary, worker, "queue:work", queue, "--queue=telemetry", "--stop-when-empty", "--tries=1")
            lifecycle_spans, lifecycle_metrics, _ = decode(collector.requests[start:])
            consumers = [span for name, span in lifecycle_spans
                         if name == "acceptance-lifecycle-worker" and span.kind == 5]
            outcomes = [(span, attrs(span.attributes)["openrust.queue.outcome"]) for span in consumers]
            assert sorted(result for _, result in outcomes) == [
                "processed", "processed", "released", "timed_out",
            ], [(span.name, attrs(span.attributes)) for span, _ in outcomes]
            released = next(span for span, result in outcomes if result == "released")
            assert released.links and any(list(span.links) == list(released.links)
                                          for span, result in outcomes if result == "processed")
            assert any(attrs(point.attributes).get("openrust.queue.outcome") == "timed_out"
                       for _, metric in lifecycle_metrics if metric.name == "openrust.queue.job.duration"
                       for point in metric.histogram.data_points)
            assert not any(span.name == "queue execute telemetry.batch.item" for _, span in lifecycle_spans)
            print("真实任务释放与重领、隔离任务强杀、Worker 退出与重启通过", flush=True)
            for lifecycle in ("deferred", "background", "null", "failover", "fallback"):
                start = len(collector.requests)
                connection = "failover" if lifecycle == "fallback" else lifecycle
                lifecycle_env = enabled | {"NEW_RELIC_TEST_QUEUE": connection}
                if lifecycle == "fallback":
                    lifecycle_env["NEW_RELIC_TEST_FAILOVER"] = "true"
                cli(binary, lifecycle_env, "telemetry:lifecycle", "lifecycle-driver")
                if lifecycle == "failover":
                    cli(binary, lifecycle_env, "queue:work", "database", "--queue=telemetry", "--stop-when-empty")
                deadline = time.monotonic() + 8
                while True:
                    spans, _, _ = decode(collector.requests[start:])
                    sends = [span for _, span in spans if span.kind == 4]
                    received = [span for _, span in spans if span.kind == 5]
                    if lifecycle != "background" or received or time.monotonic() >= deadline:
                        break
                    time.sleep(0.05)
                assert len(sends) == 1, (lifecycle, sends)
                if lifecycle == "null":
                    assert not received, received
                    continue
                assert len(received) == 1, (lifecycle, received)
                sent, job = sends[0], received[0]
                assert job.parent_span_id == sent.span_id or any(
                    link.span_id == sent.span_id and link.trace_id == sent.trace_id for link in job.links
                ), (lifecycle, job, sent)
            print("deferred、background、null、failover 首选及失败回退的生命周期链路通过", flush=True)

        start = len(collector.requests)
        cli(binary, enabled | {"NEW_RELIC_TEST_QUEUE": "sync"}, "telemetry:enqueue")
        sync_spans, _, _ = decode(collector.requests[start:])
        sender = next(span for _, span in sync_spans if span.kind == 4)
        assert any(span.kind == 5 and span.parent_span_id == sender.span_id for _, span in sync_spans)

        start = len(collector.requests)
        cli(binary, enabled | {"NEW_RELIC_TRACE_SAMPLE_RATIO": "0"}, "telemetry:probe")
        assert {item[0] for item in collector.requests[start:]} == {"/v1/metrics"}
        start = len(collector.requests)
        cli(binary, enabled | {"NEW_RELIC_METRICS_ENABLED": "false"}, "telemetry:probe")
        assert {item[0] for item in collector.requests[start:]} == {"/v1/traces"}
        start = len(collector.requests)
        cli(binary, enabled | {"NEW_RELIC_TRACES_ENABLED": "false"}, "telemetry:probe")
        assert {item[0] for item in collector.requests[start:]} == {"/v1/metrics"}
        print("sync 队列、独立采样及信号开关通过", flush=True)

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
    parser.add_argument("--queue", choices=("database", "redis", "beanstalkd", "sqs"), default="database")
    args = parser.parse_args()
    main(str(args.binary.resolve()), args.driver, args.queue)
