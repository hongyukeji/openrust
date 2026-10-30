#!/usr/bin/env python3
"""D7 唯一业务模块的真实 TCP 路由上下文验收，不构建、不连接数据库。

复用 D6 的来源快照、配置隔离、资源门限、进程组 supervisor 与清理机制；
临时目录沿用该机制的 openrust-c04d6-url-network 前缀以保持已审查的清理边界。
传入对应构建的二进制、SHA256 与构建前来源快照；PG/MySQL feature 构建分别执行。
工具仅核对来源未变与二进制摘要；二者的构建对应关系由外部构建记录证明。
"""
import hashlib
import http.client
import importlib.util
import json
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import time


def load_helper(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    # 执行既有 helper，避免导入时在仓库写 __pycache__。
    exec(compile(path.read_bytes(), str(path), 'exec'), module.__dict__)
    return module


network = load_helper(Path(__file__).with_name('verify-request-url.py'), 'route_network')


def worker_main(args):
    root, binary, owned = args.root.resolve(), args.binary.resolve(), args.owned.resolve()
    network.ensure(binary.is_file() and os.access(binary, os.X_OK), 'binary_not_executable')
    network.ensure(re.fullmatch('[0-9a-f]{64}', args.binary_sha256) is not None, 'binary_sha_invalid')
    network.ensure(network.sha(binary) == args.binary_sha256, 'binary_sha_mismatch')
    before = network.snapshot(root)
    snapshot_bytes = args.source_snapshot.read_bytes()
    network.ensure(json.loads(snapshot_bytes) == before, 'expected_source_snapshot_mismatch')
    network.ensure(owned.is_dir() and owned.stat().st_mode & 0o777 == 0o700, 'owned_directory_mode')
    bounded_path = root / '.agents/project/scripts/verify-cookie-security.py'
    network.ensure(network.sha(bounded_path) == network.TOOL_SHA, 'bounded_helper_changed')
    bounded = load_helper(bounded_path, 'route_bounded_response')
    report = {
        'accepted': False, 'cases': {}, 'owned_directory': str(owned),
        'binary_sha256': args.binary_sha256,
        'build_source_snapshot_sha256': hashlib.sha256(snapshot_bytes).hexdigest(),
        'helper_sha256': network.sha(Path(__file__).with_name('verify-request-url.py')),
        'excluded': ['build provenance without external build record', 'shared route slot observation on method miss', 'database parity', 'TLS', 'model or enum binding', 'complete Routing parity'],
        'limits': {'total_seconds': network.TOTAL, 'request_seconds': network.TIMEOUT,
                   'response_bytes': bounded.RESPONSE_LIMIT},
    }
    process = None
    (owned / 'source-before.json').write_text(json.dumps(before, sort_keys=True, indent=2) + '\n')

    def exchange(port, path, method='GET', body=None):
        connection = http.client.HTTPConnection('127.0.0.1', port, timeout=network.TIMEOUT)
        connection.response_class = bounded.BoundedResponse
        previous = signal.getitimer(signal.ITIMER_REAL)[0]
        network.ensure(previous > 0, 'request_without_total_deadline')
        started = time.monotonic()
        signal.setitimer(signal.ITIMER_REAL, min(previous, network.TIMEOUT))
        try:
            connection.request(method, path, body=body, headers={
                'Host': 'localhost', 'Connection': 'close', 'Content-Type': 'application/json',
                'X-Route-Name': 'forged', 'Authorization': 'Bearer never-echo',
            })
            response = connection.getresponse()
            content = response.read(bounded.RESPONSE_LIMIT + 1)
            headers = dict((key.lower(), value) for key, value in response.getheaders())
            network.ensure(len(content) <= bounded.RESPONSE_LIMIT, 'response_too_large')
            network.ensure(b'never-echo' not in content and 'never-echo' not in str(headers), 'sensitive_echo')
            return response.status, headers, json.loads(content) if content else None
        finally:
            connection.close()
            signal.setitimer(signal.ITIMER_REAL, max(.001, previous - (time.monotonic() - started)))

    def head_wire(port, path):
        # HTTPResponse 会按 HEAD 语义隐藏收到的正文，必须独立观察到 TCP EOF。
        previous = signal.getitimer(signal.ITIMER_REAL)[0]
        network.ensure(previous > 0, 'head_without_total_deadline')
        started = time.monotonic()
        signal.setitimer(signal.ITIMER_REAL, min(previous, network.TIMEOUT))
        try:
            with socket.create_connection(('127.0.0.1', port), timeout=network.TIMEOUT) as connection:
                connection.sendall((f'HEAD {path} HTTP/1.1\r\nHost: localhost\r\n'
                                    'Connection: close\r\n\r\n').encode('ascii'))
                chunks, total = [], 0
                while True:
                    chunk = connection.recv(min(8192, bounded.RESPONSE_LIMIT + 1 - total))
                    if not chunk:
                        break
                    chunks.append(chunk)
                    total += len(chunk)
                    network.ensure(total <= bounded.RESPONSE_LIMIT, 'head_wire_too_large')
                wire = b''.join(chunks)
            head, separator, body = wire.partition(b'\r\n\r\n')
            network.ensure(separator and len(head) <= 64 * 1024, 'head_headers_incomplete_or_large')
            lines = head.split(b'\r\n')
            status = re.fullmatch(rb'HTTP/1\.[01] ([0-9]{3}) [^\r\n]*', lines[0])
            network.ensure(status is not None and status.group(1) == b'200', 'head_wire_status')
            values = {}
            for line in lines[1:]:
                key, colon, value = line.partition(b':')
                network.ensure(colon and re.fullmatch(rb"[!#$%&'*+.^_`|~0-9A-Za-z-]+", key)
                               and b'\r' not in value and b'\n' not in value,
                               'head_wire_header_invalid')
                values.setdefault(key.lower(), []).append(value.strip())
            # HEAD 的 Content-Length 可表示 GET 大小；无论该值如何，线上不允许正文。
            network.ensure(body == b'', 'head_wire_extra_body')
            for key in (b'x-example-route-before', b'x-example-route-after'):
                network.ensure(values.get(key) == [b'example.route-context.show'], 'head_wire_route_context')
        finally:
            signal.setitimer(signal.ITIMER_REAL, max(.001, previous - (time.monotonic() - started)))

    signal.signal(signal.SIGALRM, network.alarm)
    signal.setitimer(signal.ITIMER_REAL, network.WORKER_LIMIT)
    try:
        report['before_resources'] = network.resources(root)
        cwd, config, port = network.prepare(root, owned, True)
        env = {key: value for key, value in os.environ.items() if key in ('PATH', 'HOME', 'TMPDIR')}
        process = subprocess.Popen([str(binary)], cwd=cwd,
            env=dict(env, APP_CONFIG_DIR=str(config), APP_ENV='test', RUST_LOG='off'),
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        network.ensure(os.getpgid(process.pid) == os.getpgrp(), 'owned_group_mismatch')
        until = time.monotonic() + 15
        while True:
            network.ensure(process.poll() is None, 'origin_exited')
            network.ensure(time.monotonic() < until, 'origin_readiness_timeout')
            if network.listener_owned(port, process):
                break
            time.sleep(.1)
        base = '/api/example/route-context'
        for kind, named in [('named', True), ('anonymous', False)]:
            status, headers, document = exchange(port, f'{base}/{kind}/42?id=query&absent=query')
            network.ensure(status == 200, 'route_status_' + kind)
            data = document['data']
            name = 'example.route-context.show' if named else None
            network.ensure(data['name'] == name and data['is_example'] is named, 'route_identity_' + kind)
            network.ensure(data['uri'] == f'api/example/route-context/{kind}/{{id}}', 'final_uri_' + kind)
            network.ensure(data['id'] == '42' and data['parameters'] == [['id', '42']], 'route_parameter_' + kind)
            network.ensure(data['absent'] is None and data['default'] == 'public-default', 'parameter_default_' + kind)
            network.ensure(data['methods'] == (['GET', 'HEAD', 'POST'] if named else ['GET', 'HEAD']), 'route_methods_' + kind)
            for key in ['x-example-route-before', 'x-example-route-after']:
                network.ensure(headers.get(key) == name, 'middleware_identity_' + kind)
            report['cases'][kind + '_identity_parameter_default_query_spoof'] = True
        status, _, document = exchange(port, base + '/named/43?id=query', 'POST', b'{"id":"body","absent":"body"}')
        network.ensure(status == 200 and document['data']['id'] == '43'
                       and document['data']['default'] == 'public-default', 'body_cannot_override_parameters')
        report['cases']['body_cannot_override_parameters'] = True
        for record, expected in [('missing', 404), ('blocked', 403)]:
            status, headers, _ = exchange(port, f'{base}/named/{record}')
            network.ensure(status == expected, 'business_status_' + record)
            network.ensure(all(headers.get(key) == 'example.route-context.show'
                for key in ['x-example-route-before', 'x-example-route-after']), 'business_context_' + record)
            report['cases']['business_context_' + record] = True
        head_wire(port, base + '/named/42')
        report['cases']['head_wire_empty_until_eof_with_before_after_context'] = True
        status, headers, _ = exchange(port, base + '/named/44', 'PUT')
        network.ensure(status == 405 and 'x-example-route-before' not in headers
                       and 'x-example-route-after' not in headers, 'method_miss_has_no_route_middleware')
        report['cases']['method_miss_has_no_route_middleware'] = True
        report['accepted'] = True
    except Exception as error:
        report['failure_type'] = type(error).__name__
        if isinstance(error, network.Failure):
            report['failure_tag'] = str(error)
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        errors = []
        if process is not None:
            try:
                exit_info = network.stop(process)
                report['backend_exit'] = exit_info
                network.ensure(exit_info['code'] == 0 and not exit_info['forced'], 'origin_shutdown_not_graceful')
            except Exception as error:
                errors.append(type(error).__name__)
        try:
            after = network.snapshot(root)
            report['source_changed'] = sorted(key for key in set(before) | set(after) if before.get(key) != after.get(key))
            (owned / 'source-after.json').write_text(json.dumps(after, sort_keys=True, indent=2) + '\n')
            report['binary_unchanged'] = network.sha(binary) == args.binary_sha256
            report['after_resources'] = network.resources(root)
            previous = report.get('before_resources', {}).get('target_bytes', {})
            current = report['after_resources']['target_bytes']
            network.ensure({k: v for k, v in previous.items() if k != str(root / 'target')}
                == {k: v for k, v in current.items() if k != str(root / 'target')}, 'consumer_targets_changed')
            network.clear_sensitive(owned)
        except Exception as error:
            errors.append(type(error).__name__)
        report['cleanup_errors'] = errors
        report['accepted'] = bool(report['accepted'] and not errors and not report.get('source_changed', True)
                                  and report.get('binary_unchanged'))
        (owned / 'result.json').write_text(json.dumps(report, sort_keys=True, indent=2) + '\n')
    return 0 if report['accepted'] else 1


if __name__ == '__main__':
    # 保留同一 supervisor 和 worker 调用协议，只替换本簇的业务验收函数。
    network.__file__ = __file__
    network.__doc__ = __doc__
    network.worker_main = worker_main
    raise SystemExit(network.main())
