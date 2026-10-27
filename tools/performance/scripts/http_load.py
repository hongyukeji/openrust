#!/usr/bin/env python3
"""对指定 HTTP endpoint 执行有界并发负载，并输出延迟与状态分布。"""

from __future__ import annotations

import argparse
import collections
import concurrent.futures
import http.client
import json
import math
import platform
import ssl
import threading
import time
import urllib.parse


def percentile(sorted_values: list[float], quantile: float) -> float | None:
    if not sorted_values:
        return None
    index = max(0, math.ceil(len(sorted_values) * quantile) - 1)
    return sorted_values[index]


def run_round(
    parts: urllib.parse.SplitResult,
    method: str,
    body: bytes | None,
    headers: dict[str, str],
    concurrency: int,
    request_count: int,
    timeout: float,
) -> dict[str, object]:
    path = urllib.parse.urlunsplit(("", "", parts.path or "/", parts.query, ""))
    per_worker, remainder = divmod(request_count, concurrency)
    barrier = threading.Barrier(concurrency)

    def worker(worker_index: int) -> tuple[collections.Counter[str], list[float]]:
        count = per_worker + (1 if worker_index < remainder else 0)
        statuses: collections.Counter[str] = collections.Counter()
        latencies: list[float] = []
        connection: http.client.HTTPConnection | http.client.HTTPSConnection
        if parts.scheme == "https":
            connection = http.client.HTTPSConnection(
                parts.hostname,
                parts.port,
                timeout=timeout,
                context=ssl.create_default_context(),
            )
        else:
            connection = http.client.HTTPConnection(parts.hostname, parts.port, timeout=timeout)
        barrier.wait()
        for _ in range(count):
            started = time.perf_counter()
            try:
                connection.request(method, path, body=body, headers=headers)
                response = connection.getresponse()
                response.read()
                statuses[str(response.status)] += 1
                latencies.append((time.perf_counter() - started) * 1000)
            except TimeoutError:
                statuses["timeout"] += 1
                connection.close()
                connection = new_connection(parts, timeout)
            except (OSError, http.client.HTTPException):
                statuses["connection_or_protocol_error"] += 1
                connection.close()
                connection = new_connection(parts, timeout)
        connection.close()
        return statuses, latencies

    started = time.perf_counter()
    with concurrent.futures.ThreadPoolExecutor(max_workers=concurrency) as executor:
        results = list(executor.map(worker, range(concurrency)))
    elapsed = time.perf_counter() - started
    statuses: collections.Counter[str] = collections.Counter()
    latencies: list[float] = []
    for worker_statuses, worker_latencies in results:
        statuses.update(worker_statuses)
        latencies.extend(worker_latencies)
    latencies.sort()
    return {
        "requests": request_count,
        "elapsed_seconds": round(elapsed, 3),
        "requests_per_second": round(request_count / elapsed, 2) if elapsed else 0,
        "statuses": dict(sorted(statuses.items())),
        "latency_ms": {
            "p50": round(percentile(latencies, 0.50), 3) if latencies else None,
            "p95": round(percentile(latencies, 0.95), 3) if latencies else None,
            "p99": round(percentile(latencies, 0.99), 3) if latencies else None,
        },
    }


def new_connection(
    parts: urllib.parse.SplitResult, timeout: float
) -> http.client.HTTPConnection | http.client.HTTPSConnection:
    if parts.scheme == "https":
        return http.client.HTTPSConnection(
            parts.hostname,
            parts.port,
            timeout=timeout,
            context=ssl.create_default_context(),
        )
    return http.client.HTTPConnection(parts.hostname, parts.port, timeout=timeout)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="执行固定请求数的 HTTP 并发负载并报告状态与尾延迟。")
    parser.add_argument("--url", required=True, help="目标 HTTP/HTTPS URL；凭据不会写入输出。")
    parser.add_argument("--requests", type=int, default=10000, help="每轮请求数，默认 10000。")
    parser.add_argument("--concurrency", type=int, default=32, help="并发连接数，默认 32。")
    parser.add_argument("--rounds", type=int, default=3, help="测量轮数，默认 3。")
    parser.add_argument("--timeout", type=float, default=10.0, help="单次请求超时秒数，默认 10。")
    parser.add_argument("--method", default="GET", help="HTTP 方法，默认 GET。")
    parser.add_argument("--body", help="可选 UTF-8 请求正文。")
    parser.add_argument(
        "--header", action="append", default=[], metavar="NAME: VALUE", help="请求头，可重复指定。"
    )
    parser.add_argument("--pause-seconds", type=float, default=1.0, help="轮次间隔秒数，默认 1。")
    args = parser.parse_args()
    parts = urllib.parse.urlsplit(args.url)
    if (
        parts.scheme not in {"http", "https"}
        or not parts.hostname
        or parts.username is not None
        or parts.password is not None
    ):
        parser.error("--url 必须是无内嵌凭据的 HTTP/HTTPS URL")
    if not (1 <= args.concurrency <= 4096 and args.concurrency <= args.requests <= 10_000_000):
        parser.error("请求数须不少于并发数，且并发为 1..=4096、请求数不超过 10000000")
    if not (1 <= args.rounds <= 20) or args.timeout <= 0 or args.pause_seconds < 0:
        parser.error("轮数须为 1..=20，超时须为正数，轮次间隔不能为负数")
    return args


def main() -> int:
    args = parse_args()
    parts = urllib.parse.urlsplit(args.url)
    headers: dict[str, str] = {}
    for item in args.header:
        name, separator, value = item.partition(":")
        if not separator or not name.strip():
            raise SystemExit("请求头格式应为 NAME: VALUE")
        headers[name.strip()] = value.strip()
    body = args.body.encode("utf-8") if args.body is not None else None
    if body is not None and not any(name.lower() == "content-length" for name in headers):
        headers["Content-Length"] = str(len(body))

    report: dict[str, object] = {
        "target": {"scheme": parts.scheme, "host": parts.hostname, "port": parts.port},
        "client": {"os": platform.platform(), "python": platform.python_version()},
        "method": args.method.upper(),
        "concurrency": args.concurrency,
        "rounds": [],
        "note": "脚本不采集服务进程 CPU/RSS；请按服务可执行文件 PID 单独采样。",
    }
    rounds = report["rounds"]
    assert isinstance(rounds, list)
    for index in range(args.rounds):
        rounds.append(
            run_round(
                parts,
                args.method.upper(),
                body,
                headers,
                args.concurrency,
                args.requests,
                args.timeout,
            )
        )
        if index + 1 < args.rounds and args.pause_seconds:
            time.sleep(args.pause_seconds)
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
